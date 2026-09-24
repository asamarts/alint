//! `sync_from` — the first *cross-file* content fix op. Overwrites a drifted
//! target with its canonical source so a `cross_file` `relation: identical` rule
//! converges. Unlike the same-file content fixers (hygiene / `replace`), the
//! bytes come from a DIFFERENT file (the host rule's `source:`), which the fixer
//! carries; the target is the violating path.
//!
//! Trust: it writes another file's bytes wholesale, and the ruleset's `source:`
//! chooses which file overwrites which, so it is content-injecting — a remote
//! `extends:` the user has not listed in `trusted_extends:` demotes it to a
//! `suggestion` (auto-fix.md 5.5, `alint_dsl::CONTENT_INJECTING_FIX_OPS`). It is
//! **`Unsafe` by default** (a whole-file overwrite can discard uncommitted target
//! content), so a bare `alint fix` only *suggests* it; `--unsafe-fixes` applies it.

use std::path::{Path, PathBuf};

use alint_core::located_fix::{LocatedEdit, LocatedOutcome, apply_file_edits};
use alint_core::{
    Applicability, Error, Extract, FixContext, FixEdit, FixOutcome, Fixer, Format, ReadForFix,
    Result, Violation, extract_values, is_non_literal, read_for_fix,
};
use serde_json_path::JsonPath;

use crate::cross_file::{Normalize, apply_normalize};
use crate::fixers::StructuredFixer;
use crate::fixers::creators::confine_fix_path;

/// Overwrites the violating (drifted) target with the bytes of the host rule's
/// canonical `source:`, making the two byte-identical. A *content-injecting*
/// fixer, `Unsafe` by default. Byte-level: it mirrors any content (text or
/// binary), matching the `identical` relation's byte comparison.
#[derive(Debug)]
pub struct SyncFromFixer {
    /// The canonical source file (repo-relative), from the host `cross_file`
    /// rule's `source.file`. The whole file is copied over each drifted target.
    source: PathBuf,
    applicability: Applicability,
}

impl SyncFromFixer {
    #[must_use]
    pub fn new(source: PathBuf, applicability: Applicability) -> Self {
        Self {
            source,
            applicability,
        }
    }

    /// Confine both endpoints to the repo root, rejecting a self-copy. Shared by
    /// `apply` and `fix_edit`. Returns the absolute (source, target) pair, or a
    /// reason string on an escape / self-copy (for `apply` to surface as a Skip;
    /// `fix_edit` maps it to `None`). `allow` is the rule's `allow_out_of_root`.
    fn resolve_endpoints(
        &self,
        target_rel: &Path,
        root: &Path,
        allow: bool,
    ) -> std::result::Result<(PathBuf, PathBuf), String> {
        // Both the READ (source) and the WRITE (target) must stay in the tree: an
        // absolute path discards `root` on join, and a symlinked parent escapes
        // `create`/`read` -- without confinement an untrusted ruleset could
        // exfiltrate (`source: /etc/passwd`) or overwrite (`target: ../x`)
        // out-of-tree. Same gate the same-file content fixers use.
        let source_abs = confine_fix_path(&self.source, root, allow)?;
        let target_abs = confine_fix_path(target_rel, root, allow)?;
        // A file is trivially identical to itself: never overwrite a target with
        // its own bytes (a no-op that would still churn the diff). Not reachable
        // from a real violation (`check` finds a self-referential target already
        // identical, so it never fires), but guard defensively.
        if source_abs == target_abs {
            return Err("the source and target are the same file (nothing to sync)".to_string());
        }
        Ok((source_abs, target_abs))
    }
}

impl Fixer for SyncFromFixer {
    fn describe(&self) -> String {
        format!(
            "overwrite the drifted file with the canonical {}",
            self.source.display()
        )
    }

    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(target) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let target_rel: &Path = target;
        let (source_abs, target_abs) =
            match self.resolve_endpoints(target_rel, ctx.root, ctx.allow_out_of_root) {
                Ok(pair) => pair,
                Err(reason) => return Ok(FixOutcome::Skipped(reason)),
            };
        // Source read is compose-aware (an earlier fixer in this pass may have
        // rewritten the canonical file) and size-capped. A missing / unreadable
        // source is a clean Skip, not a hard error: `check` already reports it as a
        // source-side violation, and the fix genuinely cannot proceed.
        let source_bytes = match read_for_fix(&source_abs, &self.source, ctx) {
            Ok(ReadForFix::Bytes(b)) => b,
            Ok(ReadForFix::Skipped(outcome)) => return Ok(outcome),
            Err(_) => {
                return Ok(FixOutcome::Skipped(format!(
                    "canonical source {} is missing or unreadable",
                    self.source.display()
                )));
            }
        };
        // Target read (also compose-aware, so a fixpoint pass sees the write it
        // made last pass). If already identical, this pass is a no-op -- that Skip
        // is the fixpoint's idempotence signal, so `sync_from` settles in <=2
        // passes rather than looping to the cap.
        let target_bytes = match read_for_fix(&target_abs, target_rel, ctx)? {
            ReadForFix::Bytes(b) => b,
            ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        if source_bytes == target_bytes {
            return Ok(FixOutcome::Skipped(format!(
                "{} is already identical to {}",
                target_rel.display(),
                self.source.display()
            )));
        }
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would sync {} from {}",
                target_rel.display(),
                self.source.display()
            )));
        }
        ctx.commit_write(&target_abs, &source_bytes)
            .map_err(|source| Error::Io {
                path: target_abs.clone(),
                source,
            })?;
        Ok(FixOutcome::Applied(format!(
            "synced {} from {}",
            target_rel.display(),
            self.source.display()
        )))
    }

    fn fix_edit(&self, violation: &Violation, _bytes: &[u8], root: &Path) -> Option<FixEdit> {
        let target_rel = violation.path.as_deref()?;
        // `fix_edit` has no `allow_out_of_root` (no `ctx`): confine to the root
        // strictly, so the editor / SARIF / agent surfaces never propose an
        // out-of-root read or write. An `allow_out_of_root` sync is still applied
        // by `apply`; only the proposed-edit form declines it.
        let (source_abs, _target_abs) = self.resolve_endpoints(target_rel, root, false).ok()?;
        // Read the source from disk (capped). `_bytes` (the target's current
        // content) is deliberately ignored: the two suggestion sites call
        // `fix_edit(.., &[], root)` with EMPTY bytes, so a `bytes`-equality
        // short-circuit would be wrong. A live violation already means the files
        // differ, so no equality gate is needed here.
        let source_bytes = crate::io::read_capped(&source_abs).ok()?;
        Some(FixEdit::SetContent {
            path: target_rel.to_path_buf(),
            content: source_bytes,
        })
    }
}

/// How to locate the value in a propagation target: the target's `extract`, which
/// is either `Structured(Format, JSONPath)` (rewrite the located node) or
/// `Regex(pattern)` (rewrite each match's capture group 1). A glob shares one
/// across every match; a list carries one per entry. Non-structured/regex extracts
/// (lines / whole-file) are rejected at load.
#[derive(Debug)]
pub enum ValueTargets {
    Glob(Extract),
    List(Vec<(PathBuf, Extract)>),
}

