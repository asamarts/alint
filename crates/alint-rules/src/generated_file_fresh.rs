//! `generated_file_fresh` — a committed artefact must match what a
//! declared generator produces. Two modes, selected by shape:
//!
//! - **stdout mode** (`file:`) — the generator writes its single
//!   output to stdout; alint captures it and compares to the one
//!   committed `file:`. Never writes the tree.
//! - **mutating / in-place mode** (`outputs:`) — the generator
//!   writes its outputs *in place*; alint **snapshots** them, runs
//!   the generator, **diffs**, reports any file it would change, and
//!   **restores the snapshot** — so `alint check` is left pure (no
//!   regenerated files behind). The alint-native form of
//!   `make gen && git diff --exit-code`. See
//!   `docs/design/v0.12/generated_file_fresh_mutating.md`.
//!
//! Either mode runs a user-supplied process, so the kind is
//! trust-gated at config load by `alint_dsl::reject_command_rules_in`
//! (same tier as `command` / `command_idempotent`): only the user's
//! own top-level config may declare it; an `extends:`'d ruleset is
//! refused — adopting a ruleset must never imply code execution.
//! Single-shot (one spawn), not per-file. Design + the stdout-mode
//! open-question resolutions: `docs/design/v0.10/generated_file_fresh.md`.
//!
//! ```yaml
//! - id: bindings-fresh                # stdout mode
//!   kind: generated_file_fresh
//!   file: crates/ffi/include/core.h
//!   command: ["cbindgen", "--config", "cbindgen.toml", "crates/core"]
//!
//! - id: commands-def-fresh            # mutating mode
//!   kind: generated_file_fresh
//!   outputs: "src/commands.def"       # glob or list; selects mutating mode
//!   command: ["make", "commands.def"]
//!   normalize: final-newline
//!   level: error
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use alint_core::{
    Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation, WalkOptions, walk,
};
use serde::Deserialize;

/// Maximum number of per-file staleness violations the mutating mode
/// emits before collapsing the tail into one summary line.
const MAX_STALE_REPORTED: usize = 50;

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum Normalize {
    /// Exact byte equality.
    #[default]
    None,
    /// Trim leading/trailing whitespace of the whole output.
    Trim,
    /// Normalise only a single trailing newline (the most common
    /// generator/editor diff).
    FinalNewline,
}

impl Normalize {
    fn apply(self, s: &str) -> String {
        match self {
            Self::None => s.to_string(),
            Self::Trim => s.trim().to_string(),
            Self::FinalNewline => s.strip_suffix('\n').unwrap_or(s).to_string(),
        }
    }

    /// True when `a` and `b` differ after applying the transform.
    fn differs(self, a: &[u8], b: &[u8]) -> bool {
        if self == Self::None {
            a != b
        } else {
            self.apply(&String::from_utf8_lossy(a)) != self.apply(&String::from_utf8_lossy(b))
        }
    }
}

/// `outputs:` accepts a single glob or a list (a `Scope`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged, expecting = "a glob string, or a list of globs")]
enum OutputsSpec {
    /// A single output glob.
    One(String),
    /// A non-empty list of output globs.
    Many(#[schemars(length(min = 1))] Vec<String>),
}

impl OutputsSpec {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(v) => v,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// STDOUT mode: the committed generated file to verify against the
    /// generator's stdout.
    #[serde(default)]
    file: Option<String>,
    /// MUTATING mode: the glob (or list of globs) the in-place generator
    /// rewrites; its presence selects the mutating mode. alint snapshots these,
    /// runs the generator, diffs, and restores them.
    #[serde(default)]
    outputs: Option<OutputsSpec>,
    /// Generator argv (no shell). STDOUT mode: emit the file's contents to
    /// stdout. MUTATING mode: write the `outputs` in place.
    #[schemars(length(min = 1))]
    command: Vec<String>,
    /// Generator cwd, relative to the lint root (default: lint root).
    #[serde(default)]
    workdir: Option<String>,
    /// Normalization applied before comparison to absorb trailing-newline
    /// churn: `none`, `trim`, or `final-newline`.
    #[serde(default)]
    normalize: Normalize,
    /// Generator timeout in seconds (default 120). On timeout the child is
    /// killed and one violation is emitted.
    #[serde(default)]
    #[schemars(range(min = 1))]
    timeout: Option<u64>,
}

crate::options_schema_for!(Options);

/// Which freshness check to run, fixed at load.
#[derive(Debug)]
enum Mode {
    /// Compare one committed `file` to the generator's stdout.
    Stdout { file: String },
    /// Snapshot the `outputs` scope, run the in-place generator, diff,
    /// then restore. `globs` are kept for the violation message.
    Mutating { outputs: Scope, globs: Vec<String> },
}

#[derive(Debug)]
pub struct GeneratedFileFreshRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    mode: Mode,
    command: Vec<String>,
    workdir: String,
    normalize: Normalize,
    timeout: u64,
}

