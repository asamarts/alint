//! `command` fix op -- run a user-supplied fix command per violation on a
//! `command` rule (the check runs `command:`; the fix runs `run:` -- e.g. check
//! `eslint {path}`, fix `eslint --fix {path}`).
//!
//! Trust: it shells out, so -- like the `command` rule kind itself and
//! `git_untrack` -- a `command` fix is refused at config load from any
//! non-top-level source by `alint_dsl::SPAWNING_FIX_OPS` +
//! `reject_spawning_fix_ops_in`. It is **`Unsafe` by default** (running an
//! arbitrary command on a bare `alint fix` is opt-in); a user may promote a
//! specific rule to `Safe` in their OWN top-level config.
//!
//! Convergence: EXEMPT from the property-net convergence law -- alint cannot
//! guarantee an arbitrary command's idempotence, so the command fix has its own
//! fire/silent tests instead (auto-fix.md 5.6). At RUNTIME the fixer reports
//! `Applied` only when the command actually CHANGES `{path}`, and `Skipped` when
//! it leaves the file byte-identical -- that Skip is the fixpoint driver's
//! idempotence signal, so a command that reaches a fixpoint on the file (or never
//! touches it) is run at most a pass or two, not to the 10-pass cap (audit
//! F-CMD1). A command that makes PROGRESS on `{path}` but does not fully resolve
//! its own check reports the applied change and exits 0; `alint check` then shows
//! the residual (the change is real, so `Applied` is honest).

use std::path::Path;
use std::time::Duration;

use alint_core::template::{PathTokens, render_path_argv};
use alint_core::{Applicability, Error, FixContext, FixOutcome, Fixer, Result, Violation};

use crate::spawn::{SpawnOutcome, run_capturing};

/// Cap on the child output surfaced in a fix-error message. `run_capturing` keeps
/// up to 64 MiB; a failing tool's full diagnostics dumped into the error was a
/// multi-minute hang + huge report (audit A1-MED). Mirrors the `command` rule's
/// own 16 KiB `OUTPUT_CAP_BYTES`.
const SURFACED_OUTPUT_CAP: usize = 16 * 1024;

/// Lossy-decode + trim `bytes`, truncating (on a char boundary) to
/// [`SURFACED_OUTPUT_CAP`] with a marker so a huge child output can't blow up the
/// fix-error message.
fn capped_output(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim();
    if s.len() <= SURFACED_OUTPUT_CAP {
        return s.to_string();
    }
    let end = (0..=SURFACED_OUTPUT_CAP)
        .rev()
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(0);
    format!(
        "{}... [output truncated at {SURFACED_OUTPUT_CAP} bytes]",
        &s[..end]
    )
}

/// Runs a user-supplied fix command (`run:` argv, `{path}`-templated) for each
/// violation on a `command` rule. A *spawning* fixer, `Unsafe` by default.
#[derive(Debug)]
pub struct CommandFixFixer {
    argv: Vec<String>,
    timeout: Duration,
    applicability: Applicability,
}

impl CommandFixFixer {
    #[must_use]
    pub fn new(argv: Vec<String>, timeout: Duration, applicability: Applicability) -> Self {
        Self {
            argv,
            timeout,
            applicability,
        }
    }
}