/// Propagates the host `cross_file` `relation: equals` source's single extracted
/// scalar into each drifting target's node, per format. A whole-file `apply`
/// fixer (NOT a located `collect_edits` one -- the engine's located branch assumes
/// per-file hosts, and `cross_file` is `requires_full_index`, so a located fixer
/// there would escape the `--changed` blast radius). It reuses the located
/// resolver INTERNALLY: it builds a `StructuredFixer::set` for the target node,
/// takes its located edit, and applies + verifies it via `apply_file_edits`, then
/// `commit_write`s the result -- so all the per-format locate / serialize /
/// `PutGet` machinery is reused with no engine change. `Unsafe` by default,
/// content-injecting (the ruleset's `source:` chooses which value overwrites
/// which node, so an untrusted remote demotes it -- W2 covers `sync_from`).
#[derive(Debug)]
pub struct CrossFileValueFixer {
    /// The canonical source file (repo-relative) and how to extract its scalar.
    source_file: PathBuf,
    source_extract: Extract,
    targets: ValueTargets,
    /// The host rule's `normalize` transforms. The fixer MUST correlate to the
    /// check: only rewrite a target value the check flagged as drift, i.e. one
    /// whose NORMALIZED form differs from the normalized source (a `2.5.7` capture
    /// the check accepted under `semver-minor` must not be clobbered to `2.5.0`).
    normalize: Vec<Normalize>,
    applicability: Applicability,
}

impl CrossFileValueFixer {
    // `pub(crate)`: constructed only by the `cross_file` builder (and tests). The
    // signature carries the crate-internal `Normalize`, so a `pub` constructor
    // would leak a more-private type (private_interfaces).
    #[must_use]
    pub(crate) fn new(
        source_file: PathBuf,
        source_extract: Extract,
        targets: ValueTargets,
        normalize: Vec<Normalize>,
        applicability: Applicability,
    ) -> Self {
        Self {
            source_file,
            source_extract,
            targets,
            normalize,
            applicability,
        }
    }

    /// Whether the target value `current` is DRIFT the check would flag: a LITERAL
    /// value (the check skips interpolated `${...}` values as notes) whose
    /// normalized form differs from the normalized source. The fixer rewrites only
    /// these -- never a non-literal template (would clobber it) or a normalize-equal
    /// value the check deemed correct (audit: located-fixer correlation).
    fn is_drift(&self, current: &str, source_norm: &str) -> bool {
        !is_non_literal(current) && apply_normalize(&self.normalize, current) != *source_norm
    }

    /// The `extract` for `target_rel`: the shared glob extract, or the matching
    /// list entry's. `None` when the violation path is not a configured target
    /// (should not happen -- the violation came from this rule).
    fn target_extract(&self, target_rel: &Path) -> Option<&Extract> {
        match &self.targets {
            ValueTargets::Glob(ex) => Some(ex),
            ValueTargets::List(entries) => entries
                .iter()
                .find(|(p, _)| p == target_rel)
                .map(|(_, ex)| ex),
        }
    }

    /// Read the source file and extract its single literal scalar (the value to
    /// propagate). Mirrors `check_equals`: filter non-literal (interpolated)
    /// values, then require exactly one -- else there is nothing to propagate and
    /// the fixer Skips (consistent with what `check` already reported).
    fn source_scalar(&self, ctx: &FixContext<'_>) -> std::result::Result<String, String> {
        let source_abs = confine_fix_path(&self.source_file, ctx.root, ctx.allow_out_of_root)?;
        let source_bytes = match read_for_fix(&source_abs, &self.source_file, ctx) {
            Ok(ReadForFix::Bytes(b)) => b,
            Ok(ReadForFix::Skipped(FixOutcome::Skipped(r) | FixOutcome::Applied(r))) => {
                return Err(r);
            }
            Err(_) => {
                return Err(format!(
                    "canonical source {} is missing or unreadable",
                    self.source_file.display()
                ));
            }
        };
        let text = String::from_utf8_lossy(&source_bytes);
        let values = extract_values(&self.source_extract, &text)
            .map_err(|e| format!("source extract failed: {e}"))?;
        let mut literals = values.into_iter().filter(|v| !is_non_literal(v));
        match (literals.next(), literals.next()) {
            (Some(one), None) => Ok(one),
            _ => Err("source did not resolve to exactly one literal value".to_string()),
        }
    }

    /// Build the located edit for the target node (reusing `StructuredFixer::set`
    /// to locate + serialize), apply + verify it (`apply_file_edits` runs the
    /// `Structured` `PutGet` check and demotes a node that cannot be set), and return
    /// the new WHOLE-FILE bytes. `Err(reason)` for any decline -- an invalid query,
    /// an already-equal / not-a-single-scalar / not-representable node (the
    /// empty-edit cases), a post-edit verify failure, or a no-op -- each a clean
    /// Skip so `check` and `fix` agree. The whole-file write (`commit_write` in
    /// `apply`) keeps this on the engine's blast-radius-demoted path, unlike a
    /// located fixer on this `requires_full_index` rule.
    ///
    /// Dispatches on the target's `extract`: a `Structured` node (rewrite the
    /// located node) or a `Regex` (rewrite each match's capture group 1).
    fn propagated_bytes(
        &self,
        violation: &Violation,
        extract: &Extract,
        source_value: &str,
        target_bytes: &[u8],
        root: &Path,
    ) -> std::result::Result<Vec<u8>, String> {
        match extract {
            Extract::Structured(format, query) => self.propagate_structured(
                violation,
                *format,
                query,
                source_value,
                target_bytes,
                root,
            ),
            Extract::Regex(pattern) => {
                self.propagate_regex(violation, pattern, source_value, target_bytes)
            }
            // build_value_targets rejects every other extract, so this is unreachable
            // for a built rule.
            _ => Err("unsupported target extract for value propagation".to_string()),
        }
    }

    /// Regex-extract target (Phase 2): rewrite EACH match's capture group 1 to the
    /// source value, then re-extract to verify the whole file still yields exactly
    /// the source value at that pattern (a source value carrying a char that breaks
    /// the surrounding pattern -- e.g. a `"` inside a `"([^"]+)"` capture -- fails
    /// this and declines, never writing a value the check would still reject).
    fn propagate_regex(
        &self,
        violation: &Violation,
        pattern: &str,
        source_value: &str,
        target_bytes: &[u8],
    ) -> std::result::Result<Vec<u8>, String> {
        let target_rel = violation.path.as_deref().unwrap_or_else(|| Path::new(""));
        // A capture rewrite needs byte offsets into the real bytes; a lossy decode
        // would shift them, so require valid UTF-8 (decline otherwise -- the check
        // reads lossily, but a fix must not splice at a shifted offset).
        let text = std::str::from_utf8(target_bytes).map_err(|_| {
            format!(
                "{} is not valid UTF-8; cannot rewrite a regex capture",
                target_rel.display()
            )
        })?;
        let re = regex::Regex::new(pattern)
            .map_err(|e| format!("invalid target regex `{pattern}`: {e}"))?;
        let source_norm = apply_normalize(&self.normalize, source_value);
        // A `ReplaceRange` over each DRIFTING match's group-1 span. Disjoint +
        // left-to-right (captures_iter is non-overlapping, leftmost), so
        // `apply_file_edits` splices them in one pass. Rewrite a capture ONLY if the
        // check flagged it as drift (`is_drift`): SKIP a non-literal `${...}`
        // template (clobbering it would hardcode a computed value the check leaves
        // alone) and a normalize-equal capture (one the check deemed correct).
        let mut collected = Vec::new();
        let mut any_match = false;
        let mut had_group = false;
        for cap in re.captures_iter(text) {
            any_match = true;
            let Some(g1) = cap.get(1) else { continue };
            had_group = true;
            if !self.is_drift(g1.as_str(), &source_norm) {
                continue;
            }
            collected.push(alint_core::CollectedEdit {
                edit: FixEdit::ReplaceRange {
                    path: target_rel.to_path_buf(),
                    range: g1.start()..g1.end(),
                    content: source_value.as_bytes().to_vec(),
                },
                applicability: self.applicability,
                verify: alint_core::EditVerifier::None,
                isolation_group: None,
            });
        }
        if !any_match {
            return Err(format!(
                "the target regex `{pattern}` matched nothing in {}",
                target_rel.display()
            ));
        }
        if !had_group {
            return Err(format!(
                "the target regex `{pattern}` has no capture group 1 to set on {}",
                target_rel.display()
            ));
        }
        if collected.is_empty() {
            return Err(format!(
                "{}: nothing to propagate at regex `{pattern}` (every capture already \
                 matches {source_value:?}, is a non-literal template, or is normalize-equal)",
                target_rel.display()
            ));
        }
        let batch: Vec<LocatedEdit> = collected
            .into_iter()
            .enumerate()
            .map(|(i, collected)| LocatedEdit {
                rule_index: 0,
                violation_index: i,
                collected,
            })
            .collect();
        let (new_bytes, outcomes) = apply_file_edits(target_bytes, batch, self.applicability);
        if !outcomes
            .iter()
            .any(|(_, o)| matches!(o, LocatedOutcome::Applied))
        {
            return Err(format!(
                "the target regex `{pattern}` capture could not be set on {}",
                target_rel.display()
            ));
        }
        // Re-extract verify, mirroring the check: after the splice, every LITERAL
        // capture must NORMALIZE-EQUAL the source (non-literal captures are skipped
        // by the check, so ignore them here too), and at least one literal must
        // remain. A source value carrying a char that truncates the surrounding
        // pattern fails this -> decline (never write a value the check still flags).
        let re_new = extract_values(
            &Extract::Regex(pattern.to_string()),
            &String::from_utf8_lossy(&new_bytes),
        )
        .map_err(|e| format!("regex re-extract failed: {e}"))?;
        let literal_new: Vec<&String> = re_new.iter().filter(|v| !is_non_literal(v)).collect();
        if literal_new.is_empty()
            || !literal_new
                .iter()
                .all(|v| apply_normalize(&self.normalize, v) == source_norm)
        {
            return Err(format!(
                "setting {} to {source_value:?} would not satisfy the regex `{pattern}` \
                 (the value likely contains a char the pattern's capture cannot hold)",
                target_rel.display()
            ));
        }
        Ok(new_bytes)
    }

