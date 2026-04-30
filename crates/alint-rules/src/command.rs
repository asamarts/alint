//! `command` — shell out to an external CLI per matched file.
//!
//! Per-file rule: for every file matching `paths`, spawn the
//! given `command:` argv with path-template substitution, capture
//! exit code and stdout/stderr. Exit `0` is a pass; non-zero is
//! one violation whose message is the (truncated) stdout+stderr.
//! Spawn / wait failures and timeouts produce a violation with a
//! clear cause line.
//!
//! ```yaml
//! - id: workflows-clean
//!   kind: command
//!   paths: ".github/workflows/*.{yml,yaml}"
//!   command: ["actionlint", "{path}"]
//!   level: error
//! ```
//!
//! Path templates supported in argv tokens (and in the alint-injected
//! `ALINT_PATH` env var): `{path}`, `{dir}`, `{stem}`, `{ext}`,
//! `{basename}`, `{parent_name}`. Working directory is the alint
//! root. Stdin is closed (`/dev/null`).
//!
//! Environment threaded into the child:
//!
//! - `ALINT_PATH` — relative path of the matched file.
//! - `ALINT_ROOT` — absolute repo root.
//! - `ALINT_RULE_ID` — the rule's `id:`.
//! - `ALINT_LEVEL` — `error` / `warning` / `info`.
//! - `ALINT_VAR_<NAME>` — one per top-level `vars:` entry,
//!   uppercased.
//! - `ALINT_FACT_<NAME>` — one per resolved fact, stringified.
//!
//! Trust model: `command` rules are only allowed in the user's own
//! top-level config. Any extended source (local file, HTTPS URL,
//! `alint://bundled/`) declaring `kind: command` is rejected at
//! load time by `alint_dsl::reject_command_rules_in` — otherwise a
//! malicious or compromised ruleset would gain arbitrary process
//! execution simply by being fetched. Mirrors the existing
//! `custom:` fact gate.

use std::io::Read;
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

use alint_core::template::{PathTokens, render_path};
use alint_core::{Context, Error, FactValue, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

/// Default per-file timeout. Generous for slow tools (kubeconform
/// pulling schemas, slow shellcheck on large files) but bounded
/// enough to not stall a CI run on a hung child indefinitely.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Cap on each of stdout / stderr captured into a violation
/// message. Tools like cargo can emit tens of MB on a single
/// failed file; bound it to keep reports legible and memory low.
const OUTPUT_CAP_BYTES: usize = 16 * 1024;

/// Granularity of the wait-loop. 10ms is short enough that fast
/// tools (10–50ms typical for shellcheck per file) don't see
/// noticeable polling overhead, and long enough to keep CPU
/// idle while the child runs.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    command: Vec<String>,
    /// Per-file timeout in seconds. Default
    /// [`DEFAULT_TIMEOUT_SECS`].
    #[serde(default)]
    timeout: Option<u64>,
}

#[derive(Debug)]
pub struct CommandRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    argv: Vec<String>,
    timeout: Duration,
}