impl Rule for GeneratedFileFreshRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Single-shot: staleness is independent of which files
        // changed, so it always evaluates (never `--changed`-
        // filtered). `path_scope` stays `None` (default) so the
        // engine doesn't skip-by-intersection. Same dispatch
        // class as `pair`.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        match &self.mode {
            Mode::Stdout { file } => Ok(self.eval_stdout(ctx, file)),
            Mode::Mutating { outputs, globs } => Ok(self.eval_mutating(ctx, outputs, globs)),
        }
    }
}

impl GeneratedFileFreshRule {
    /// Build the shared child environment + spawn the generator.
    fn run(&self, ctx: &Context<'_>) -> crate::spawn::SpawnOutcome {
        let env = [
            ("ALINT_ROOT", ctx.root.to_string_lossy().into_owned()),
            ("ALINT_RULE_ID", self.id.clone()),
            ("ALINT_LEVEL", self.level.as_str().to_string()),
        ];
        crate::spawn::run_capturing(
            &self.command,
            &ctx.root.join(&self.workdir),
            &env,
            Duration::from_secs(self.timeout),
        )
    }

    // ─── stdout mode (shipped behaviour, unchanged) ──────────────

    fn eval_stdout(&self, ctx: &Context<'_>, file: &str) -> Vec<Violation> {
        let file = Path::new(file);
        // Confine the committed-output path before we spawn or read. This
        // kind is spawn-gated (an untrusted `extends:` can't introduce it),
        // so this is defense-in-depth keeping the "every config-derived
        // path read is confined" invariant total.
        let Some(file_rel) = crate::pathsafe::normalize_confined(file) else {
            return vec![self.violation(file, "escapes the repo root")];
        };
        let (status, stdout, stderr) = match self.run(ctx) {
            crate::spawn::SpawnOutcome::Exited {
                status,
                stdout,
                stderr,
            } => (status, stdout, stderr),
            crate::spawn::SpawnOutcome::SpawnError(e) => {
                let program = self.command.first().map_or("", String::as_str);
                return vec![self.violation(
                    file,
                    &format!("generator `{program}` could not be spawned: {e}"),
                )];
            }
            crate::spawn::SpawnOutcome::TimedOut { secs } => {
                return vec![self.violation(
                    file,
                    &format!(
                        "generator did not exit within {secs}s \
                         (raise `timeout:` on the rule to extend)"
                    ),
                )];
            }
        };

        if !status.success() {
            return vec![self.violation(file, &exit_desc(status, &stderr))];
        }

        let committed = match crate::io::read_capped(&ctx.root.join(&file_rel)) {
            Ok(b) => b,
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                return vec![self.violation(
                    file,
                    &format!("is too large to diff ({})", crate::io::over_cap(n)),
                )];
            }
            Err(crate::io::ReadCapError::Io(_)) => {
                return vec![self.violation(
                    file,
                    "is not on disk, but the generator produced output for it",
                )];
            }
        };

