//! `command_idempotent` — a declared check-mode command must be
//! a no-op. The check-mode-idempotence sibling of
//! `generated_file_fresh`: run a user-declared formatter/checker
//! in its `--check` mode once (single-shot); exit `0` =
//! formatter-clean, non-zero = violation(s). Optional
//! `files_from` / `files_pattern` attributes per-file violations
//! from the tool's own offender list. alint never runs a
//! mutating formatter and never writes the working tree itself.
//!
//! Same trust tier as the `command` / `generated_file_fresh`
//! rules: it spawns a user-supplied process, so it is trust-gated
//! at config load by `alint_dsl::reject_command_rules_in` — only
//! the user's own top-level config may declare it; an `extends:`'d
//! ruleset (local / HTTPS / `alint://bundled/`) declaring it is
//! refused. Design + open-question resolutions:
//! `docs/design/v0.10/command_idempotent.md`.
//!
//! ```yaml
//! - id: code-is-formatted
//!   kind: command_idempotent
//!   command: ["cargo", "fmt", "--all", "--", "--check"]
//!   workdir: "."                       # default: lint root
//!   files_from: stderr                 # none (default) | stdout | stderr
//!   files_pattern: "Diff in (.+) at"   # optional regex, group 1 = path
//!   level: error
//! ```

use std::path::PathBuf;
use std::time::Duration;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use regex::Regex;
use serde::Deserialize;

/// Cap on the tool output captured into a fallback violation
/// message — a noisy formatter can emit a lot; keep reports
/// legible (mirrors the `command` rule's output cap intent).
const OUTPUT_SNIPPET_CAP: usize = 400;

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum FilesFrom {
    /// One violation for the whole invocation (default).
    #[default]
    None,
    /// Parse the checker's stdout for the offender list.
    Stdout,
    /// Parse the checker's stderr for the offender list.
    Stderr,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Checker argv (no shell), run in its --check / idempotence mode; exit
    /// 0 = clean.
    #[schemars(length(min = 1))]
    command: Vec<String>,
    /// Checker cwd, relative to the lint root (default: lint root).
    #[serde(default)]
    workdir: Option<String>,
    /// On failure, parse this stream into per-file violations (default: none =
    /// one violation for the whole invocation).
    #[serde(default)]
    files_from: FilesFrom,
    /// Regex whose capture group 1 is a file path, applied per output line
    /// (requires `files_from`; omit for bare-path listers).
    #[serde(default)]
    files_pattern: Option<String>,
    /// Checker timeout in seconds (default 120). On timeout the child is killed
    /// and one violation is emitted.
    #[serde(default)]
    #[schemars(range(min = 1))]
    timeout: Option<u64>,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct CommandIdempotentRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    command: Vec<String>,
    workdir: String,
    files_from: FilesFrom,
    files_pattern: Option<Regex>,
    timeout: u64,
}

impl Rule for CommandIdempotentRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Single-shot: the verdict is one check-mode exit code,
        // independent of which files changed (never `--changed`-
        // filtered). `path_scope` stays `None` (default) so the
        // engine doesn't skip-by-intersection. Same dispatch
        // class as `pair` / `generated_file_fresh`.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let env = [
            ("ALINT_ROOT", ctx.root.to_string_lossy().into_owned()),
            ("ALINT_RULE_ID", self.id.clone()),
            ("ALINT_LEVEL", self.level.as_str().to_string()),
        ];
        let (status, stdout_b, stderr_b) = match crate::spawn::run_capturing(
            &self.command,
            &ctx.root.join(&self.workdir),
            &env,
            Duration::from_secs(self.timeout),
        ) {
            crate::spawn::SpawnOutcome::Exited {
                status,
                stdout,
                stderr,
            } => (status, stdout, stderr),
            crate::spawn::SpawnOutcome::SpawnError(e) => {
                let program = self.command.first().map_or("", String::as_str);
                return Ok(vec![self.violation(
                    &self.workdir,
                    &format!("checker `{program}` could not be spawned: {e}"),
                )]);
            }
            crate::spawn::SpawnOutcome::TimedOut { secs } => {
                return Ok(vec![self.violation(
                    &self.workdir,
                    &format!(
                        "`{}` did not exit within {secs}s \
                         (raise `timeout:` on the rule to extend)",
                        self.command.join(" ")
                    ),
                )]);
            }
        };

        if status.success() {
            // The tree is idempotent / formatter-clean.
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&stdout_b);
        let stderr = String::from_utf8_lossy(&stderr_b);
        let code = status
            .code()
            .map_or_else(|| "a signal".to_string(), |c| c.to_string());

        // Non-zero exit: the tree is not formatter-clean. Either
        // one whole-invocation violation, or — with `files_from` —
        // one per offending file the tool itself listed.
        let stream = match self.files_from {
            FilesFrom::None => {
                return Ok(vec![self.violation(
                    &self.workdir,
                    &format!(
                        "`{}` exited with {code} — the tree is not formatter-clean{}",
                        self.command.join(" "),
                        snippet(&stdout, &stderr),
                    ),
                )]);
            }
            FilesFrom::Stdout => &stdout,
            FilesFrom::Stderr => &stderr,
        };

        let violations = self.parse_offenders(stream);
        if violations.is_empty() {
            // Non-zero exit but nothing parseable — never swallow
            // a failure into a pass; fall back to one violation.
            return Ok(vec![self.violation(
                &self.workdir,
                &format!(
                    "`{}` exited with {code} but no files matched `files_pattern`{}",
                    self.command.join(" "),
                    snippet(&stdout, &stderr),
                ),
            )]);
        }
        Ok(violations)
    }
}