impl Rule for CommandRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            let tokens = PathTokens::from_path(&entry.path);
            let rendered: Vec<String> = self.argv.iter().map(|s| render_path(s, &tokens)).collect();
            if let Outcome::Fail(msg) = run_one(
                &rendered,
                ctx.root,
                &entry.path,
                &self.id,
                self.level,
                ctx,
                self.timeout,
            ) {
                let final_msg = self.message.clone().unwrap_or(msg);
                violations.push(Violation::new(final_msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }
}

/// Outcome of one per-file invocation. `Pass` produces no
/// violation; `Fail(message)` becomes a single violation
/// anchored on the file path.
enum Outcome {
    Pass,
    Fail(String),
}

#[allow(clippy::too_many_arguments)] // Fewer args = more state-keeping; this is the natural shape.
fn run_one(
    argv: &[String],
    root: &Path,
    rel_path: &Path,
    rule_id: &str,
    level: Level,
    ctx: &Context<'_>,
    timeout: Duration,
) -> Outcome {
    let Some((program, rest)) = argv.split_first() else {
        return Outcome::Fail("command rule's argv is empty".to_string());
    };

    let mut cmd = StdCommand::new(program);
    cmd.args(rest)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("ALINT_PATH", rel_path.to_string_lossy().as_ref())
        .env("ALINT_ROOT", root.to_string_lossy().as_ref())
        .env("ALINT_RULE_ID", rule_id)
        .env("ALINT_LEVEL", level.as_str());

    if let Some(vars) = ctx.vars {
        for (k, v) in vars {
            cmd.env(format!("ALINT_VAR_{}", k.to_uppercase()), v);
        }
    }
    if let Some(facts) = ctx.facts {
        for (k, v) in facts.as_map() {
            cmd.env(format!("ALINT_FACT_{}", k.to_uppercase()), fact_to_env(v));
        }
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Outcome::Fail(format!(
                "could not spawn `{}`: {} \
                 (is it on PATH? working dir: {})",
                program,
                e,
                root.display()
            ));
        }
    };

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout_bytes = drain(child.stdout.take());
                let stderr_bytes = drain(child.stderr.take());
                if status.success() {
                    return Outcome::Pass;
                }
                return Outcome::Fail(format_failure(
                    program,
                    status.code(),
                    &stdout_bytes,
                    &stderr_bytes,
                ));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::Fail(format!(
                        "`{}` did not exit within {}s (raise `timeout:` on the rule to extend)",
                        program,
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Outcome::Fail(format!("`{program}` wait error: {e}"));
            }
        }
    }
}

/// Read up to [`OUTPUT_CAP_BYTES`] from a captured pipe. Errors
/// drain to an empty buffer so the failure-message render still
/// produces something useful for the user.
fn drain(pipe: Option<impl Read>) -> Vec<u8> {
    let Some(mut p) = pipe else {
        return Vec::new();
    };
    let mut buf = Vec::with_capacity(1024);
    let _ = p
        .by_ref()
        .take(OUTPUT_CAP_BYTES as u64)
        .read_to_end(&mut buf);
    buf
}

fn format_failure(program: &str, code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> String {
    let stdout_s = lossy_trim(stdout);
    let stderr_s = lossy_trim(stderr);
    let exit = code.map_or_else(|| "killed by signal".to_string(), |c| format!("exit {c}"));
    match (stdout_s.is_empty(), stderr_s.is_empty()) {
        (true, true) => format!("`{program}` failed ({exit}); no output"),
        (false, true) => format!("`{program}` failed ({exit}):\n{stdout_s}"),
        (true, false) => format!("`{program}` failed ({exit}):\n{stderr_s}"),
        (false, false) => format!("`{program}` failed ({exit}):\n{stdout_s}\n{stderr_s}"),
    }
}

fn lossy_trim(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim_end().to_string()
}

fn fact_to_env(v: &FactValue) -> String {
    match v {
        FactValue::Bool(b) => b.to_string(),
        FactValue::Int(i) => i.to_string(),
        FactValue::String(s) => s.clone(),
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "command requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.command.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "command rule's `command:` argv must not be empty",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "command rules do not support `fix:` blocks in v0.5.x — \
             wire a paired fix-on-save tool via a separate `command` \
             rule (or another rule kind) for now",
        ));
    }
    let timeout = Duration::from_secs(opts.timeout.unwrap_or(DEFAULT_TIMEOUT_SECS));
    Ok(Box::new(CommandRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        argv: opts.command,
        timeout,
    }))
}