        if self.normalize.differs(&committed, &stdout) {
            return vec![self.violation(
                file,
                &format!(
                    "is stale - its committed contents differ from `{}` output{}",
                    self.command.join(" "),
                    first_diff_hint(&stdout, &committed),
                ),
            )];
        }
        Vec::new()
    }

    // ─── mutating / in-place mode ────────────────────────────────

    fn eval_mutating(
        &self,
        ctx: &Context<'_>,
        outputs: &Scope,
        globs: &[String],
    ) -> Vec<Violation> {
        // 1. Snapshot every output file that exists now. The restorer
        //    holds the originals and writes them back on Drop — so an
        //    early return, error, or panic still restores the tree.
        let mut restorer = OutputRestorer::new(ctx.root);
        for entry in ctx.index.files() {
            if outputs.matches(&entry.path, ctx.index) {
                match crate::io::read_capped(&ctx.root.join(&entry.path)) {
                    Ok(bytes) => restorer.snapshot(entry.path.clone(), bytes),
                    Err(crate::io::ReadCapError::TooLarge(n)) => {
                        return vec![self.fail(&format!(
                            "output `{}` is too large to snapshot ({})",
                            entry.path.display(),
                            crate::io::over_cap(n)
                        ))];
                    }
                    // In the index but unreadable now — skip it.
                    Err(crate::io::ReadCapError::Io(_)) => {}
                }
            }
        }

        // 2. Run the in-place generator.
        match self.run(ctx) {
            crate::spawn::SpawnOutcome::Exited { status, stderr, .. } if !status.success() => {
                return vec![self.fail(&exit_desc(status, &stderr))];
            }
            crate::spawn::SpawnOutcome::SpawnError(e) => {
                let program = self.command.first().map_or("", String::as_str);
                return vec![
                    self.fail(&format!("generator `{program}` could not be spawned: {e}")),
                ];
            }
            crate::spawn::SpawnOutcome::TimedOut { secs } => {
                return vec![self.fail(&format!(
                    "generator did not exit within {secs}s (raise `timeout:` to extend)"
                ))];
            }
            crate::spawn::SpawnOutcome::Exited { .. } => {}
        }

        // 3. Diff. Changed / removed are found against the snapshot;
        //    new files need a fresh walk of the scope.
        let cmd = self.command.join(" ");
        let mut stale: Vec<(Arc<Path>, String)> = Vec::new();
        for (path, before) in restorer.snapshots() {
            // Capped read (M3-F7): a pathologically large generated file must
            // not be read unbounded into memory. Over-cap / unreadable is
            // reported distinctly from removed so the message stays accurate.
            match crate::io::read_capped(&ctx.root.join(path)) {
                Ok(after) if self.normalize.differs(before, &after) => stale.push((
                    path.clone(),
                    format!("is out of date - re-run `{cmd}` and commit the result"),
                )),
                Ok(_) => {}
                Err(crate::io::ReadCapError::Io(e))
                    if e.kind() == std::io::ErrorKind::NotFound =>
                {
                    stale.push((
                        path.clone(),
                        format!("was removed by `{cmd}` - re-run it and commit the result"),
                    ));
                }
                Err(_) => stale.push((
                    path.clone(),
                    format!(
                        "could not be read to verify freshness (too large or unreadable) - re-run `{cmd}`"
                    ),
                )),
            }
        }
        // New in-scope files the generator created (register them so
        // the restorer deletes them, then flag them).
        let walk_opts = WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        if let Ok(after_index) = walk(ctx.root, &walk_opts) {
            for entry in after_index.files() {
                if outputs.matches(&entry.path, &after_index) && !restorer.has(&entry.path) {
                    restorer.register_new(entry.path.clone());
                    stale.push((
                        entry.path.clone(),
                        format!("is an uncommitted generated file - `{cmd}` created it; commit it"),
                    ));
                }
            }
        }

        if stale.is_empty() && restorer.is_empty() {
            // `outputs:` matched nothing and produced nothing — almost
            // always a glob typo; surface it rather than pass silently.
            return vec![self.fail(&format!(
                "`outputs:` glob {globs:?} matched no files and the generator produced none"
            ))];
        }

        // 4. Report (the restorer restores on Drop, below).
        stale.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out: Vec<Violation> = stale
            .iter()
            .take(MAX_STALE_REPORTED)
            .map(|(path, desc)| self.violation(path, desc))
            .collect();
        if stale.len() > MAX_STALE_REPORTED {
            out.push(self.fail(&format!(
                "…and {} more stale output(s)",
                stale.len() - MAX_STALE_REPORTED
            )));
        }
        out
        // 5. `restorer` drops here → tree restored byte-for-byte.
    }

    /// A path-anchored violation (stdout mode + per-output mutating).
    fn violation(&self, file: &Path, desc: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{}: {desc}", file.display()));
        Violation::new(msg).with_path(file.to_path_buf())
    }

    /// A rule-level violation (generator spawn/exit failure, no
    /// single file to anchor on).
    fn fail(&self, desc: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("generated_file_fresh `{}`: {desc}", self.id));
        // No path to anchor on; key on a stable identity so the fingerprint
        // doesn't fall through to the volatile message (anti-panic) branch.
        Violation::new(msg).with_baseline_key("generated-file-fresh-generator-failure")
    }
}

