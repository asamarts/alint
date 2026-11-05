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
    Result, Violation, extract_values_bounded, is_non_literal, read_for_fix,
};

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
/// Optional target-value predicates from the host `cross_file` rule. Kept
/// alongside [`ValueTargets`] so the fixer can apply the same selection as the
/// checker without changing the registration fixer's target model.
#[derive(Debug)]
pub(crate) enum ValueTargetMatches {
    Glob(Option<regex::Regex>),
    List(Vec<(PathBuf, Option<regex::Regex>)>),
}
/// Hard ceilings remain active when users disable `fix_size_limit`: a bounded
/// checker must not expand one small target into unbounded edit metadata/output.
/// The messages name the limit without its value, which `fix_size_limit` can
/// take from the environment.
const MAX_PROPAGATED_FIX_BYTES: usize = 32 * 1024 * 1024;

fn propagated_output_limit(configured: Option<u64>) -> usize {
    configured
        .map_or(MAX_PROPAGATED_FIX_BYTES, |limit| {
            usize::try_from(limit).unwrap_or(usize::MAX)
        })
        .min(MAX_PROPAGATED_FIX_BYTES)
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
    target_matches: Option<ValueTargetMatches>,
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
            target_matches: None,
            normalize,
            applicability,
        }
    }

    /// Attach the host rule's resolved target predicates. Builders call this
    /// for `sync_from`; direct unit-test constructors default to no predicate.
    #[must_use]
    pub(crate) fn with_target_matches(mut self, matches: ValueTargetMatches) -> Self {
        self.target_matches = Some(matches);
        self
    }

    /// The `extract` and optional value predicate for `target_rel`: the shared
    /// glob settings, or the matching list entry's. `None` when the violation
    /// path is not a configured target (should not happen -- the violation came
    /// from this rule).
    fn target_config(&self, target_rel: &Path) -> Option<(&Extract, Option<&regex::Regex>)> {
        let extract = match &self.targets {
            ValueTargets::Glob(extract) => extract,
            ValueTargets::List(entries) => entries
                .iter()
                .find(|(path, _)| path == target_rel)
                .map(|(_, extract)| extract)?,
        };
        let matches = match self.target_matches.as_ref() {
            Some(ValueTargetMatches::Glob(matches)) => matches.as_ref(),
            Some(ValueTargetMatches::List(entries)) => entries
                .iter()
                .find(|(path, _)| path == target_rel)
                .and_then(|(_, matches)| matches.as_ref()),
            None => None,
        };
        Some((extract, matches))
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
        let values = extract_values_bounded(
            &self.source_extract,
            &text,
            crate::cross_file::MAX_EXTRACTED_VALUES,
        )
        .map_err(|e| format!("source extract failed: {e}"))?;
        if !values.non_strings.is_empty() {
            return Err("source extract matched a non-string value".to_string());
        }
        let mut literals = values.strings.into_iter().filter(|v| !is_non_literal(v));
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
    // The dispatcher carries one immutable view of each input needed by both
    // extraction regimes; wrapping them in a temporary context would add a
    // second name for every field without reducing the call contract.
    #[allow(clippy::too_many_arguments)]
    fn propagated_bytes(
        &self,
        violation: &Violation,
        extract: &Extract,
        matches: Option<&regex::Regex>,
        source_value: &str,
        target_bytes: &[u8],
        root: &Path,
        output_limit: usize,
    ) -> std::result::Result<Vec<u8>, String> {
        if target_bytes.len() > output_limit || source_value.len() > output_limit {
            return Err(format!(
                "value propagation would exceed the output safety limit (`fix_size_limit`, \
                 at most {} MiB)",
                MAX_PROPAGATED_FIX_BYTES >> 20
            ));
        }
        if let Some(matches) = matches
            && !matches.is_match(source_value)
        {
            return Err(format!(
                "canonical source value {source_value:?} does not satisfy the target `matches` filter"
            ));
        }
        let new_bytes = match extract {
            Extract::Structured(extract) => self.propagate_structured(
                violation,
                extract,
                matches,
                source_value,
                target_bytes,
                root,
            )?,
            Extract::Regex(pattern) => self.propagate_regex(
                violation,
                pattern,
                matches,
                source_value,
                target_bytes,
                output_limit,
            )?,
            // build_value_targets rejects every other extract, so this is unreachable
            // for a built rule.
            _ => return Err("unsupported target extract for value propagation".to_string()),
        };
        if new_bytes.len() > output_limit {
            return Err(format!(
                "value propagation would produce {} bytes, over the output safety limit \
                 (`fix_size_limit`, at most {} MiB)",
                new_bytes.len(),
                MAX_PROPAGATED_FIX_BYTES >> 20
            ));
        }
        Ok(new_bytes)
    }

    /// Return the exact raw target value that identified this live `equals`
    /// finding. Baseline suppression operates on the same key, so limiting a
    /// regex repair to this value prevents one live finding from rewriting
    /// separately-baselined captures in the same file.
    fn violation_target_value(violation: &Violation) -> Option<&str> {
        let mut parts = violation.baseline_key.as_deref()?.splitn(3, '\0');
        if parts.next()? != "equals" {
            return None;
        }
        parts.next()?;
        parts.next()
    }

    fn violation_target_occurrence(violation: &Violation) -> Option<usize> {
        let mut parts = violation.fix_key.as_deref()?.splitn(2, '\0');
        if parts.next()? != "cross_file_equals" {
            return None;
        }
        parts.next()?.parse().ok()
    }

    /// Regex-extract target: rewrite only captures represented by the triggering
    /// live finding, then re-extract to verify those captures still resolve to the
    /// source. Distinct captures remain for their own findings/fix passes.
    // Selection, splice construction, and re-extraction verification share the
    // same capture accounting; keeping them together makes the safety invariant
    // reviewable in one place.
    #[allow(clippy::too_many_lines)]
    fn propagate_regex(
        &self,
        violation: &Violation,
        pattern: &str,
        matches: Option<&regex::Regex>,
        source_value: &str,
        target_bytes: &[u8],
        output_limit: usize,
    ) -> std::result::Result<Vec<u8>, String> {
        let target_rel = violation.path.as_deref().unwrap_or_else(|| Path::new(""));
        let target_value = Self::violation_target_value(violation).ok_or_else(|| {
            format!(
                "{}: regex propagation requires the live finding's exact target-value identity",
                target_rel.display()
            )
        })?;
        let occurrence = Self::violation_target_occurrence(violation).ok_or_else(|| {
            format!(
                "{}: regex propagation requires the live finding's occurrence identity",
                target_rel.display()
            )
        })?;
        // A capture rewrite needs byte offsets into the real bytes; a lossy decode
        // would shift them, so require valid UTF-8.
        let text = std::str::from_utf8(target_bytes).map_err(|_| {
            format!(
                "{} is not valid UTF-8; cannot rewrite a regex capture",
                target_rel.display()
            )
        })?;
        let captures = crate::cross_file::target_regex_capture_ranges(target_rel, text, pattern)?;
        if !captures.any_match {
            return Err(format!(
                "the target regex `{pattern}` matched nothing in {}",
                target_rel.display()
            ));
        }
        if !captures.had_group {
            return Err(format!(
                "the target regex `{pattern}` has no capture group 1 to set on {}",
                target_rel.display()
            ));
        }
        let source_norm = apply_normalize(&self.normalize, source_value);
        let mut source_equal_before = 0usize;
        let mut selected_drift = 0usize;
        for capture in &captures.ranges {
            let current = &text[capture.clone()];
            if matches.is_some_and(|matches| !matches.is_match(current)) || is_non_literal(current)
            {
                continue;
            }
            if apply_normalize(&self.normalize, current) == source_norm {
                source_equal_before += 1;
            } else if current == target_value {
                selected_drift += 1;
            }
        }
        if selected_drift == 0 {
            return Err(format!(
                "{}: nothing to propagate for target value {target_value:?} at regex `{pattern}`",
                target_rel.display()
            ));
        }
        // Fix passes are applied sequentially. Earlier live siblings may already
        // have removed lower ordinals from this value's candidate set; clamping
        // retains the baseline-suppressed prefix while selecting one remaining
        // live occurrence.
        let selected = occurrence.min(selected_drift - 1);
        let capture = captures
            .ranges
            .iter()
            .filter(|capture| {
                let current = &text[(*capture).clone()];
                matches.is_none_or(|matches| matches.is_match(current))
                    && !is_non_literal(current)
                    && current == target_value
            })
            .nth(selected)
            .cloned()
            .ok_or_else(|| "regex propagation occurrence disappeared".to_string())?;
        let projected_bytes = target_bytes
            .len()
            .checked_sub(capture.len())
            .and_then(|size| size.checked_add(source_value.len()))
            .ok_or_else(|| "regex propagation output size overflowed".to_string())?;
        if source_value.len() > output_limit || projected_bytes > output_limit {
            return Err(format!(
                "regex propagation would exceed the output safety limit (`fix_size_limit`, \
                 at most {} MiB)",
                MAX_PROPAGATED_FIX_BYTES >> 20
            ));
        }
        let ranges = vec![capture];

        let expected_source_values = source_equal_before
            .checked_add(ranges.len())
            .ok_or_else(|| "regex propagation match count overflowed".to_string())?;
        let mut new_bytes = Vec::with_capacity(projected_bytes);
        let mut cursor = 0usize;
        for range in &ranges {
            new_bytes.extend_from_slice(&target_bytes[cursor..range.start]);
            new_bytes.extend_from_slice(source_value.as_bytes());
            cursor = range.end;
        }
        new_bytes.extend_from_slice(&target_bytes[cursor..]);

        // Re-extract verify: every rewritten capture must remain selected and
        // normalize-equal after the splice. Other live or baselined values are
        // deliberately left for their own finding identity.
        let updated_text = String::from_utf8_lossy(&new_bytes);
        let re_new =
            crate::cross_file::target_regex_capture_ranges(target_rel, &updated_text, pattern)
                .map_err(|error| format!("regex re-extract failed: {error}"))?;
        let source_values_after = re_new
            .ranges
            .iter()
            .map(|range| &updated_text[range.clone()])
            .filter(|value| {
                !is_non_literal(value)
                    && matches.is_none_or(|matches| matches.is_match(value))
                    && apply_normalize(&self.normalize, value) == source_norm
            })
            .count();
        if source_values_after < expected_source_values {
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
    // Query selection, delegated edit application, and post-edit verification
    // deliberately form one transaction over the parsed document.
    #[allow(clippy::too_many_lines)]
    fn propagate_structured(
        &self,
        violation: &Violation,
        extract: &alint_core::StructuredExtract,
        matches: Option<&regex::Regex>,
        source_value: &str,
        target_bytes: &[u8],
        root: &Path,
    ) -> std::result::Result<Vec<u8>, String> {
        let target_rel = violation.path.as_deref().unwrap_or_else(|| Path::new(""));
        let target_value = Self::violation_target_value(violation).ok_or_else(|| {
            format!(
                "{}: structured propagation requires the live finding's exact target-value identity",
                target_rel.display()
            )
        })?;
        let occurrence = Self::violation_target_occurrence(violation).ok_or_else(|| {
            format!(
                "{}: structured propagation requires the live finding's occurrence identity",
                target_rel.display()
            )
        })?;
        let format = extract.format;
        let query = extract.query.as_str();
        let path_expr = &extract.path;
        let target_text = String::from_utf8_lossy(target_bytes);
        let parsed = format.parse(&target_text).map_err(|error| {
            format!(
                "could not parse {} as {format:?} for `{query}`: {error}",
                target_rel.display()
            )
        })?;
        let source_norm = apply_normalize(&self.normalize, source_value);
        let queried = path_expr.query(&parsed);
        if queried.len() == 1
            && let Some(node) = queried.first()
            && (node.is_number() || node.is_boolean())
        {
            return Err(format!(
                "{} `{query}` is a {} node; `sync_from` on `equals` would change it \
                 to a string (the propagated value is text). Pin a numeric/bool value \
                 with a same-file `set_value` (typed `equals:`) instead.",
                target_rel.display(),
                if node.is_number() {
                    "numeric"
                } else {
                    "boolean"
                }
            ));
        }
        let mut source_equal_before = 0usize;
        let mut selected_drift = 0usize;
        for node in queried.iter() {
            let Some(current) = node.as_str() else {
                continue;
            };
            if matches.is_some_and(|matches| !matches.is_match(current)) || is_non_literal(current)
            {
                continue;
            }
            if apply_normalize(&self.normalize, current) == source_norm {
                source_equal_before += 1;
            } else if current == target_value {
                selected_drift += 1;
            }
        }
        if selected_drift == 0 {
            return Err(if is_non_literal(target_value) {
                format!(
                    "{} `{query}` is a non-literal template ({target_value:?}); `sync_from` \
                     leaves interpolated values alone",
                    target_rel.display()
                )
            } else if source_equal_before > 0 {
                format!(
                    "{} already equals {source_value:?} at `{query}`",
                    target_rel.display()
                )
            } else {
                format!(
                    "{}: nothing to propagate for target value {target_value:?} at `{query}`",
                    target_rel.display()
                )
            });
        }

        let selected = occurrence.min(selected_drift - 1);
        let delegate = StructuredFixer::set_selected_string(
            format,
            path_expr.clone(),
            query.to_string(),
            source_value.to_string(),
            target_value.to_string(),
            selected,
            self.applicability,
        );
        let collected = delegate.collect_edits(
            std::slice::from_ref(violation),
            target_rel,
            target_bytes,
            root,
        );
        if collected.is_empty() {
            return Err(format!(
                "could not set {} at `{query}` to {source_value:?} \
                 (the selected string node was not writable in {format:?})",
                target_rel.display()
            ));
        }
        let batch: Vec<LocatedEdit> = collected
            .into_iter()
            .enumerate()
            .map(|(i, collected)| LocatedEdit {
                rule_index: 0,
                violation_index: i,
                sensitive: false,
                collected,
            })
            .collect();
        let (new_bytes, outcomes) = apply_file_edits(target_bytes, batch, self.applicability);
        if !outcomes
            .iter()
            .any(|(_, outcome)| matches!(outcome, LocatedOutcome::Applied))
        {
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

        let expected_source_values = source_equal_before
            .checked_add(1)
            .ok_or_else(|| "structured propagation match count overflowed".to_string())?;
        let updated_text = String::from_utf8_lossy(&new_bytes);
        let updated = format
            .parse(&updated_text)
            .map_err(|error| format!("post-edit {format:?} parse failed: {error}"))?;
        let source_values_after = path_expr
            .query(&updated)
            .iter()
            .filter_map(|value| value.as_str())
            .filter(|value| {
                !is_non_literal(value)
                    && matches.is_none_or(|matches| matches.is_match(value))
                    && apply_normalize(&self.normalize, value) == source_norm
            })
            .count();
        if source_values_after < expected_source_values {
            return Err(format!(
                "setting {} to {source_value:?} would not satisfy `{query}`",
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
        let Some((extract, matches)) = self.target_config(target_rel) else {
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
        let new_bytes = match self.propagated_bytes(
            violation,
            extract,
            matches,
            &source_value,
            &target_bytes,
            ctx.root,
            propagated_output_limit(ctx.fix_size_limit),
        ) {
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
        let (extract, matches) = self.target_config(target_rel)?;
        let source_abs = confine_fix_path(&self.source_file, root, false).ok()?;
        let source_text = crate::io::read_capped(&source_abs)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())?;
        let values = extract_values_bounded(
            &self.source_extract,
            &source_text,
            crate::cross_file::MAX_EXTRACTED_VALUES,
        )
        .ok()?;
        if !values.non_strings.is_empty() {
            return None;
        }
        let mut literals = values.strings.into_iter().filter(|v| !is_non_literal(v));
        let (Some(source_value), None) = (literals.next(), literals.next()) else {
            return None;
        };
        let content = self
            .propagated_bytes(
                violation,
                extract,
                matches,
                &source_value,
                bytes,
                root,
                MAX_PROPAGATED_FIX_BYTES,
            )
            .ok()?;
        Some(FixEdit::SetContent {
            path: target_rel.to_path_buf(),
            content,
        })
    }
}

/// Repairs a `cross_file` `relation: registered` rule's two postconditions
/// (`fix: { create_and_register: {} }`), dispatching on the finding kind:
/// - a REGISTRATION finding (`registered\0member\0...`) APPENDS the missing member
///   the `check_registered` finding carried (in its `baseline_key`) to the target's
///   structured array;
/// - an EXISTENCE finding (`registered\0exists\0...`, a missing NAMED member) CREATES
///   the member's file from the fix's `content`/`content_from` (`create_member`).
///
/// The two are independent idempotent fixes the fixpoint applies in order (create,
/// then -- once the re-walk sees the member exists -- register); the check gates a
/// registration finding on existence, so a member that cannot be created is never
/// registered as a phantom. A *content-injecting* op (it writes a ruleset-chosen
/// value into the manifest AND ruleset-authored bytes to a ruleset-named path),
/// **`Unsafe` by default**, `Safe`-promotable.
#[derive(Debug)]
pub struct CreateAndRegisterFixer {
    /// Per target: the ARRAY-locating extract (`Structured(format, "$.a.b")` -- the
    /// check's `[*]` element query with the trailing `[*]` stripped at build). Reuses
    /// [`ValueTargets`] (a target-path -> extract map); here the extract locates the
    /// array to append to, not a scalar to set.
    targets: ValueTargets,
    /// The bytes for a MISSING named member (the create half). `None` = register-
    /// only (an existence finding is then reported, not fixed). Only a `file:` (named)
    /// source can produce an existence finding, so this is inert for a glob source
    /// (rejected at build).
    content: Option<alint_core::ContentSourceSpec>,
    applicability: Applicability,
}

impl CreateAndRegisterFixer {
    pub(crate) fn new(
        targets: ValueTargets,
        content: Option<alint_core::ContentSourceSpec>,
        applicability: Applicability,
    ) -> Self {
        Self {
            targets,
            content,
            applicability,
        }
    }

    /// The member path an EXISTENCE finding (`registered\0exists\0<path>`) names --
    /// the file to create. `None` for a registration finding or a malformed key.
    fn existence_member(violation: &Violation) -> Option<PathBuf> {
        let key = violation.baseline_key.as_deref()?;
        let parts: Vec<&str> = key.split('\u{0}').collect();
        (parts.len() == 3 && parts[0] == "registered" && parts[1] == "exists")
            .then(|| PathBuf::from(parts[2]))
    }

    /// Create a MISSING named member's file from `content` (the create half of the
    /// two-postcondition model). Mirrors `FileCreateFixer`'s confinement + no-clobber
    /// + symlink-refusal guards; creates parent dirs (a new crate needs its dir).
    fn create_member(&self, member: &Path, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(source) = &self.content else {
            return Ok(FixOutcome::Skipped(format!(
                "{} does not exist and the rule has no `content`/`content_from` to \
                 create it",
                member.display()
            )));
        };
        let abs = match confine_fix_path(member, ctx.root, ctx.allow_out_of_root) {
            Ok(p) => p,
            Err(reason) => return Ok(FixOutcome::Skipped(reason)),
        };
        // Never clobber an existing file/dir, and never create THROUGH a (possibly
        // broken) symlink at the target -- exactly `FileCreateFixer`'s guards.
        if abs.exists() {
            return Ok(FixOutcome::Skipped(format!(
                "{} already exists",
                member.display()
            )));
        }
        if abs
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Ok(FixOutcome::Skipped(format!(
                "{} is a symlink; refusing to create through it",
                member.display()
            )));
        }
        let content = match crate::fixers::creators::resolve_source_bytes(
            source,
            ctx.root,
            ctx.allow_out_of_root,
        ) {
            Ok(bytes) => bytes,
            Err(skip) => return Ok(FixOutcome::Skipped(skip)),
        };
        if ctx.dry_run || ctx.stage_ops.is_some() {
            if let Some(sink) = ctx.stage_ops {
                sink.borrow_mut().push(FixEdit::CreateFile {
                    path: member.to_path_buf(),
                    content: content.clone(),
                });
            }
            return Ok(FixOutcome::Applied(format!(
                "would create {}",
                member.display()
            )));
        }
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&abs, &content).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!("created {}", member.display())))
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
        let Extract::Structured(extract) = extract else {
            return None;
        };
        let text = String::from_utf8_lossy(target_bytes);
        let value = extract.format.parse(&text).ok()?;
        let located = extract.path.query_located(&value);
        let node = located.iter().next()?;
        Some((
            extract.format,
            crate::fixers::structured::to_segs(node.location()),
        ))
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
            // Reached only for a malformed registration finding (an existence
            // finding is routed to `create_member` by `apply` before this).
            return Err("no member to register in this finding".to_string());
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
        let Extract::Structured(extract) = extract else {
            return Ok(());
        };
        let text = String::from_utf8_lossy(bytes);
        let value = extract
            .format
            .parse(&text)
            .map_err(|e| format!("re-verify: the appended document did not parse: {e}"))?;
        let arrays = extract.path.query(&value);
        let Some(array) = arrays.iter().next().and_then(|value| value.as_array()) else {
            return Err("re-verify: the target list is not an array".to_string());
        };
        for member in members {
            if !array
                .iter()
                .any(|value| value.as_str() == Some(member.as_str()))
            {
                return Err(format!(
                    "re-verify: member {member:?} is not present after the append"
                ));
            }
        }
        Ok(())
    }
}

impl Fixer for CreateAndRegisterFixer {
    fn describe(&self) -> String {
        "create the member's file and/or register it in the manifest list".to_string()
    }

    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn can_fix(&self, violation: &Violation) -> bool {
        // An EXISTENCE finding (missing named member) is fixable iff the rule carries
        // content to create it.
        if Self::existence_member(violation).is_some() {
            return self.content.is_some();
        }
        // A registration finding (carries a member) whose target format supports a
        // list append.
        if Self::missing_members(violation).is_empty() {
            return false;
        }
        violation
            .path
            .as_deref()
            .and_then(|p| self.target_extract(p))
            .is_some_and(|ex| {
                matches!(ex, Extract::Structured(extract)
                    if alint_core::structured_fix::supports_list_append(extract.format))
            })
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        // An EXISTENCE finding (a missing NAMED member) -> create the file from
        // `content`/`content_from` (the create half of the two-postcondition model;
        // the registration finding, handled below, appends its path independently).
        if let Some(member) = Self::existence_member(violation) {
            return self.create_member(&member, ctx);
        }
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

    /// Editor / LSP form: a `CreateFile` for an existence finding, or a `SetContent`
    /// with the member appended for a registration finding. The two engine suggestion
    /// sites call this with EMPTY `bytes`, where the registration re-parse fails and
    /// it declines (`None`) -- only the LSP passes the real buffer (consistent with
    /// `CrossFileValueFixer`).
    fn fix_edit(&self, violation: &Violation, bytes: &[u8], root: &Path) -> Option<FixEdit> {
        // An existence finding -> a CreateFile (mirrors `create_member`'s guards).
        if let Some(member) = Self::existence_member(violation) {
            let source = self.content.as_ref()?;
            let abs = confine_fix_path(&member, root, false).ok()?;
            if abs.exists()
                || abs
                    .symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_symlink())
            {
                return None;
            }
            let content =
                crate::fixers::creators::resolve_source_bytes(source, root, false).ok()?;
            return Some(FixEdit::CreateFile {
                path: member,
                content,
            });
        }
        let target_rel = violation.path.as_deref()?;
        let extract = self.target_extract(target_rel)?;
        let content = Self::registered_bytes(violation, extract, bytes).ok()?;
        Some(FixEdit::SetContent {
            path: target_rel.to_path_buf(),
            content,
        })
    }

    /// The missing member an existence finding creates, under the rule's
    /// `allow_out_of_root` as `create_member` reads it (`fix_edit` declines a
    /// member outside the root).
    fn created_file(
        &self,
        violation: &Violation,
        root: &Path,
        allow_out_of_root: bool,
    ) -> Option<(PathBuf, Vec<u8>)> {
        let member = Self::existence_member(violation)?;
        let content = crate::fixers::creators::resolve_source_bytes(
            self.content.as_ref()?,
            root,
            allow_out_of_root,
        )
        .ok()?;
        Some((member, content))
    }
}

/// The locator string for a value-propagation target's extract, for messages: the
/// `JSONPath` query or the regex pattern.
fn locator(extract: &Extract) -> &str {
    match extract {
        Extract::Structured(extract) => &extract.query,
        Extract::Regex(pattern) => pattern,
        _ => "",
    }
}

#[cfg(test)]
#[path = "cross_file_ops_tests.rs"]
mod tests;