    /// Structured-extract target (Phase 1): rewrite the located node via
    /// `StructuredFixer::set`.
    fn propagate_structured(
        &self,
        violation: &Violation,
        format: Format,
        query: &str,
        source_value: &str,
        target_bytes: &[u8],
        root: &Path,
    ) -> std::result::Result<Vec<u8>, String> {
        let target_rel = violation.path.as_deref().unwrap_or_else(|| Path::new(""));
        let path_expr =
            JsonPath::parse(query).map_err(|_| format!("invalid target query `{query}`"))?;
        let target_text = String::from_utf8_lossy(target_bytes);
        // TYPE-PRESERVATION GUARD (audit F1): the propagated value is always a
        // STRING (cross-file extraction yields text), so setting a NUMBER / BOOL
        // node in a typed format (json/toml/yaml/hcl) would silently change its
        // type to a quoted string. Worse, the `equals` check compares only string
        // leaves, so a same-typed node would keep firing anyway -- a string
        // coercion is the only thing that "converges", masking the mismatch as a
        // fix. Decline instead; a numeric/bool pin belongs in a same-file
        // `set_value` with a typed `equals:`. (String-leaf formats -- xml / dotenv /
        // ini / properties -- parse every leaf as a string, so this never fires
        // there.)
        if let Ok(parsed) = format.parse(&target_text) {
            let located = path_expr.query_located(&parsed);
            if located.len() == 1 {
                if let Some(node) = located.iter().next() {
                    let n = node.node();
                    if n.is_number() || n.is_boolean() {
                        return Err(format!(
                            "{} `{query}` is a {} node; `sync_from` on `equals` would change it \
                             to a string (the propagated value is text). Pin a numeric/bool value \
                             with a same-file `set_value` (typed `equals:`) instead.",
                            target_rel.display(),
                            if n.is_number() { "numeric" } else { "boolean" }
                        ));
                    }
                    // Correlate to the check (audit): NEVER clobber a non-literal
                    // `${...}` template node (the check skips it as a note), and skip
                    // a node the check deemed correct under `normalize`.
                    if let Some(s) = n.as_str() {
                        if is_non_literal(s) {
                            return Err(format!(
                                "{} `{query}` is a non-literal template ({s:?}); `sync_from` \
                                 leaves interpolated values alone",
                                target_rel.display()
                            ));
                        }
                        let source_norm = apply_normalize(&self.normalize, source_value);
                        if apply_normalize(&self.normalize, s) == source_norm {
                            return Err(format!(
                                "{} `{query}` already equals {source_value:?} (under normalize)",
                                target_rel.display()
                            ));
                        }
                    }
                }
            }
        }
        let delegate = StructuredFixer::set(
            format,
            path_expr,
            query.to_string(),
            serde_json::Value::String(source_value.to_string()),
            self.applicability,
        );
        let collected = delegate.collect_edits(
            std::slice::from_ref(violation),
            target_rel,
            target_bytes,
            root,
        );
        if collected.is_empty() {
            // An empty edit set is EITHER an already-equal target (setting it to the
            // same value reserializes byte-identical -> a no-op edit) OR a genuine
            // locate/serialize failure (no single scalar node, or the value is not
            // representable in this format). Re-extract the target to tell them apart
            // so the skip reason is honest (and idempotence reads clearly).
            let already = extract_values(
                &Extract::Structured(format, query.to_string()),
                &target_text,
            )
            .is_ok_and(|vals| vals.iter().any(|v| v == source_value));
            return Err(if already {
                format!(
                    "{} already equals {source_value:?} at `{query}`",
                    target_rel.display()
                )
            } else {
                format!(
                    "could not set {} at `{query}` to {source_value:?} (no single scalar node, \
                     or the value is not representable in {format:?})",
                    target_rel.display()
                )
            });
        }
        let batch: Vec<LocatedEdit> = collected
            .into_iter()
            .enumerate()
            .map(|(i, collected)| LocatedEdit {
                rule_index: 0,
                violation_index: i,
                collected,
            })
            .collect();
        let (new_bytes, outcomes) = apply_file_edits(target_bytes, batch, self.applicability);
        if !outcomes
            .iter()
            .any(|(_, o)| matches!(o, LocatedOutcome::Applied))
        {
            // Every edit was demoted (post-edit verify declined) or dropped: do NOT
            // write. Consistent with `check` -- the violation stands.
            return Err(format!(
                "the target value at `{query}` could not be set (post-edit verify declined)"
            ));
        }
        if new_bytes.as_slice() == target_bytes {
            return Err(format!(
                "{} already equals {source_value:?} at `{query}`",
                target_rel.display()
            ));
        }
        Ok(new_bytes)
    }
}

