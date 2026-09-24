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

/// Which structured node to set in a value-propagation target: the format and the
/// `JSONPath` source. A glob shares one across every match; a list carries one per
/// entry. (Phase 1 = structured-extract targets only; a regex-extract target is a
/// deferred follow-up and is rejected at load.)
#[derive(Debug)]
pub enum ValueTargets {
    Glob(Format, String),
    List(Vec<(PathBuf, Format, String)>),
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
    applicability: Applicability,
}

impl CrossFileValueFixer {
    #[must_use]
    pub fn new(
        source_file: PathBuf,
        source_extract: Extract,
        targets: ValueTargets,
        applicability: Applicability,
    ) -> Self {
        Self {
            source_file,
            source_extract,
            targets,
            applicability,
        }
    }

    /// The (format, `JSONPath` source) for `target_rel`: the shared glob node, or
    /// the matching list entry. `None` when the violation path is not a configured
    /// target (should not happen -- the violation came from this rule).
    fn target_node(&self, target_rel: &Path) -> Option<(Format, &str)> {
        match &self.targets {
            ValueTargets::Glob(fmt, q) => Some((*fmt, q.as_str())),
            ValueTargets::List(entries) => entries
                .iter()
                .find(|(p, _, _)| p == target_rel)
                .map(|(_, fmt, q)| (*fmt, q.as_str())),
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
    fn propagated_bytes(
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
            let target_text = String::from_utf8_lossy(target_bytes);
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
        let Some((format, query)) = self.target_node(target_rel) else {
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
            format,
            query,
            &source_value,
            &target_bytes,
            ctx.root,
        ) {
            Ok(b) => b,
            Err(reason) => return Ok(FixOutcome::Skipped(reason)),
        };
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would set {} `{query}` to {source_value:?} from {}",
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
            "set {} `{query}` to {source_value:?} from {}",
            target_rel.display(),
            self.source_file.display()
        )))
    }

    // No `fix_edit`: value propagation reuses the located resolver at apply time
    // (it needs the source read + the delegate build), which the check-side
    // proposed-edit path does not carry. It is Unsafe anyway, so the Safe-only
    // machine surfaces advertise nothing. Inherits the trait default (`None`).
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

    fn value_fixer(
        source: &str,
        source_q: &str,
        target_fmt: Format,
        target_q: &str,
    ) -> CrossFileValueFixer {
        CrossFileValueFixer::new(
            PathBuf::from(source),
            Extract::Structured(Format::Toml, source_q.to_string()),
            ValueTargets::Glob(target_fmt, target_q.to_string()),
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
                Format::Toml,
                "$.package.version".to_string(),
            )]),
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
}
