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

use alint_core::{
    Applicability, Error, FixContext, FixEdit, FixOutcome, Fixer, ReadForFix, Result, Violation,
    read_for_fix,
};

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
}