impl Fixer for CrossFileValueFixer {
    fn describe(&self) -> String {
        format!(
            "propagate the value from the canonical {}",
            self.source_file.display()
        )
    }

    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(target) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let target_rel: &Path = target;
        let Some(extract) = self.target_extract(target_rel) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} is not a configured value-propagation target",
                target_rel.display()
            )));
        };
        let target_abs = match confine_fix_path(target_rel, ctx.root, ctx.allow_out_of_root) {
            Ok(p) => p,
            Err(reason) => return Ok(FixOutcome::Skipped(reason)),
        };
        let source_value = match self.source_scalar(ctx) {
            Ok(v) => v,
            Err(reason) => return Ok(FixOutcome::Skipped(reason)),
        };
        let target_bytes = match read_for_fix(&target_abs, target_rel, ctx)? {
            ReadForFix::Bytes(b) => b,
            ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        let new_bytes =
            match self.propagated_bytes(violation, extract, &source_value, &target_bytes, ctx.root)
            {
                Ok(b) => b,
                Err(reason) => return Ok(FixOutcome::Skipped(reason)),
            };
        let at = locator(extract);
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would set {} `{at}` to {source_value:?} from {}",
                target_rel.display(),
                self.source_file.display()
            )));
        }
        ctx.commit_write(&target_abs, &new_bytes)
            .map_err(|source| Error::Io {
                path: target_abs.clone(),
                source,
            })?;
        Ok(FixOutcome::Applied(format!(
            "set {} `{at}` to {source_value:?} from {}",
            target_rel.display(),
            self.source_file.display()
        )))
    }

    /// The editor / LSP form: a `SetContent` of the target with the value already
    /// propagated into its node. Value propagation HAS a worktree-edit form (a
    /// located rewrite), unlike `git_untrack` / `command`, so -- once a user
    /// Safe-promotes it, or opts into an unsafe quick-fix -- the LSP should offer
    /// it, consistent with the compose-derived SARIF / agent surfaces (which do NOT
    /// use `fix_edit`, so this changes only the LSP; audit 4c). Reuses
    /// `propagated_bytes` on `bytes` (the editor buffer). Confines the source read
    /// STRICTLY (no `allow_out_of_root` in `fix_edit`), like `SyncFromFixer`; the
    /// two engine suggestion sites call this with EMPTY `bytes`, where the target
    /// re-parse fails and it declines (`None`) -- the LSP passes the real buffer.
    fn fix_edit(&self, violation: &Violation, bytes: &[u8], root: &Path) -> Option<FixEdit> {
        let target_rel = violation.path.as_deref()?;
        let extract = self.target_extract(target_rel)?;
        let source_abs = confine_fix_path(&self.source_file, root, false).ok()?;
        let source_text = crate::io::read_capped(&source_abs)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())?;
        let values = extract_values(&self.source_extract, &source_text).ok()?;
        let mut literals = values.into_iter().filter(|v| !is_non_literal(v));
        let (Some(source_value), None) = (literals.next(), literals.next()) else {
            return None;
        };
        let content = self
            .propagated_bytes(violation, extract, &source_value, bytes, root)
            .ok()?;
        Some(FixEdit::SetContent {
            path: target_rel.to_path_buf(),
            content,
        })
    }
}

/// Registers a member in a manifest list for a `cross_file` `relation: registered`
/// rule (`fix: { create_and_register: {} }`): appends each missing member the
/// `check_registered` finding carried (in its `baseline_key`) to the target's
/// structured array. A *content-injecting* op (it writes a ruleset-chosen value
/// into the manifest), **`Unsafe` by default** (mutates a manifest), Safe-promotable.
///
/// PHASE 1b is register-only: an existing member that is unregistered is appended.
/// A missing NAMED member's file CREATE (the two-file transaction) is Phase 2, so
/// an existence-only finding (no members in the key) is Skipped here.
#[derive(Debug)]
pub struct CreateAndRegisterFixer {
    /// Per target: the ARRAY-locating extract (`Structured(format, "$.a.b")` -- the
    /// check's `[*]` element query with the trailing `[*]` stripped at build). Reuses
    /// [`ValueTargets`] (a target-path -> extract map); here the extract locates the
    /// array to append to, not a scalar to set.
    targets: ValueTargets,
    applicability: Applicability,
}

impl CreateAndRegisterFixer {
    pub(crate) fn new(targets: ValueTargets, applicability: Applicability) -> Self {
        Self {
            targets,
            applicability,
        }
    }

    fn target_extract(&self, target_rel: &Path) -> Option<&Extract> {
        match &self.targets {
            ValueTargets::Glob(ex) => Some(ex),
            ValueTargets::List(entries) => entries
                .iter()
                .find(|(p, _)| p == target_rel)
                .map(|(_, ex)| ex),
        }
    }

    /// Locate the array node the target `extract` points at and return its
    /// `(format, path segments)` for [`structured_fix::document_append`]. `None`
    /// when the target does not parse, the extract is not structured, or the array
    /// is absent (so the fixer declines rather than mis-target).
    fn array_location(
        extract: &Extract,
        target_bytes: &[u8],
    ) -> Option<(Format, Vec<alint_core::structured_fix::PathSeg>)> {
        let Extract::Structured(fmt, array_query) = extract else {
            return None;
        };
        let text = String::from_utf8_lossy(target_bytes);
        let value = fmt.parse(&text).ok()?;
        let path = JsonPath::parse(array_query).ok()?;
        let located = path.query_located(&value);
        let node = located.iter().next()?;
        Some((*fmt, crate::fixers::structured::to_segs(node.location())))
    }

    /// The single missing member the `check_registered` finding recorded in its
    /// `baseline_key` (`registered\0member\0<target>\0<member>`). Empty for an
    /// existence-only finding (`registered\0exists\0...`) or a malformed key -- the
    /// fixer then Skips (correlating EXACTLY to what the check flagged; no re-glob,
    /// so it never diverges from the check's gitignore-aware member set). One member
    /// per finding (per-member keying keeps each member's baseline fingerprint
    /// stable), so at most one element.
    fn missing_members(violation: &Violation) -> Vec<String> {
        let Some(key) = &violation.baseline_key else {
            return Vec::new();
        };
        let parts: Vec<&str> = key.split('\u{0}').collect();
        if parts.len() == 4 && parts[0] == "registered" && parts[1] == "member" {
            vec![parts[3].to_string()]
        } else {
            Vec::new()
        }
    }

    /// Compute the target's new bytes with the missing members appended, or an
    /// `Err(reason)` to Skip. Shared by `apply` (disk) and `fix_edit` (editor).
    fn registered_bytes(
        violation: &Violation,
        extract: &Extract,
        target_bytes: &[u8],
    ) -> std::result::Result<Vec<u8>, String> {
        let members = Self::missing_members(violation);
        if members.is_empty() {
            return Err("no members to register (an existence-only finding; \
                        creating a missing member is a follow-up)"
                .to_string());
        }
        let (fmt, segs) = Self::array_location(extract, target_bytes)
            .ok_or_else(|| "could not locate the target list array".to_string())?;
        if !alint_core::structured_fix::supports_list_append(fmt) {
            return Err(format!("list append is not yet supported for {fmt:?}"));
        }
        let values: Vec<serde_json::Value> = members
            .iter()
            .cloned()
            .map(serde_json::Value::String)
            .collect();
        let new_bytes =
            alint_core::structured_fix::document_append(fmt, target_bytes, &segs, &values)
                .ok_or_else(|| {
                    "nothing to append (already present, or not an array)".to_string()
                })?;
        // Post-splice re-verify (the located-fixer lesson + audit B#3): re-parse the
        // new bytes and confirm each appended member is now an element of the array.
        // A re-parse failure (e.g. a malformed edit) or a missing member means the
        // append did not produce what the CHECK would accept -- decline rather than
        // write bytes the check would still flag (or invalid syntax).
        Self::verify_registered(extract, &new_bytes, &members)?;
        Ok(new_bytes)
    }

    /// Confirm every `member` is now an element of the array the `extract` (with its
    /// trailing `[*]` restored) selects in `bytes`. `Err` if the bytes don't parse
    /// or a member is absent.
    fn verify_registered(
        extract: &Extract,
        bytes: &[u8],
        members: &[String],
    ) -> std::result::Result<(), String> {
        let Extract::Structured(fmt, array_query) = extract else {
            return Ok(());
        };
        let element_query = format!("{array_query}[*]");
        let elements = extract_values(
            &Extract::Structured(*fmt, element_query),
            &String::from_utf8_lossy(bytes),
        )
        .map_err(|e| format!("re-verify: the appended document did not parse: {e}"))?;
        for m in members {
            if !elements.iter().any(|e| e == m) {
                return Err(format!(
                    "re-verify: member {m:?} is not present after the append"
                ));
            }
        }
        Ok(())
    }
}