/// Describe a non-success exit (shared by both modes).
fn exit_desc(status: std::process::ExitStatus, stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let snippet: String = stderr.trim().chars().take(400).collect();
    let code = status
        .code()
        .map_or_else(|| "a signal".to_string(), |c| c.to_string());
    format!("generator exited with {code}: {snippet}")
}

/// Restores the working tree on Drop: writes every snapshot file back
/// and deletes every generator-created file. Best-effort (Drop can't
/// surface errors) and panic-safe - `alint check` must not leave a
/// mutating generator's output behind.
struct OutputRestorer<'a> {
    root: &'a Path,
    snapshots: HashMap<Arc<Path>, Vec<u8>>,
    new_files: Vec<Arc<Path>>,
}

impl<'a> OutputRestorer<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            snapshots: HashMap::new(),
            new_files: Vec::new(),
        }
    }

    fn snapshot(&mut self, path: Arc<Path>, bytes: Vec<u8>) {
        self.snapshots.insert(path, bytes);
    }

    fn register_new(&mut self, path: Arc<Path>) {
        self.new_files.push(path);
    }

    fn has(&self, path: &Path) -> bool {
        self.snapshots.contains_key(path)
    }

    fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    fn snapshots(&self) -> impl Iterator<Item = (&Arc<Path>, &Vec<u8>)> {
        self.snapshots.iter()
    }
}

impl Drop for OutputRestorer<'_> {
    fn drop(&mut self) {
        for (path, bytes) in &self.snapshots {
            let full = self.root.join(path);
            // Only rewrite when the generator actually changed the
            // file. An unconditional write bumps the mtime on every
            // `alint check` — even an idempotent generator that left
            // the output untouched — which needlessly invalidates
            // build caches and trips file watchers.
            // Capped read (M3-F7): over-cap / unreadable falls through to the
            // rewrite below with the freshly-generated bytes — correct behavior.
            if matches!(crate::io::read_capped(&full), Ok(current) if current == *bytes) {
                continue;
            }
            let _ = std::fs::write(&full, bytes);
        }
        for path in &self.new_files {
            let _ = std::fs::remove_file(self.root.join(path));
        }
    }
}