impl CommandIdempotentRule {
    /// One violation per offending file extracted from `stream`
    /// (per non-empty line: `files_pattern` group 1 if set, else
    /// the whole trimmed line). Lines that don't match the pattern
    /// are skipped.
    fn parse_offenders(&self, stream: &str) -> Vec<Violation> {
        let mut out = Vec::new();
        for line in stream.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let path = match &self.files_pattern {
                Some(re) => match re.captures(line).and_then(|c| c.get(1)) {
                    Some(m) => m.as_str(),
                    None => continue,
                },
                None => line,
            };
            out.push(self.violation(path, "not formatter-clean"));
        }
        out
    }

    fn violation(&self, path: &str, desc: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{path}: {desc}"));
        Violation::new(msg).with_path(PathBuf::from(path))
    }
}

/// A short, trimmed snippet of the checker's output for a
/// fallback message (lossy is fine for a hint; bounded so a noisy
/// formatter can't blow up the report). Empty ⇒ no suffix.
fn snippet(stdout: &str, stderr: &str) -> String {
    let joined = format!("{}\n{}", stdout.trim(), stderr.trim());
    let s = joined.trim();
    if s.is_empty() {
        return String::new();
    }
    let snip: String = s.chars().take(OUTPUT_SNIPPET_CAP).collect();
    format!(": {snip}")
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.command.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "command_idempotent requires a non-empty `command` argv \
             (the checker to run in its --check / idempotence mode)",
        ));
    }
    if opts.files_pattern.is_some() && opts.files_from == FilesFrom::None {
        return Err(Error::rule_config(
            &spec.id,
            "command_idempotent `files_pattern` requires `files_from: stdout|stderr`",
        ));
    }
    let files_pattern = match &opts.files_pattern {
        Some(p) => Some(Regex::new(p).map_err(|e| {
            Error::rule_config(&spec.id, format!("invalid `files_pattern` regex: {e}"))
        })?),
        None => None,
    };
    Ok(Box::new(CommandIdempotentRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        command: opts.command,
        workdir: opts.workdir.unwrap_or_else(|| ".".to_string()),
        files_from: opts.files_from,
        files_pattern,
        timeout: opts
            .timeout
            .unwrap_or(crate::spawn::DEFAULT_SPAWN_TIMEOUT_SECS),
    }))
}