impl Fixer for CreateAndRegisterFixer {
    fn describe(&self) -> String {
        "register the member in the manifest list".to_string()
    }

    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn can_fix(&self, violation: &Violation) -> bool {
        // A registration finding (carries members) whose target format supports a
        // list append. An existence-only finding (create -- Phase 2) is not fixable
        // here, so `check` must not promise it.
        if Self::missing_members(violation).is_empty() {
            return false;
        }
        violation
            .path
            .as_deref()
            .and_then(|p| self.target_extract(p))
            .is_some_and(|ex| {
                matches!(ex, Extract::Structured(fmt, _)
                    if alint_core::structured_fix::supports_list_append(*fmt))
            })
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(target) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let target_rel: &Path = target;
        let Some(extract) = self.target_extract(target_rel) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} is not a configured registration target",
                target_rel.display()
            )));
        };
        let target_abs = match confine_fix_path(target_rel, ctx.root, ctx.allow_out_of_root) {
            Ok(p) => p,
            Err(reason) => return Ok(FixOutcome::Skipped(reason)),
        };
        let target_bytes = match read_for_fix(&target_abs, target_rel, ctx)? {
            ReadForFix::Bytes(b) => b,
            ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        let new_bytes = match Self::registered_bytes(violation, extract, &target_bytes) {
            Ok(b) => b,
            Err(reason) => return Ok(FixOutcome::Skipped(reason)),
        };
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would register member(s) in {}",
                target_rel.display()
            )));
        }
        ctx.commit_write(&target_abs, &new_bytes)
            .map_err(|source| Error::Io {
                path: target_abs.clone(),
                source,
            })?;
        Ok(FixOutcome::Applied(format!(
            "registered member(s) in {}",
            target_rel.display()
        )))
    }

    /// Editor / LSP form: a `SetContent` with the members appended. The two engine
    /// suggestion sites call this with EMPTY `bytes`, where the target re-parse
    /// fails and it declines (`None`) -- only the LSP passes the real buffer
    /// (consistent with `CrossFileValueFixer`).
    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        let target_rel = violation.path.as_deref()?;
        let extract = self.target_extract(target_rel)?;
        let content = Self::registered_bytes(violation, extract, bytes).ok()?;
        Some(FixEdit::SetContent {
            path: target_rel.to_path_buf(),
            content,
        })
    }
}

