//! `git_untrack` — the first *spawning* fix op. Removes the violating path from
//! git's index (`git rm --cached`) while leaving it on disk, for the "a committed
//! artifact should be untracked" hygiene case (host `file_absent` with
//! `git_tracked_only: true`, so the rule converges once the path leaves the
//! tracked set).
//!
//! Trust: because it shells out to `git`, a `git_untrack` fix is refused at config
//! load from any non-top-level source (`extends:` / nested / bundled) by
//! `alint_dsl::SPAWNING_FIX_OPS` + `reject_spawning_fix_ops_in`, exactly like a
//! spawning rule kind (`command`). It is **`Unsafe` by default** (it restructures
//! the git index on a bare `alint fix`), so a bare fix only *suggests* it;
//! `--unsafe-fixes` applies it.

use std::path::Path;

use alint_core::git::{UntrackOutcome, untrack_path};
use alint_core::{Applicability, Error, FixContext, FixOutcome, Fixer, Result, Violation};

/// Untracks the violating file from git's index (`git rm --cached`), leaving it
/// on disk. Paired with `file_absent` (`git_tracked_only: true`). A *spawning*
/// fixer, `Unsafe` by default.
#[derive(Debug)]
pub struct GitUntrackFixer {
    applicability: Applicability,
}

impl GitUntrackFixer {
    #[must_use]
    pub fn new(applicability: Applicability) -> Self {
        Self { applicability }
    }
}