// Tests below shell out to `/bin/sh` and `/bin/true` to
// exercise the spawn / argv-template / timeout paths without
// pulling in a per-OS test fixture. That doesn't translate to
// Windows (`/bin/sh` doesn't exist), so the whole module is
// gated to Unix targets — Cross-Platform / windows-latest skips
// it cleanly while Linux + macOS continue to exercise it.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex};

    fn idx(paths: &[&str]) -> FileIndex {
        FileIndex {
            entries: paths
                .iter()
                .map(|p| FileEntry {
                    path: std::path::Path::new(p).into(),
                    is_dir: false,
                    size: 1,
                })
                .collect(),
        }
    }

    fn rule(argv: Vec<&str>, scope: &str, timeout: Duration) -> CommandRule {
        CommandRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::from_patterns(&[scope.to_string()]).unwrap(),
            argv: argv.into_iter().map(String::from).collect(),
            timeout,
        }
    }

    fn ctx<'a>(root: &'a Path, index: &'a FileIndex) -> Context<'a> {
        Context {
            root,
            index,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        }
    }

    #[test]
    fn pass_on_zero_exit() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
        let index = idx(&["a.txt"]);
        let r = rule(
            vec!["/bin/sh", "-c", "exit 0"],
            "*.txt",
            Duration::from_secs(5),
        );
        let v = r.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert!(v.is_empty(), "unexpected violations: {v:?}");
    }

    #[test]
    fn fail_on_nonzero_exit_carries_stderr() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"x").unwrap();
        let index = idx(&["a.txt"]);
        let r = rule(
            vec!["/bin/sh", "-c", "echo problem >&2; exit 7"],
            "*.txt",
            Duration::from_secs(5),
        );
        let v = r.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path.as_deref(), Some(Path::new("a.txt")));
        assert!(v[0].message.contains("exit 7"), "msg: {}", v[0].message);
        assert!(v[0].message.contains("problem"), "msg: {}", v[0].message);
    }

    #[test]
    fn path_template_substitutes_in_argv() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"hi").unwrap();
        let index = idx(&["a.txt"]);
        // Echo the arg back via stderr so we can match it.
        // `[ "$1" = "a.txt" ]` exits 0 on equal.
        let r = rule(
            vec![
                "/bin/sh",
                "-c",
                "[ \"$1\" = a.txt ] || exit 1",
                "_",
                "{path}",
            ],
            "*.txt",
            Duration::from_secs(5),
        );
        let v = r.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert!(v.is_empty(), "argv substitution failed: {v:?}");
    }

    #[test]
    fn timeout_emits_violation() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"x").unwrap();
        let index = idx(&["a.txt"]);
        let r = rule(
            vec!["/bin/sh", "-c", "sleep 5"],
            "*.txt",
            Duration::from_millis(150),
        );
        let v = r.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(v.len(), 1);
        assert!(
            v[0].message.contains("did not exit"),
            "msg: {}",
            v[0].message
        );
    }

    #[test]
    fn unknown_program_produces_spawn_error_violation() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"x").unwrap();
        let index = idx(&["a.txt"]);
        let r = rule(
            vec!["alint-no-such-program-xyzzy"],
            "*.txt",
            Duration::from_secs(2),
        );
        let v = r.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("could not spawn"));
    }

    #[test]
    fn alint_path_env_set_for_child() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"x").unwrap();
        let index = idx(&["a.txt"]);
        // Child fails unless ALINT_PATH matches.
        let r = rule(
            vec!["/bin/sh", "-c", "[ \"$ALINT_PATH\" = a.txt ] || exit 1"],
            "*.txt",
            Duration::from_secs(5),
        );
        let v = r.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert!(v.is_empty(), "ALINT_PATH not set: {v:?}");
    }

    #[test]
    fn empty_argv_rejected_at_build_time() {
        let yaml = r#"
id: t
kind: command
level: error
paths: "*.txt"
command: []
"#;
        let spec: RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let err = build(&spec).expect_err("empty argv must error");
        assert!(format!("{err}").contains("argv must not be empty"));
    }

    #[test]
    fn missing_paths_rejected_at_build_time() {
        let yaml = r#"
id: t
kind: command
level: error
command: ["/bin/true"]
"#;
        let spec: RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let err = build(&spec).expect_err("missing paths must error");
        assert!(format!("{err}").contains("requires a `paths` field"));
    }

    #[test]
    fn fix_block_rejected_at_build_time() {
        let yaml = r#"
id: t
kind: command
level: error
paths: "*.txt"
command: ["/bin/true"]
fix:
  file_remove: {}
"#;
        let spec: RuleSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let err = build(&spec).expect_err("fix on command rule must error");
        assert!(format!("{err}").contains("do not support `fix:`"));
    }
}