/// The locator string for a value-propagation target's extract, for messages: the
/// `JSONPath` query or the regex pattern.
fn locator(extract: &Extract) -> &str {
    match extract {
        Extract::Structured(_, query) => query,
        Extract::Regex(pattern) => pattern,
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn ctx(tmp: &TempDir, dry_run: bool) -> FixContext<'_> {
        FixContext {
            root: tmp.path(),
            dry_run,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: None,
        }
    }

    fn write(tmp: &TempDir, rel: &str, content: &[u8]) {
        let abs = tmp.path().join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(abs, content).unwrap();
    }

    fn read(tmp: &TempDir, rel: &str) -> Vec<u8> {
        std::fs::read(tmp.path().join(rel)).unwrap()
    }

    fn viol(target: &str) -> Violation {
        Violation::new("x").with_path(PathBuf::from(target))
    }

    #[test]
    fn overwrites_a_drifted_target_with_the_source() {
        let tmp = TempDir::new().unwrap();
        write(&tmp, "canon.txt", b"canonical\n");
        write(&tmp, "copy.txt", b"stale\n");
        let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
            .apply(&viol("copy.txt"), &ctx(&tmp, false))
            .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        assert_eq!(read(&tmp, "copy.txt"), b"canonical\n", "target now mirrors");
        assert_eq!(read(&tmp, "canon.txt"), b"canonical\n", "source untouched");
    }

    #[test]
    fn is_idempotent_when_already_identical() {
        let tmp = TempDir::new().unwrap();
        write(&tmp, "canon.txt", b"same\n");
        write(&tmp, "copy.txt", b"same\n");
        let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
            .apply(&viol("copy.txt"), &ctx(&tmp, false))
            .unwrap();
        match out {
            FixOutcome::Skipped(reason) => {
                assert!(reason.contains("already identical"), "{reason}");
            }
            FixOutcome::Applied(s) => panic!("expected an idempotent skip, got Applied({s})"),
        }
    }

    #[test]
    fn mirrors_binary_content() {
        // The `identical` relation compares bytes, so `sync_from` mirrors binary
        // content too (a vendored asset), unlike the text-only hygiene fixers.
        let tmp = TempDir::new().unwrap();
        let bin = [0u8, 159, 146, 150, b'\n'];
        write(&tmp, "asset.bin", &bin);
        write(&tmp, "vendored.bin", b"old\n");
        let out = SyncFromFixer::new(PathBuf::from("asset.bin"), Applicability::Unsafe)
            .apply(&viol("vendored.bin"), &ctx(&tmp, false))
            .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        assert_eq!(read(&tmp, "vendored.bin"), bin, "binary mirrored verbatim");
    }

    #[test]
    fn dry_run_reports_but_does_not_write() {
        let tmp = TempDir::new().unwrap();
        write(&tmp, "canon.txt", b"canonical\n");
        write(&tmp, "copy.txt", b"stale\n");
        let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
            .apply(&viol("copy.txt"), &ctx(&tmp, true))
            .unwrap();
        match out {
            FixOutcome::Applied(s) => {
                assert!(s.starts_with("would sync"), "dry-run summary: {s}");
                assert!(s.contains("copy.txt") && s.contains("canon.txt"), "{s}");
            }
            FixOutcome::Skipped(r) => panic!("expected a would-sync report, got Skipped({r})"),
        }
        assert_eq!(read(&tmp, "copy.txt"), b"stale\n", "dry-run must not write");
    }

    #[test]
    fn confines_an_absolute_source_read() {
        // SECURITY: an absolute `source` must NOT read out of the repo root
        // (`root.join("/abs")` discards the base) -- else an untrusted ruleset
        // could copy `/etc/passwd` into a tracked file. Confined -> Skip, no write.
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let secret = outside.path().join("secret");
        std::fs::write(&secret, b"TOP SECRET\n").unwrap();
        write(&tmp, "copy.txt", b"stale\n");
        let out = SyncFromFixer::new(secret.clone(), Applicability::Unsafe)
            .apply(&viol("copy.txt"), &ctx(&tmp, false))
            .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("escapes the repo root")),
            "an absolute source must be confined, got {out:?}"
        );
        assert_eq!(
            read(&tmp, "copy.txt"),
            b"stale\n",
            "must NOT copy out-of-root bytes in"
        );
    }

    #[test]
    fn confines_an_absolute_target_write() {
        // SECURITY: an absolute target (violation path) must NOT be written outside
        // the root.
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let victim = outside.path().join("victim");
        std::fs::write(&victim, b"original\n").unwrap();
        write(&tmp, "canon.txt", b"canonical\n");
        let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
            .apply(&viol(victim.to_str().unwrap()), &ctx(&tmp, false))
            .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("escapes the repo root")),
            "an absolute target must be confined, got {out:?}"
        );
        assert_eq!(
            std::fs::read(&victim).unwrap(),
            b"original\n",
            "out-of-root victim untouched"
        );
    }

    #[test]
    fn skips_a_missing_source_cleanly() {
        // A missing canonical source is a clean Skip (not a hard fix error): `check`
        // already reports it as a source-side violation.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "copy.txt", b"stale\n");
        let out = SyncFromFixer::new(PathBuf::from("gone.txt"), Applicability::Unsafe)
            .apply(&viol("copy.txt"), &ctx(&tmp, false))
            .unwrap();
        match out {
            FixOutcome::Skipped(reason) => {
                assert!(reason.contains("missing or unreadable"), "{reason}");
            }
            FixOutcome::Applied(s) => panic!("expected a missing-source skip, got Applied({s})"),
        }
    }

    #[test]
    fn skips_a_non_regular_target_without_hanging() {
        // SECURITY/DoS (audit HIGH): a `targets:` LIST entry is a config-verbatim
        // path that skips the walker's special-file filter. A bare read of a FIFO
        // target would block `fix` forever. A directory is the portable, hang-free
        // proxy for a non-regular file; the fixer must Skip (via read_for_fix's
        // guard), never reach a blocking read.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "canon.txt", b"canonical\n");
        std::fs::create_dir(tmp.path().join("a_dir")).unwrap();
        let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
            .apply(&viol("a_dir"), &ctx(&tmp, false))
            .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("not a regular file")),
            "a non-regular target must Skip cleanly, got {out:?}"
        );
    }

    #[test]
    fn skips_a_non_regular_source_without_hanging() {
        // Symmetric guard for the `source:` read (also config-verbatim).
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("src_dir")).unwrap();
        write(&tmp, "copy.txt", b"stale\n");
        let out = SyncFromFixer::new(PathBuf::from("src_dir"), Applicability::Unsafe)
            .apply(&viol("copy.txt"), &ctx(&tmp, false))
            .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("not a regular file")),
            "a non-regular source must Skip cleanly, got {out:?}"
        );
        assert_eq!(read(&tmp, "copy.txt"), b"stale\n", "target untouched");
    }

    #[test]
    fn skips_a_self_referential_target() {
        let tmp = TempDir::new().unwrap();
        write(&tmp, "canon.txt", b"x\n");
        let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
            .apply(&viol("canon.txt"), &ctx(&tmp, false))
            .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("same file")),
            "got {out:?}"
        );
    }

    #[test]
    fn stage_mode_composes_without_writing() {
        // `--diff`: a content fixer routes through the compose buffer (dry_run
        // false, compose Some, stage sink present but unused by a content op), and
        // the buffer is diffed without flushing -- so disk is untouched.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "canon.txt", b"canonical\n");
        write(&tmp, "copy.txt", b"stale\n");
        let compose = RefCell::new(BTreeMap::new());
        let sink = RefCell::new(Vec::new());
        let fix_ctx = FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: Some(&compose),
            stage_ops: Some(&sink),
        };
        let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
            .apply(&viol("copy.txt"), &fix_ctx)
            .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        assert_eq!(
            read(&tmp, "copy.txt"),
            b"stale\n",
            "stage must NOT flush to disk"
        );
        // The composed write is captured for the diff (keyed by resolved target).
        let buffered = compose.borrow();
        let composed = buffered
            .iter()
            .find(|(k, _)| k.ends_with("copy.txt"))
            .map(|(_, v)| v.clone());
        assert_eq!(
            composed,
            Some(b"canonical\n".to_vec()),
            "the sync is composed, got {:?}",
            *buffered
        );
        assert!(
            sink.borrow().is_empty(),
            "a content op records no whole-file stage edit"
        );
    }

    #[test]
    fn allow_out_of_root_permits_an_out_of_root_source() {
        // Parity with the same-file content fixers: `allow_out_of_root` opts into
        // reading a source outside the tree (a top-level policy escape hatch).
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let src = outside.path().join("canon");
        std::fs::write(&src, b"external canonical\n").unwrap();
        write(&tmp, "copy.txt", b"stale\n");
        let fix_ctx = FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: None,
            allow_out_of_root: true,
            compose: None,
            stage_ops: None,
        };
        let out = SyncFromFixer::new(src, Applicability::Unsafe)
            .apply(&viol("copy.txt"), &fix_ctx)
            .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        assert_eq!(read(&tmp, "copy.txt"), b"external canonical\n");
    }

    #[test]
    fn fix_edit_sets_the_target_to_the_source_bytes() {
        // The editor / SARIF / agent form: a `SetContent` of the target with the
        // source's bytes, produced even with EMPTY `_bytes` (the suggestion path).
        let tmp = TempDir::new().unwrap();
        write(&tmp, "canon.txt", b"canonical\n");
        write(&tmp, "copy.txt", b"stale\n");
        let edit = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
            .fix_edit(&viol("copy.txt"), &[], tmp.path())
            .expect("a proposed edit");
        match edit {
            FixEdit::SetContent { path, content } => {
                assert_eq!(path, PathBuf::from("copy.txt"));
                assert_eq!(content, b"canonical\n");
            }
            other => panic!("expected SetContent, got {other:?}"),
        }
    }

    #[test]
    fn fix_edit_declines_an_out_of_root_source() {
        // `fix_edit` confines strictly (no `allow_out_of_root`): an out-of-root
        // source yields NO proposed edit.
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let src = outside.path().join("canon");
        std::fs::write(&src, b"x\n").unwrap();
        write(&tmp, "copy.txt", b"stale\n");
        assert!(
            SyncFromFixer::new(src, Applicability::Unsafe)
                .fix_edit(&viol("copy.txt"), &[], tmp.path())
                .is_none(),
            "an out-of-root source must not produce an editor edit"
        );
    }

    #[test]
    fn carries_its_tier() {
        assert_eq!(
            SyncFromFixer::new(PathBuf::from("s"), Applicability::Unsafe).applicability(),
            Applicability::Unsafe
        );
        assert_eq!(
            SyncFromFixer::new(PathBuf::from("s"), Applicability::Safe).applicability(),
            Applicability::Safe
        );
    }

    // ─── CrossFileValueFixer (relation: equals value propagation) ────────

    // Source and target share `fmt` (the common test case); the source extract
    // uses the same format as the target so a JSON source parses as JSON, a TOML
    // source as TOML, etc.
    fn value_fixer(
        source: &str,
        source_q: &str,
        fmt: Format,
        target_q: &str,
    ) -> CrossFileValueFixer {
        CrossFileValueFixer::new(
            PathBuf::from(source),
            Extract::Structured(fmt, source_q.to_string()),
            ValueTargets::Glob(Extract::Structured(fmt, target_q.to_string())),
            Vec::new(),
            Applicability::Unsafe,
        )
    }

    #[test]
    fn value_propagates_a_scalar_into_the_target_node() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp,
            "Cargo.toml",
            b"[workspace.package]\nversion = \"2.0.0\"\n",
        );
        write(
            &tmp,
            "crate/Cargo.toml",
            b"[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
        );
        let out = value_fixer(
            "Cargo.toml",
            "$.workspace.package.version",
            Format::Toml,
            "$.package.version",
        )
        .apply(&viol("crate/Cargo.toml"), &ctx(&tmp, false))
        .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        let after = String::from_utf8(read(&tmp, "crate/Cargo.toml")).unwrap();
        assert!(after.contains("version = \"2.0.0\""), "{after}");
        assert!(
            after.contains("name = \"a\""),
            "other keys preserved: {after}"
        );
    }

    #[test]
    fn value_is_idempotent_when_the_target_already_equals() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp,
            "Cargo.toml",
            b"[workspace.package]\nversion = \"2.0.0\"\n",
        );
        write(
            &tmp,
            "crate/Cargo.toml",
            b"[package]\nversion = \"2.0.0\"\n",
        );
        let out = value_fixer(
            "Cargo.toml",
            "$.workspace.package.version",
            Format::Toml,
            "$.package.version",
        )
        .apply(&viol("crate/Cargo.toml"), &ctx(&tmp, false))
        .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("already equals")),
            "an already-equal target must Skip, got {out:?}"
        );
    }

    #[test]
    fn value_skips_when_the_source_is_not_exactly_one_value() {
        let tmp = TempDir::new().unwrap();
        // The source query matches nothing.
        write(&tmp, "Cargo.toml", b"[workspace.package]\nname = \"ws\"\n");
        write(
            &tmp,
            "crate/Cargo.toml",
            b"[package]\nversion = \"1.0.0\"\n",
        );
        let out = value_fixer(
            "Cargo.toml",
            "$.workspace.package.version",
            Format::Toml,
            "$.package.version",
        )
        .apply(&viol("crate/Cargo.toml"), &ctx(&tmp, false))
        .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("exactly one")),
            "a 0-match source must Skip, got {out:?}"
        );
    }

    #[test]
    fn value_dry_run_reports_without_writing() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp,
            "Cargo.toml",
            b"[workspace.package]\nversion = \"2.0.0\"\n",
        );
        write(
            &tmp,
            "crate/Cargo.toml",
            b"[package]\nversion = \"1.0.0\"\n",
        );
        let out = value_fixer(
            "Cargo.toml",
            "$.workspace.package.version",
            Format::Toml,
            "$.package.version",
        )
        .apply(&viol("crate/Cargo.toml"), &ctx(&tmp, true))
        .unwrap();
        assert!(
            matches!(out, FixOutcome::Applied(ref s) if s.starts_with("would set")),
            "got {out:?}"
        );
        assert!(
            String::from_utf8(read(&tmp, "crate/Cargo.toml"))
                .unwrap()
                .contains("1.0.0"),
            "dry-run must not write"
        );
    }

    #[test]
    fn value_confines_an_absolute_target() {
        // SECURITY: an absolute target must not be written outside the root.
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let victim = outside.path().join("victim.toml");
        std::fs::write(&victim, b"[package]\nversion = \"1.0.0\"\n").unwrap();
        write(
            &tmp,
            "Cargo.toml",
            b"[workspace.package]\nversion = \"2.0.0\"\n",
        );
        let out = CrossFileValueFixer::new(
            PathBuf::from("Cargo.toml"),
            Extract::Structured(Format::Toml, "$.workspace.package.version".to_string()),
            ValueTargets::List(vec![(
                victim.clone(),
                Extract::Structured(Format::Toml, "$.package.version".to_string()),
            )]),
            Vec::new(),
            Applicability::Unsafe,
        )
        .apply(&viol(victim.to_str().unwrap()), &ctx(&tmp, false))
        .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("escapes the repo root")),
            "an absolute target must be confined, got {out:?}"
        );
        assert!(
            std::fs::read_to_string(&victim).unwrap().contains("1.0.0"),
            "out-of-root victim untouched"
        );
    }

    #[test]
    fn value_carries_its_tier() {
        assert_eq!(
            value_fixer("s", "$.a", Format::Toml, "$.b").applicability(),
            Applicability::Unsafe
        );
    }

    #[test]
    fn value_declines_a_numeric_target_node_no_coercion() {
        // Audit F1: the propagated value is always a STRING, so setting a NUMERIC
        // target node would silently change its type to a quoted string (and the
        // `equals` check, comparing only string leaves, would keep firing). Decline
        // instead of coercing.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "src.json", br#"{"port":"8080"}"#);
        write(&tmp, "svc.json", br#"{"port": 9090, "host": "x"}"#);
        let out = value_fixer("src.json", "$.port", Format::Json, "$.port")
            .apply(&viol("svc.json"), &ctx(&tmp, false))
            .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("numeric node")),
            "a numeric target node must decline, got {out:?}"
        );
        assert_eq!(
            read(&tmp, "svc.json"),
            br#"{"port": 9090, "host": "x"}"#,
            "the numeric node must NOT be coerced to a string"
        );
    }

    #[test]
    fn value_declines_a_boolean_target_node() {
        let tmp = TempDir::new().unwrap();
        write(&tmp, "src.json", br#"{"flag":"true"}"#);
        write(&tmp, "t.json", br#"{"flag": false}"#);
        let out = value_fixer("src.json", "$.flag", Format::Json, "$.flag")
            .apply(&viol("t.json"), &ctx(&tmp, false))
            .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("boolean node")),
            "a boolean target node must decline, got {out:?}"
        );
        assert_eq!(read(&tmp, "t.json"), br#"{"flag": false}"#, "unchanged");
    }

    #[test]
    fn value_skips_a_non_scalar_target_node() {
        // An object/array target node is not a single scalar -> StructuredFixer::set
        // emits no edit -> clean Skip, byte-unchanged.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "src.json", br#"{"v":"3.0"}"#);
        write(&tmp, "obj.json", br#"{"v": {"nested": 1}}"#);
        let out = value_fixer("src.json", "$.v", Format::Json, "$.v")
            .apply(&viol("obj.json"), &ctx(&tmp, false))
            .unwrap();
        assert!(matches!(out, FixOutcome::Skipped(_)), "got {out:?}");
        assert_eq!(
            read(&tmp, "obj.json"),
            br#"{"v": {"nested": 1}}"#,
            "unchanged"
        );
    }

    #[test]
    fn value_fix_edit_produces_a_set_content_for_the_editor() {
        // Audit 4c: value propagation HAS a worktree-edit form, so the LSP can offer
        // it. `fix_edit` (real buffer bytes) returns a SetContent with the value
        // already propagated into the node.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "src.json", br#"{"v":"3.0"}"#);
        let buffer = br#"{"name":"x","v":"1.0"}"#;
        write(&tmp, "t.json", buffer);
        let edit = value_fixer("src.json", "$.v", Format::Json, "$.v")
            .fix_edit(&viol("t.json"), buffer, tmp.path())
            .expect("a proposed edit");
        match edit {
            FixEdit::SetContent { path, content } => {
                assert_eq!(path, PathBuf::from("t.json"));
                let s = String::from_utf8(content).unwrap();
                assert!(s.contains(r#""v":"3.0""#), "value propagated: {s}");
                assert!(s.contains(r#""name":"x""#), "other keys preserved: {s}");
            }
            other => panic!("expected SetContent, got {other:?}"),
        }
    }

    #[test]
    fn value_fix_edit_declines_empty_bytes() {
        // The two engine SUGGESTION sites call fix_edit with EMPTY bytes; the target
        // re-parse fails, so it declines (None). Only the LSP passes a real buffer.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "src.json", br#"{"v":"3.0"}"#);
        assert!(
            value_fixer("src.json", "$.v", Format::Json, "$.v")
                .fix_edit(&viol("t.json"), &[], tmp.path())
                .is_none(),
            "empty bytes -> no proposed edit"
        );
    }

    // ─── regex-extract value propagation (Phase 2) ───────────────────────

    // A value fixer with a regex source pattern + a (possibly different) regex
    // target pattern.
    fn value_fixer_regex(source_pat: &str, target_pat: &str) -> CrossFileValueFixer {
        CrossFileValueFixer::new(
            PathBuf::from("VERSION"),
            Extract::Regex(source_pat.to_string()),
            ValueTargets::Glob(Extract::Regex(target_pat.to_string())),
            Vec::new(),
            Applicability::Unsafe,
        )
    }

    #[test]
    fn value_propagates_a_regex_capture_preserving_the_pattern() {
        let tmp = TempDir::new().unwrap();
        write(&tmp, "VERSION", b"2.5.0\n");
        write(
            &tmp,
            "README.md",
            b"# Proj\n![v](https://x/badge/version-1.0.0-blue)\n",
        );
        let out = value_fixer_regex("^([0-9.]+)", "version-([0-9.]+)-")
            .apply(&viol("README.md"), &ctx(&tmp, false))
            .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        let after = String::from_utf8(read(&tmp, "README.md")).unwrap();
        assert!(
            after.contains("version-2.5.0-blue"),
            "capture rewritten: {after}"
        );
        assert!(
            after.contains("# Proj") && after.contains("![v]"),
            "rest preserved: {after}"
        );
    }

    #[test]
    fn value_regex_declines_when_the_value_breaks_the_pattern() {
        // Re-extract verify: a source value that cannot satisfy the target's capture
        // (here a digit-bearing value into a `[a-z]+` capture) must be declined --
        // never write a value the `equals` check would still reject.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "src.txt", b"val=DIGITS123\n");
        write(&tmp, "f.txt", b"v=old\n");
        let fixer = CrossFileValueFixer::new(
            PathBuf::from("src.txt"),
            Extract::Regex("val=(.+)".to_string()),
            ValueTargets::Glob(Extract::Regex("v=([a-z]+)".to_string())),
            Vec::new(),
            Applicability::Unsafe,
        );
        let out = fixer.apply(&viol("f.txt"), &ctx(&tmp, false)).unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("would not satisfy the regex")),
            "a value that breaks the pattern must decline, got {out:?}"
        );
        assert_eq!(read(&tmp, "f.txt"), b"v=old\n", "target unchanged");
    }

    #[test]
    fn value_regex_is_idempotent_when_already_equal() {
        let tmp = TempDir::new().unwrap();
        write(&tmp, "src.txt", b"tag: 9.9\n");
        write(&tmp, "t.txt", b"pinned to 9.9 here\n");
        let fixer = CrossFileValueFixer::new(
            PathBuf::from("src.txt"),
            Extract::Regex("tag: ([0-9.]+)".to_string()),
            ValueTargets::Glob(Extract::Regex("pinned to ([0-9.]+) ".to_string())),
            Vec::new(),
            Applicability::Unsafe,
        );
        let out = fixer.apply(&viol("t.txt"), &ctx(&tmp, false)).unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("nothing to propagate")),
            "an already-equal capture must Skip, got {out:?}"
        );
    }

    #[test]
    fn value_regex_declines_a_missing_capture_group() {
        // A target regex with no group 1 has nothing to set.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "src.txt", b"tag: 9.9\n");
        write(&tmp, "t.txt", b"nogroup 1.0\n");
        let fixer = CrossFileValueFixer::new(
            PathBuf::from("src.txt"),
            Extract::Regex("tag: ([0-9.]+)".to_string()),
            ValueTargets::Glob(Extract::Regex("nogroup".to_string())),
            Vec::new(),
            Applicability::Unsafe,
        );
        let out = fixer.apply(&viol("t.txt"), &ctx(&tmp, false)).unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("no capture group 1")),
            "got {out:?}"
        );
    }

    #[test]
    fn value_regex_declines_a_no_match_distinctly() {
        // Audit F-1: a regex that HAS a group 1 but matches NOTHING must say
        // "matched nothing", not the misleading "no capture group 1".
        let tmp = TempDir::new().unwrap();
        write(&tmp, "VERSION", b"tag: 9.9\n");
        write(&tmp, "t.txt", b"unrelated content\n");
        let out = value_fixer_regex("tag: ([0-9.]+)", "v=([0-9.]+)")
            .apply(&viol("t.txt"), &ctx(&tmp, false))
            .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("matched nothing")),
            "got {out:?}"
        );
    }

    #[test]
    fn value_regex_skips_a_non_literal_capture() {
        // Audit HIGH: an interpolated `${VERSION}` capture is a template the check
        // skips (as a note); the fixer must NOT clobber it. Here the ONLY capture is
        // non-literal -> nothing to propagate, file untouched.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "VERSION", b"2.5.0\n");
        write(&tmp, "t.txt", b"image: myapp:${VERSION}\n");
        let out = value_fixer_regex("([0-9.]+)", "myapp:(\\S+)")
            .apply(&viol("t.txt"), &ctx(&tmp, false))
            .unwrap();
        assert!(matches!(out, FixOutcome::Skipped(_)), "got {out:?}");
        assert_eq!(
            read(&tmp, "t.txt"),
            b"image: myapp:${VERSION}\n",
            "the interpolated template must NOT be clobbered"
        );
    }

    #[test]
    fn value_regex_clobbers_only_the_literal_drift_beside_a_template() {
        // A literal drift AND a `${VERSION}` template under one pattern: rewrite the
        // literal, LEAVE the template (audit HIGH).
        let tmp = TempDir::new().unwrap();
        write(&tmp, "VERSION", b"2.5.0\n");
        write(
            &tmp,
            "t.md",
            b"badge/version-1.0.0-blue and badge/version-${VERSION}-green\n",
        );
        let out = value_fixer_regex("([0-9.]+)", "version-(\\S+?)-")
            .apply(&viol("t.md"), &ctx(&tmp, false))
            .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        let after = String::from_utf8(read(&tmp, "t.md")).unwrap();
        assert!(
            after.contains("version-2.5.0-blue"),
            "literal drift fixed: {after}"
        );
        assert!(
            after.contains("version-${VERSION}-green"),
            "the template must survive: {after}"
        );
    }

    #[test]
    fn value_regex_respects_normalize() {
        // Audit HIGH/MED: under `normalize: semver-minor`, a capture in the same
        // band as the source is NOT drift -- do not clobber it. Source 2.5.0 (band
        // 2.5): "2.4.0" drifts (band 2.4), "2.5.7" does not (band 2.5).
        let tmp = TempDir::new().unwrap();
        write(&tmp, "src.txt", b"2.5.0\n");
        write(&tmp, "t.txt", b"min v([0-9.]+) ... v2.4.0 and v2.5.7\n");
        let fixer = CrossFileValueFixer::new(
            PathBuf::from("src.txt"),
            Extract::Regex("([0-9.]+)".to_string()),
            ValueTargets::Glob(Extract::Regex("v([0-9.]+)".to_string())),
            vec![Normalize::SemverMinor],
            Applicability::Unsafe,
        );
        let out = fixer.apply(&viol("t.txt"), &ctx(&tmp, false)).unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        let after = String::from_utf8(read(&tmp, "t.txt")).unwrap();
        assert!(
            after.contains("v2.5.0 and"),
            "the drifting 2.4.0 -> 2.5.0: {after}"
        );
        assert!(
            after.contains("v2.5.7"),
            "the in-band 2.5.7 must NOT be clobbered: {after}"
        );
    }

    #[test]
    fn value_regex_declines_a_non_utf8_target() {
        // Audit F-2: a target with invalid UTF-8 declines cleanly (byte-offset
        // safety), never mis-splices.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "VERSION", b"9\n");
        write(&tmp, "t.txt", b"v=1 \xff\xfe raw\n");
        let out = value_fixer_regex("([0-9]+)", "v=([0-9]+)")
            .apply(&viol("t.txt"), &ctx(&tmp, false))
            .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("not valid UTF-8")),
            "got {out:?}"
        );
        assert_eq!(read(&tmp, "t.txt"), b"v=1 \xff\xfe raw\n", "untouched");
    }

    #[test]
    fn value_structured_skips_a_non_literal_node() {
        // Audit HIGH: a JSON string node `${VERSION}` is a template the check skips;
        // the structured path must NOT clobber it either.
        let tmp = TempDir::new().unwrap();
        write(&tmp, "src.json", br#"{"v":"2.5.0"}"#);
        write(&tmp, "t.json", br#"{"v":"${VERSION}"}"#);
        let out = value_fixer("src.json", "$.v", Format::Json, "$.v")
            .apply(&viol("t.json"), &ctx(&tmp, false))
            .unwrap();
        assert!(
            matches!(out, FixOutcome::Skipped(ref r) if r.contains("non-literal template")),
            "got {out:?}"
        );
        assert_eq!(
            read(&tmp, "t.json"),
            br#"{"v":"${VERSION}"}"#,
            "template survives"
        );
    }
}