// Tests shell out to `/bin/sh` to exercise the spawn / exit-code
// / offender-parsing paths without a per-OS fixture; gated to
// Unix (no `/bin/sh` on Windows CI), mirroring the `command`
// rule's test module.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::Path;

    fn rule(
        command: &[&str],
        files_from: FilesFrom,
        files_pattern: Option<&str>,
    ) -> CommandIdempotentRule {
        CommandIdempotentRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            command: command.iter().map(ToString::to_string).collect(),
            workdir: ".".into(),
            files_from,
            files_pattern: files_pattern.map(|p| Regex::new(p).unwrap()),
            timeout: 60,
        }
    }

    fn eval(r: &CommandIdempotentRule, root: &Path) -> Vec<Violation> {
        let idx = alint_core::FileIndex::from_entries(Vec::new());
        let ctx = Context {
            root,
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        r.evaluate(&ctx).unwrap()
    }

    #[test]
    fn zero_exit_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        let r = rule(&["/bin/sh", "-c", "exit 0"], FilesFrom::None, None);
        assert!(eval(&r, dir.path()).is_empty());
    }

    #[test]
    fn nonzero_exit_none_is_one_violation_with_output() {
        let dir = tempfile::tempdir().unwrap();
        let r = rule(
            &["/bin/sh", "-c", "echo 'would reformat' >&2; exit 1"],
            FilesFrom::None,
            None,
        );
        let v = eval(&r, dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path.as_deref(), Some(Path::new(".")));
        assert!(v[0].message.contains("not formatter-clean"));
        assert!(v[0].message.contains("would reformat"), "{:?}", v[0]);
    }

    #[test]
    fn files_from_stdout_bare_paths_one_violation_each() {
        let dir = tempfile::tempdir().unwrap();
        // gofmt -l / prettier --check shape: bare paths, one/line.
        let r = rule(
            &["/bin/sh", "-c", "printf 'src/a.rs\\nsrc/b.rs\\n'; exit 1"],
            FilesFrom::Stdout,
            None,
        );
        let v = eval(&r, dir.path());
        assert_eq!(v.len(), 2, "{v:?}");
        let paths: Vec<_> = v.iter().filter_map(|x| x.path.as_deref()).collect();
        assert!(paths.contains(&Path::new("src/a.rs")));
        assert!(paths.contains(&Path::new("src/b.rs")));
    }

    #[test]
    fn files_from_stderr_with_pattern_extracts_group_one() {
        let dir = tempfile::tempdir().unwrap();
        // cargo fmt --check shape: "Diff in <path> at line N".
        let script = "echo 'Diff in src/x.rs at line 4' >&2; \
                      echo 'noise that is not a path' >&2; \
                      echo 'Diff in src/y.rs at line 9' >&2; exit 1";
        let r = rule(
            &["/bin/sh", "-c", script],
            FilesFrom::Stderr,
            Some(r"Diff in (.+) at"),
        );
        let v = eval(&r, dir.path());
        assert_eq!(v.len(), 2, "non-matching line skipped: {v:?}");
        let paths: Vec<_> = v.iter().filter_map(|x| x.path.as_deref()).collect();
        assert!(paths.contains(&Path::new("src/x.rs")));
        assert!(paths.contains(&Path::new("src/y.rs")));
    }

    #[test]
    fn nonzero_but_no_parseable_files_falls_back_not_silent() {
        let dir = tempfile::tempdir().unwrap();
        let r = rule(
            &["/bin/sh", "-c", "echo 'totally unstructured' >&2; exit 1"],
            FilesFrom::Stderr,
            Some(r"^MATCH (.+)$"),
        );
        let v = eval(&r, dir.path());
        assert_eq!(v.len(), 1, "must not swallow a failure: {v:?}");
        assert!(v[0].message.contains("no files matched"));
    }

    #[test]
    fn spawn_failure_is_a_violation() {
        let dir = tempfile::tempdir().unwrap();
        let r = rule(&["alint-no-such-checker-xyz"], FilesFrom::None, None);
        let v = eval(&r, dir.path());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("could not be spawned"));
    }

    #[test]
    fn build_errors_on_empty_command_and_pattern_without_files_from() {
        let spec = crate::test_support::spec_yaml(
            "id: t\nkind: command_idempotent\ncommand: []\nlevel: error\n",
        );
        assert!(
            build(&spec)
                .unwrap_err()
                .to_string()
                .contains("non-empty `command`")
        );
        let spec = crate::test_support::spec_yaml(
            "id: t\nkind: command_idempotent\ncommand: [\"true\"]\n\
             files_pattern: \"(.+)\"\nlevel: error\n",
        );
        assert!(
            build(&spec)
                .unwrap_err()
                .to_string()
                .contains("`files_pattern` requires `files_from")
        );
    }

    #[test]
    fn bad_files_pattern_regex_is_a_build_error() {
        let spec = crate::test_support::spec_yaml(
            "id: t\nkind: command_idempotent\ncommand: [\"true\"]\n\
             files_from: stdout\nfiles_pattern: \"[\"\nlevel: error\n",
        );
        assert!(
            build(&spec)
                .unwrap_err()
                .to_string()
                .contains("invalid `files_pattern` regex")
        );
    }

    #[test]
    fn hung_checker_times_out_with_one_violation() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = rule(&["sh", "-c", "sleep 5"], FilesFrom::None, None);
        r.timeout = 1;
        let v = eval(&r, dir.path());
        assert_eq!(v.len(), 1, "a hung checker must yield one violation");
        assert!(
            v[0].message.contains("did not exit within 1s"),
            "{:?}",
            v[0].message
        );
    }
}