/// A short hint at where the generator output and the committed
/// file first diverge (line-based; lossy is fine for a hint).
fn first_diff_hint(produced: &[u8], committed: &[u8]) -> String {
    let p = String::from_utf8_lossy(produced);
    let c = String::from_utf8_lossy(committed);
    for (i, (lp, lc)) in p.lines().zip(c.lines()).enumerate() {
        if lp != lc {
            return format!(" (first differs at line {})", i + 1);
        }
    }
    let (np, nc) = (p.lines().count(), c.lines().count());
    if np == nc {
        String::new()
    } else {
        format!(" (generator produced {np} lines, file has {nc})")
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.command.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "generated_file_fresh requires a non-empty `command` argv (the generator)",
        ));
    }
    let mode = match (opts.file, opts.outputs) {
        (Some(f), None) => {
            if f.trim().is_empty() {
                return Err(Error::rule_config(
                    &spec.id,
                    "generated_file_fresh `file` must not be empty",
                ));
            }
            Mode::Stdout { file: f }
        }
        (None, Some(o)) => {
            let globs = o.into_vec();
            if globs.iter().all(|g| g.trim().is_empty()) {
                return Err(Error::rule_config(
                    &spec.id,
                    "generated_file_fresh `outputs` must not be empty",
                ));
            }
            let outputs = Scope::from_patterns(&globs)
                .map_err(|e| Error::rule_config(&spec.id, format!("invalid `outputs`: {e}")))?;
            Mode::Mutating { outputs, globs }
        }
        (Some(_), Some(_)) => {
            return Err(Error::rule_config(
                &spec.id,
                "generated_file_fresh: set exactly one of `file` (stdout mode) or `outputs` \
                 (mutating/in-place mode), not both",
            ));
        }
        (None, None) => {
            return Err(Error::rule_config(
                &spec.id,
                "generated_file_fresh requires `file` (compare the generator's stdout to one \
                 committed file) or `outputs` (the in-place generator's regenerated globs)",
            ));
        }
    };
    Ok(Box::new(GeneratedFileFreshRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        mode,
        command: opts.command,
        workdir: opts.workdir.unwrap_or_else(|| ".".to_string()),
        normalize: opts.normalize,
        timeout: opts
            .timeout
            .unwrap_or(crate::spawn::DEFAULT_SPAWN_TIMEOUT_SECS),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdout_rule(file: &str, command: &[&str], normalize: Normalize) -> GeneratedFileFreshRule {
        GeneratedFileFreshRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            mode: Mode::Stdout { file: file.into() },
            command: command.iter().map(ToString::to_string).collect(),
            workdir: ".".into(),
            normalize,
            timeout: 60,
        }
    }

    fn mutating_rule(
        globs: &[&str],
        command: &[&str],
        normalize: Normalize,
    ) -> GeneratedFileFreshRule {
        let globs: Vec<String> = globs.iter().map(ToString::to_string).collect();
        GeneratedFileFreshRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            mode: Mode::Mutating {
                outputs: Scope::from_patterns(&globs).unwrap(),
                globs,
            },
            command: command.iter().map(ToString::to_string).collect(),
            workdir: ".".into(),
            normalize,
            timeout: 60,
        }
    }

    fn eval_in(r: &GeneratedFileFreshRule, root: &Path) -> Vec<Violation> {
        // A fresh on-disk walk so the index reflects current files
        // (the mutating mode snapshots from the index).
        let idx = walk(
            root,
            &WalkOptions {
                respect_gitignore: true,
                extra_ignores: Vec::new(),
            },
        )
        .unwrap();
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
    fn stdout_file_escape_fires_without_spawning() {
        // Defense-in-depth (v0.12 path-confinement): an absolute / escaping
        // `file:` fires "escapes the repo root" before the generator runs.
        let dir = tempfile::tempdir().unwrap();
        let r = stdout_rule("/etc/hostname", &["true"], Normalize::None);
        let v = eval_in(&r, dir.path());
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("escapes the repo root"),
            "{}",
            v[0].message
        );
    }

    // ─── stdout mode (unchanged) ─────────────────────────────────

    #[test]
    fn fresh_file_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("out.txt"), "alpha\nbravo\n").unwrap();
        let r = stdout_rule(
            "out.txt",
            &["sh", "-c", "printf 'alpha\\nbravo\\n'"],
            Normalize::None,
        );
        assert!(eval_in(&r, dir.path()).is_empty());
    }

    #[test]
    fn stale_file_fails_with_line_hint() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("out.txt"), "alpha\nWRONG\n").unwrap();
        let r = stdout_rule(
            "out.txt",
            &["sh", "-c", "printf 'alpha\\nbravo\\n'"],
            Normalize::None,
        );
        let v = eval_in(&r, dir.path());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("stale"));
        assert!(v[0].message.contains("line 2"), "{:?}", v[0].message);
    }

    #[test]
    fn trim_normalize_absorbs_surrounding_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("out.txt"), "  hello\n\n").unwrap();
        let g = ["sh", "-c", "printf hello"];
        assert_eq!(
            eval_in(&stdout_rule("out.txt", &g, Normalize::None), dir.path()).len(),
            1,
            "exact-byte compare sees the whitespace diff"
        );
        assert!(
            eval_in(&stdout_rule("out.txt", &g, Normalize::Trim), dir.path()).is_empty(),
            "trim normalize absorbs surrounding whitespace"
        );
    }

    #[test]
    fn missing_committed_file_is_a_violation() {
        let dir = tempfile::tempdir().unwrap();
        let r = stdout_rule("nope.txt", &["sh", "-c", "printf x"], Normalize::None);
        let v = eval_in(&r, dir.path());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("not on disk"));
    }

    #[test]
    fn generator_nonzero_exit_is_a_violation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("out.txt"), "x").unwrap();
        let r = stdout_rule(
            "out.txt",
            &["sh", "-c", "echo boom >&2; exit 3"],
            Normalize::None,
        );
        let v = eval_in(&r, dir.path());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("exited with 3"));
        assert!(v[0].message.contains("boom"));
    }

    // ─── mutating / in-place mode ────────────────────────────────

    #[test]
    fn mutating_fresh_outputs_are_silent_and_tree_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("gen.txt");
        std::fs::write(&p, "fresh\n").unwrap();
        // Generator rewrites the SAME content it already has.
        let r = mutating_rule(
            &["gen.txt"],
            &["sh", "-c", "printf 'fresh\\n' > gen.txt"],
            Normalize::None,
        );
        assert!(
            eval_in(&r, dir.path()).is_empty(),
            "regenerated == committed"
        );
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "fresh\n");
    }

    #[test]
    fn mutating_stale_output_fires_then_restores() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("gen.txt");
        std::fs::write(&p, "STALE\n").unwrap();
        // Generator would rewrite it to "fresh\n" → out of date.
        let r = mutating_rule(
            &["gen.txt"],
            &["sh", "-c", "printf 'fresh\\n' > gen.txt"],
            Normalize::None,
        );
        let v = eval_in(&r, dir.path());
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("out of date"), "{}", v[0].message);
        assert_eq!(v[0].path.as_deref(), Some(Path::new("gen.txt")));
        // The tree is restored to the original committed bytes.
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "STALE\n",
            "alint check must leave the tree byte-identical"
        );
    }

    #[test]
    fn mutating_new_file_fires_and_is_deleted_on_restore() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.gen"), "x\n").unwrap();
        // Generator creates a NEW in-scope file the user never committed.
        let r = mutating_rule(
            &["*.gen"],
            &["sh", "-c", "printf 'y\\n' > b.gen"],
            Normalize::None,
        );
        let v = eval_in(&r, dir.path());
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("uncommitted generated file"),
            "{}",
            v[0].message
        );
        assert_eq!(v[0].path.as_deref(), Some(Path::new("b.gen")));
        assert!(
            !dir.path().join("b.gen").exists(),
            "new file deleted on restore"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.gen")).unwrap(),
            "x\n"
        );
    }

    #[test]
    fn mutating_final_newline_normalize_absorbs_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gen.txt"), "data").unwrap(); // no trailing NL
        let r = mutating_rule(
            &["gen.txt"],
            &["sh", "-c", "printf 'data\\n' > gen.txt"], // adds one
            Normalize::FinalNewline,
        );
        assert!(
            eval_in(&r, dir.path()).is_empty(),
            "final-newline normalize absorbs it"
        );
    }

    #[test]
    fn mutating_generator_failure_is_one_violation_and_restores() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gen.txt"), "orig\n").unwrap();
        // Generator mutates then fails — the tree must still be restored.
        let r = mutating_rule(
            &["gen.txt"],
            &["sh", "-c", "printf 'half\\n' > gen.txt; exit 7"],
            Normalize::None,
        );
        let v = eval_in(&r, dir.path());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("exited with 7"), "{}", v[0].message);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("gen.txt")).unwrap(),
            "orig\n",
            "a failed generator's partial write is rolled back"
        );
    }

    #[test]
    fn build_rejects_both_file_and_outputs() {
        let yaml = "id: t\nkind: generated_file_fresh\nfile: a\noutputs: b\ncommand: [\"x\"]\nlevel: error\n";
        let err = build(&crate::test_support::spec_yaml(yaml))
            .unwrap_err()
            .to_string();
        assert!(err.contains("exactly one"), "{err}");
    }

    #[test]
    fn build_rejects_neither_file_nor_outputs() {
        let yaml = "id: t\nkind: generated_file_fresh\ncommand: [\"x\"]\nlevel: error\n";
        let err = build(&crate::test_support::spec_yaml(yaml))
            .unwrap_err()
            .to_string();
        assert!(err.contains("`file`") && err.contains("`outputs`"), "{err}");
    }

    #[test]
    fn build_accepts_outputs_list() {
        let yaml = "id: t\nkind: generated_file_fresh\noutputs: [\"a/**\", \"b.txt\"]\ncommand: [\"make\"]\nlevel: error\n";
        assert!(build(&crate::test_support::spec_yaml(yaml)).is_ok());
    }
}