impl Fixer for GitUntrackFixer {
    fn describe(&self) -> String {
        "untrack the file from git (git rm --cached, keeps it on disk)".to_string()
    }

    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        // `&PathBuf` -> `&Path` once (no method-resolution ambiguity).
        let rel: &Path = path;
        // Dry-run / stage (`--diff`): a git-untrack changes the INDEX, not the
        // working tree, so there is NO worktree hunk to stage -- nothing is pushed
        // to `stage_ops`. The preview routes through git's OWN `--dry-run` (via
        // `untrack_path(.., true)`), so it can NEVER diverge from the real run: a
        // path git would refuse (staged content differs from HEAD + worktree) is
        // predicted as a skip, never a false "would untrack" (round-4 dry-run
        // faithfulness). `--dry-run` mutates nothing.
        let dry = ctx.dry_run || ctx.stage_ops.is_some();
        match untrack_path(ctx.root, rel, dry) {
            UntrackOutcome::Untracked if dry => Ok(FixOutcome::Applied(format!(
                "would untrack {} from git (git rm --cached)",
                rel.display()
            ))),
            UntrackOutcome::Untracked => Ok(FixOutcome::Applied(format!(
                "untracked {} from git (git rm --cached)",
                rel.display()
            ))),
            UntrackOutcome::NotAGitRepo => Ok(FixOutcome::Skipped(format!(
                "{} is not in a git repository",
                rel.display()
            ))),
            UntrackOutcome::NotTracked => Ok(FixOutcome::Skipped(format!(
                "{} is already untracked",
                rel.display()
            ))),
            // A real failure is a fix error (exit 1); the same failure PREDICTED by
            // the dry-run is a benign "would fail" skip (a preview must not error).
            UntrackOutcome::Failed(stderr) if dry => Ok(FixOutcome::Skipped(format!(
                "git rm --cached {} would fail: {stderr}",
                rel.display()
            ))),
            UntrackOutcome::Failed(stderr) => Err(Error::Other(format!(
                "git rm --cached {} failed: {stderr}",
                rel.display()
            ))),
        }
    }

    // No `fix_edit`: a git-index change has no editor/worktree-edit form (an LSP
    // cannot run `git rm --cached` as a text edit), so this fixer contributes no
    // proposed edit to the SARIF / agent / LSP surfaces. It is `Unsafe` anyway,
    // and those surfaces advertise only Safe fixes, so nothing is lost. Inherits
    // the trait default (`fix_edit` -> `None`).
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    // The tracked-set reader used only by the test helper `is_tracked` (the fixer
    // itself now reaches it via `untrack_path`), so it is imported here, not at the
    // module level, to avoid an unused import in non-test builds.
    use alint_core::git::collect_tracked_paths;
    use tempfile::TempDir;

    /// Is `git` on PATH? The untrack tests genuinely shell out, so skip cleanly
    /// (rather than fail) on a git-less box; CI always has `git`.
    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn git(root: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git runs")
            .status
            .success();
        assert!(ok, "git {args:?} failed");
    }

    /// A tempdir that IS a git repo with `name` added (staged) to the index.
    fn repo_with_tracked(name: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(name), b"artifact\n").unwrap();
        git(tmp.path(), &["init", "-q", "-b", "main"]);
        git(tmp.path(), &["add", "--", name]);
        tmp
    }

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

    fn is_tracked(tmp: &TempDir, name: &str) -> bool {
        collect_tracked_paths(tmp.path()).is_some_and(|t| t.contains(Path::new(name)))
    }

    /// A git repo with `name` committed (HEAD holds `content`).
    fn repo_with_committed(name: &str, content: &[u8]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(name), content).unwrap();
        git(tmp.path(), &["init", "-q", "-b", "main"]);
        git(tmp.path(), &["add", "--", name]);
        git(
            tmp.path(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "init",
            ],
        );
        tmp
    }

    #[test]
    fn untracks_a_path_with_glob_metachars_without_collateral() {
        // SECURITY (audit CRITICAL): a violation path containing git pathspec
        // metacharacters (`[`, `]`, `*`) must be untracked LITERALLY. `--` ends
        // OPTION parsing only, not globbing -- without GIT_LITERAL_PATHSPECS a
        // `[id].tsx` route reads as a char class (matching `i.tsx`/`d.tsx`) and a
        // file named `*` empties the whole index, all silently. git_untrack must
        // remove ONLY the named file.
        if !git_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        for n in ["[id].tsx", "i.tsx", "d.tsx"] {
            std::fs::write(tmp.path().join(n), b"x\n").unwrap();
        }
        git(tmp.path(), &["init", "-q", "-b", "main"]);
        git(tmp.path(), &["add", "-A"]);
        let v = Violation::new("x").with_path(Path::new("[id].tsx"));
        let out = GitUntrackFixer::new(Applicability::Unsafe)
            .apply(&v, &ctx(&tmp, false))
            .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        assert!(!is_tracked(&tmp, "[id].tsx"), "the named file is untracked");
        assert!(
            is_tracked(&tmp, "i.tsx"),
            "sibling i.tsx must NOT be collaterally untracked"
        );
        assert!(
            is_tracked(&tmp, "d.tsx"),
            "sibling d.tsx must NOT be collaterally untracked"
        );
    }

    #[test]
    fn dry_run_faithfully_predicts_a_staged_differs_failure() {
        // Round-4 faithfulness: `git rm --cached` refuses (without -f) when a path's
        // staged content differs from BOTH HEAD and the worktree. The dry-run/stage
        // preview must predict that (it routes through git's own `--dry-run`), never
        // a false "would untrack"; the real run then errors.
        if !git_available() {
            return;
        }
        let tmp = repo_with_committed("f.log", b"v1\n"); // HEAD = v1
        std::fs::write(tmp.path().join("f.log"), b"v2\n").unwrap();
        git(tmp.path(), &["add", "--", "f.log"]); // index = v2 (differs from HEAD)
        std::fs::write(tmp.path().join("f.log"), b"v3\n").unwrap(); // worktree = v3
        let v = Violation::new("x").with_path(Path::new("f.log"));
        // Dry-run must NOT claim "would untrack" -- it must predict the failure.
        let dry = GitUntrackFixer::new(Applicability::Unsafe)
            .apply(&v, &ctx(&tmp, true))
            .unwrap();
        match dry {
            FixOutcome::Skipped(reason) => assert!(reason.contains("would fail"), "{reason}"),
            FixOutcome::Applied(s) => {
                panic!("dry-run falsely predicted success: {s}")
            }
        }
        // The real run surfaces the same failure as a fix error (not a force-remove).
        let real = GitUntrackFixer::new(Applicability::Unsafe).apply(&v, &ctx(&tmp, false));
        assert!(
            real.is_err(),
            "real run must error on staged-differs, got {real:?}"
        );
        assert!(
            is_tracked(&tmp, "f.log"),
            "the file is left tracked, never force-removed"
        );
    }

    #[test]
    fn untracks_a_tracked_file_and_keeps_it_on_disk() {
        if !git_available() {
            return;
        }
        let tmp = repo_with_tracked("build.o");
        assert!(is_tracked(&tmp, "build.o"), "precondition: file is tracked");
        let v = Violation::new("x").with_path(Path::new("build.o"));
        let out = GitUntrackFixer::new(Applicability::Unsafe)
            .apply(&v, &ctx(&tmp, false))
            .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        assert!(!is_tracked(&tmp, "build.o"), "must leave the git index");
        assert!(
            tmp.path().join("build.o").exists(),
            "untrack keeps the file on disk"
        );
    }

    #[test]
    fn is_idempotent_on_an_already_untracked_file() {
        if !git_available() {
            return;
        }
        let tmp = repo_with_tracked("build.o");
        let v = Violation::new("x").with_path(Path::new("build.o"));
        let fixer = GitUntrackFixer::new(Applicability::Unsafe);
        assert!(matches!(
            fixer.apply(&v, &ctx(&tmp, false)).unwrap(),
            FixOutcome::Applied(_)
        ));
        // Second pass: already gone from the index -> a clean skip, never an error.
        let again = fixer.apply(&v, &ctx(&tmp, false)).unwrap();
        match again {
            FixOutcome::Skipped(reason) => {
                assert!(reason.contains("already untracked"), "{reason}");
            }
            FixOutcome::Applied(s) => {
                panic!("expected an already-untracked skip, got Applied({s:?})")
            }
        }
    }

    #[test]
    fn skips_outside_a_git_repository() {
        if !git_available() {
            return;
        }
        // A plain tempdir with a file but no `git init`.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("build.o"), b"x\n").unwrap();
        let v = Violation::new("x").with_path(Path::new("build.o"));
        let out = GitUntrackFixer::new(Applicability::Unsafe)
            .apply(&v, &ctx(&tmp, false))
            .unwrap();
        match out {
            FixOutcome::Skipped(reason) => {
                assert!(reason.contains("not in a git repository"), "{reason}");
            }
            FixOutcome::Applied(s) => panic!("expected a not-a-repo skip, got Applied({s:?})"),
        }
        assert!(tmp.path().join("build.o").exists());
    }

    #[test]
    fn dry_run_reports_but_does_not_untrack() {
        if !git_available() {
            return;
        }
        let tmp = repo_with_tracked("build.o");
        let v = Violation::new("x").with_path(Path::new("build.o"));
        let out = GitUntrackFixer::new(Applicability::Unsafe)
            .apply(&v, &ctx(&tmp, true))
            .unwrap();
        match out {
            FixOutcome::Applied(s) => {
                assert!(s.starts_with("would untrack"), "dry-run summary: {s}");
                assert!(s.contains("build.o"), "must name the file: {s}");
            }
            FixOutcome::Skipped(r) => panic!("expected a would-untrack report, got Skipped({r:?})"),
        }
        assert!(
            is_tracked(&tmp, "build.o"),
            "dry-run must NOT change the index"
        );
    }

    #[test]
    fn stage_mode_reports_and_records_nothing() {
        if !git_available() {
            return;
        }
        let tmp = repo_with_tracked("build.o");
        let sink = std::cell::RefCell::new(Vec::new());
        let fix_ctx = FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: Some(&sink),
        };
        let v = Violation::new("x").with_path(Path::new("build.o"));
        let out = GitUntrackFixer::new(Applicability::Unsafe)
            .apply(&v, &fix_ctx)
            .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)));
        assert!(
            is_tracked(&tmp, "build.o"),
            "stage must NOT change the index"
        );
        assert!(
            sink.into_inner().is_empty(),
            "a git-index op has no worktree hunk to stage"
        );
    }

    #[test]
    fn carries_its_tier_and_offers_no_fix_edit() {
        assert_eq!(
            GitUntrackFixer::new(Applicability::Unsafe).applicability(),
            Applicability::Unsafe
        );
        // No editor/worktree representation for a git-index op.
        let v = Violation::new("x").with_path(Path::new("build.o"));
        assert!(
            GitUntrackFixer::new(Applicability::Unsafe)
                .fix_edit(&v, &[], Path::new("/repo"))
                .is_none()
        );
    }
}