impl Fixer for CommandFixFixer {
    fn describe(&self) -> String {
        format!("run `{}`", self.argv.join(" "))
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
        let rel: &Path = path;
        // `render_path_argv` (not `render_path`): the same option-injection guard
        // the `command` rule uses, so a repo filename rendered from `{path}` can't
        // masquerade as a flag.
        let tokens = PathTokens::from_path(rel);
        let rendered: Vec<String> = self
            .argv
            .iter()
            .map(|s| render_path_argv(s, &tokens))
            .collect();
        // Dry-run / stage (`--diff`): a fix command runs arbitrary side effects and
        // has no previewable worktree edit, so report what WOULD run and spawn
        // NOTHING (nothing is pushed to `stage_ops`).
        if ctx.dry_run || ctx.stage_ops.is_some() {
            return Ok(FixOutcome::Applied(format!(
                "would run `{}` on {}",
                rendered.join(" "),
                rel.display()
            )));
        }
        let env = [
            ("ALINT_PATH", rel.display().to_string()),
            ("ALINT_ROOT", ctx.root.display().to_string()),
        ];
        // Read `{path}` BEFORE running so we can tell whether the command actually
        // changed it (audit F-CMD1). alint's fixpoint re-runs a rule until a pass
        // applies NOTHING ("a fix's OWN idempotence is what makes a pass stop
        // applying"). A command fix has no built-in idempotence, so without this it
        // would re-run an arbitrary (slow, side-effectful) command every pass up to
        // the 10-pass cap and then brand it "non-convergent". Treating "the command
        // ran but left `{path}` byte-identical" as a Skip is that idempotence
        // signal: an `eslint --fix` that reaches a fixpoint on the file (or a
        // command that does not touch `{path}` at all) converges in <=2 passes.
        let abs = ctx.root.join(rel);
        let before = std::fs::read(&abs).ok();
        match run_capturing(&rendered, ctx.root, &env, self.timeout) {
            SpawnOutcome::Exited { status, .. } if status.success() => {
                if std::fs::read(&abs).ok() == before {
                    Ok(FixOutcome::Skipped(format!(
                        "ran `{}`; no change to {} (a fixpoint / no-op)",
                        rendered.join(" "),
                        rel.display()
                    )))
                } else {
                    Ok(FixOutcome::Applied(format!(
                        "ran `{}` on {}",
                        rendered.join(" "),
                        rel.display()
                    )))
                }
            }
            // A non-zero exit is the command signaling failure -- surface it as a
            // fix error (exit 1), like the `command` rule treats a non-zero check.
            // The captured output is CAPPED before surfacing: a failing tool can
            // emit tens of MB and `run_capturing` keeps up to 64 MiB, so dumping it
            // verbatim was a multi-minute hang + huge report (audit A1-MED).
            SpawnOutcome::Exited {
                status,
                stdout,
                stderr,
            } => {
                let code = status
                    .code()
                    .map_or_else(|| "a signal".to_string(), |c| c.to_string());
                let out = if stderr.is_empty() { &stdout } else { &stderr };
                Err(Error::Other(format!(
                    "fix command `{}` exited {code} on {}: {}",
                    rendered.join(" "),
                    rel.display(),
                    capped_output(out)
                )))
            }
            SpawnOutcome::SpawnError(e) => Err(Error::Other(format!(
                "could not spawn fix command `{}`: {e}",
                rendered.join(" ")
            ))),
            SpawnOutcome::TimedOut { secs } => Err(Error::Other(format!(
                "fix command `{}` timed out after {secs}s on {}",
                rendered.join(" "),
                rel.display()
            ))),
        }
    }

    // No `fix_edit`: an arbitrary command has no editor-expressible worktree edit,
    // so it contributes no proposed edit to the SARIF / agent / LSP surfaces
    // (Unsafe, so those Safe-only surfaces would not advertise it anyway). Inherits
    // the trait default (`fix_edit` -> `None`).
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::FixContext;
    use std::path::Path;
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

    fn fixer(argv: &[&str]) -> CommandFixFixer {
        CommandFixFixer::new(
            argv.iter().map(|s| (*s).to_string()).collect(),
            Duration::from_secs(30),
            Applicability::Unsafe,
        )
    }

    #[test]
    fn runs_the_fix_command_with_path_substituted() {
        // A fix command that rewrites the file (`sh -c 'echo fixed > "$1"' _ {path}`)
        // runs, exits 0, and its side effect lands -- the fire test.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "before\n").unwrap();
        let v = Violation::new("x").with_path(Path::new("a.txt"));
        let out = fixer(&["sh", "-c", "printf fixed > \"$1\"", "_", "{path}"])
            .apply(&v, &ctx(&tmp, false))
            .unwrap();
        assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "fixed"
        );
    }

    #[test]
    fn a_nonzero_exit_is_a_fix_error() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        let v = Violation::new("x").with_path(Path::new("a.txt"));
        let res = fixer(&["sh", "-c", "echo boom >&2; exit 3"]).apply(&v, &ctx(&tmp, false));
        let err = res.unwrap_err().to_string();
        assert!(err.contains("exited 3"), "{err}");
        assert!(err.contains("boom"), "surfaces stderr: {err}");
    }

    #[test]
    fn a_command_that_does_not_change_the_path_is_a_skip_not_applied() {
        // F-CMD1: a fix command that runs cleanly but leaves `{path}` byte-identical
        // is a Skip (a no-op / fixpoint), NOT Applied -- this is what stops alint's
        // fixpoint driver from re-running an arbitrary command every pass to the
        // 10-pass cap. `true` exits 0 and never touches the file.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "unchanged\n").unwrap();
        let v = Violation::new("x").with_path(Path::new("a.txt"));
        let out = fixer(&["true"]).apply(&v, &ctx(&tmp, false)).unwrap();
        match out {
            FixOutcome::Skipped(r) => assert!(r.contains("no change"), "{r}"),
            FixOutcome::Applied(s) => panic!("a no-op command must be a Skip, got Applied({s})"),
        }
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "unchanged\n",
            "the file is untouched"
        );
    }

    #[test]
    fn a_failing_command_caps_its_surfaced_output() {
        // A1-MED: a failing tool can emit tens of MB; `run_capturing` keeps up to
        // 64 MiB, so the fix-error must be truncated before it is surfaced.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        let v = Violation::new("x").with_path(Path::new("a.txt"));
        let err = fixer(&["sh", "-c", "yes B | head -c 1000000 >&2; exit 4"])
            .apply(&v, &ctx(&tmp, false))
            .unwrap_err()
            .to_string();
        assert!(err.contains("truncated"), "output must be marked truncated");
        assert!(
            err.len() < 64 * 1024,
            "the surfaced error must be bounded, got {} bytes",
            err.len()
        );
    }

    #[test]
    fn a_hanging_command_is_killed_at_the_timeout() {
        // M2 (audit gap): the `TimedOut` arm must surface a fix error, not hang.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        let v = Violation::new("x").with_path(Path::new("a.txt"));
        let short = CommandFixFixer::new(
            vec!["sleep".to_string(), "30".to_string()],
            Duration::from_secs(1),
            Applicability::Unsafe,
        );
        let err = short.apply(&v, &ctx(&tmp, false)).unwrap_err().to_string();
        assert!(err.contains("timed out after 1s"), "{err}");
    }

    #[test]
    fn dry_run_reports_without_running() {
        // The command must NOT run under dry-run: a marker the command would create
        // stays absent, and the report is a "would run".
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        let v = Violation::new("x").with_path(Path::new("a.txt"));
        let out = fixer(&["sh", "-c", "touch ran.marker"])
            .apply(&v, &ctx(&tmp, true))
            .unwrap();
        match out {
            FixOutcome::Applied(s) => assert!(s.starts_with("would run"), "{s}"),
            FixOutcome::Skipped(s) => panic!("expected a would-run report, got Skipped({s})"),
        }
        assert!(
            !tmp.path().join("ran.marker").exists(),
            "dry-run must not spawn the command"
        );
    }

    #[test]
    fn spawn_failure_is_a_fix_error() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        let v = Violation::new("x").with_path(Path::new("a.txt"));
        let res =
            fixer(&["definitely-not-a-real-program-xyz", "{path}"]).apply(&v, &ctx(&tmp, false));
        assert!(
            res.unwrap_err().to_string().contains("could not spawn"),
            "an unspawnable program is a fix error"
        );
    }

    #[test]
    fn carries_its_tier_and_offers_no_fix_edit() {
        assert_eq!(fixer(&["true"]).applicability(), Applicability::Unsafe);
        let v = Violation::new("x").with_path(Path::new("a.txt"));
        assert!(
            fixer(&["true"])
                .fix_edit(&v, &[], Path::new("/repo"))
                .is_none(),
            "a command fix has no editor-expressible edit"
        );
    }
}
