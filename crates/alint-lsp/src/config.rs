//! Config discovery, per-config sessions, trust, and baseline filtering.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tower_lsp::lsp_types::{InitializeParams, Position, Range, Url};

use alint_core::baseline::Baseline;
use alint_core::{Engine, Level, Report, RuleEntry, RuleResult, WalkOptions, walk};

use crate::diagnostics::{TextSource, group_findings, hint_findings};
use alint_core::RuleRegistry;

use crate::{ConfigRun, Finding, Session, WorkspaceCheck};

/// How the server builds sessions: the `alint lsp` command line, and whether
/// the client vouched for the workspace.
#[derive(Debug, Clone, Default)]
pub(crate) struct Settings {
    /// Explicit reference date shared by full checks and buffer re-evaluation.
    pub(crate) analysis_time: Option<alint_core::AnalysisTime>,
    /// The client sent `trusted: true` (VS Code: a trusted workspace), or the
    /// config is the user's own (`--config`). Until then a session builds no
    /// rule or fact that spawns a process or runs Git, fetches no remote
    /// `extends:`, and reads nothing from the environment.
    pub(crate) trusted: bool,
    /// `--no-spawn`: a config with a spawning or Git rule, or a spawning fact,
    /// fails to load, as it does for `alint check --no-spawn`, trusted or not.
    pub(crate) no_spawn: bool,
    /// `--no-gitignore`: walk without `.gitignore` handling.
    pub(crate) no_gitignore: bool,
    /// `--config`: the config every document is linted with, rooted at its
    /// workspace folder, in place of the folder's own.
    pub(crate) config: Option<PathBuf>,
}

/// What identifies a session: the config it loads and the root it lints.
/// Without `--config` the root is the config's directory, as for `alint
/// check` run where the config was found; with it, the workspace folder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SessionKey {
    pub(crate) config: PathBuf,
    pub(crate) root: PathBuf,
}

/// A config that failed to load or build, as its diagnostic shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigError {
    pub(crate) message: String,
    /// Where in the config file the error is, when the file itself is not
    /// valid YAML; otherwise the diagnostic goes at its start.
    pub(crate) range: Option<Range>,
    /// The workspace's untrusted limits caused the error: a remote
    /// `extends:` or an environment value the session would not resolve.
    pub(crate) untrusted: bool,
}

impl From<String> for ConfigError {
    fn from(message: String) -> Self {
        Self {
            message,
            range: None,
            untrusted: false,
        }
    }
}

/// The note an untrusted workspace gets, once, when its config uses
/// something the session left out.
pub(crate) const UNTRUSTED_NOTE: &str = "alint: this workspace is not trusted, so command rules, \
     rules that run Git (`git_*` kinds, `git_tracked_only`, `changed_since`), custom facts, \
     remote `extends:` and environment values (`{{env.*}}`, `env.*` in `when:`) are off; other \
     rules lint as usual. To turn them on, trust the workspace: in VS Code, grant \
     Workspace Trust; in other editors, start the server with the initialization option \
     `\"trusted\": true`. See https://alint.org/docs/cli/lsp/#workspace-trust";

/// Run every config relevant to the workspace: each workspace folder's
/// config, which governs every open document in that folder.
pub(crate) fn check_workspace(
    folders: &[PathBuf],
    docs: &[(Url, Option<String>)],
    settings: &Settings,
    registry: &RuleRegistry,
) -> WorkspaceCheck {
    let fixed = settings.config.as_deref();
    let mut check = WorkspaceCheck::default();
    let mut keys: Vec<SessionKey> = Vec::new();
    for folder in folders {
        if let Some(key) = folder_key(folder, fixed)
            && !keys.contains(&key)
        {
            keys.push(key);
        }
    }
    for (uri, _) in docs {
        if let Ok(abs) = uri.to_file_path()
            && let Some(key) = session_key(&abs, folders, fixed)
        {
            if !keys.contains(&key) {
                keys.push(key.clone());
            }
            check.doc_config.insert(uri.clone(), key);
        }
    }
    for key in keys {
        let overlays: Vec<(PathBuf, String)> = docs
            .iter()
            .filter(|(uri, _)| check.doc_config.get(uri) == Some(&key))
            .filter_map(|(uri, text)| Some((uri.to_file_path().ok()?, text.clone()?)))
            .collect();
        let run = run_config(&key, &overlays, settings, registry);
        check.runs.push((key, run));
    }
    check
}

/// The session that governs `doc`, in its workspace folder (the deepest
/// folder holding it). A document outside every workspace folder is not
/// linted (`None`).
///
/// The folder's own config governs every file in the folder, as `alint check`
/// run from the folder does: the config discovery finds from the folder
/// (`alint_dsl::discover`), which may sit above it inside the folder's
/// repository, so a client rooted at a subdirectory still finds the
/// repository's config.
///
/// A folder without a config of its own (a directory holding several
/// repositories) lints a document with the config at the top of the nearest
/// repository above it, inside the folder: the nearest directory holding a
/// `.git` directory or file, as `alint check` run in that repository finds.
///
/// Either way, a `.alint.yml` below the governing config's directory (a
/// package's, or one in a vendored or ignored directory, which holds no
/// `.git`) never governs a file as a top-level config; it contributes rules
/// only through the governing config's `nested_configs: true`, with a nested
/// config's limits. With `--config` (`fixed`), that config governs every
/// folder, rooted at the folder.
pub(crate) fn session_key(
    doc: &Path,
    folders: &[PathBuf],
    fixed: Option<&Path>,
) -> Option<SessionKey> {
    let folder = folders
        .iter()
        .filter(|f| doc.starts_with(f))
        .max_by_key(|f| f.components().count())?;
    folder_key(folder, fixed).or_else(|| repository_key(doc, folder))
}

/// The session of the repository `doc` is in, for a folder without a config
/// of its own (see [`session_key`]): the config at the top of the nearest
/// repository above `doc`, when that top is inside `folder`.
fn repository_key(doc: &Path, folder: &Path) -> Option<SessionKey> {
    let top = alint_core::git::repository_top(doc.parent()?)?;
    if !top.starts_with(folder) {
        return None;
    }
    // Discovery from a repository's top checks the top and stops there.
    let config = alint_dsl::discover(&top)?;
    let root = config.parent()?.to_path_buf();
    Some(SessionKey { config, root })
}

/// The session a workspace folder's documents use (see [`session_key`]).
fn folder_key(folder: &Path, fixed: Option<&Path>) -> Option<SessionKey> {
    if let Some(config) = fixed {
        return Some(SessionKey {
            config: config.to_path_buf(),
            root: folder.to_path_buf(),
        });
    }
    let config = alint_dsl::discover(folder)?;
    let root = config
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    Some(SessionKey { config, root })
}

/// The config path that governs `doc` (test convenience over [`session_key`]).
#[cfg(test)]
pub(crate) fn config_for_document(doc: &Path, folders: &[PathBuf]) -> Option<PathBuf> {
    session_key(doc, folders, None).map(|key| key.config)
}

/// Build one config's session, run it over the tree, apply the
/// baseline, and overlay unsaved buffers. `overlays` are the open
/// documents this config governs, with their editor text.
fn run_config(
    key: &SessionKey,
    overlays: &[(PathBuf, String)],
    settings: &Settings,
    registry: &RuleRegistry,
) -> Result<ConfigRun, ConfigError> {
    let session = build_session_for_config(key, settings, registry)?;
    let report = session
        .engine
        .run(&session.root, &session.index)
        .map_err(|e| session.redact(&format!("running rules: {e}")))?;
    let notes: Vec<String> = report
        .results
        .iter()
        .flat_map(|r| r.notes.iter())
        .map(|n| match &n.path {
            Some(p) => format!("{}: {}", p.display(), n.message_after_path(p)),
            None => n.message.to_string(),
        })
        .collect();
    let results = apply_baseline(&session, report.results, None);
    let mut texts = TextSource::with_overlays(overlays.iter().cloned().collect());
    let mut by_path = group_findings(&session.group_ctx(), &results, &mut texts);
    if !session.pattern_hints.is_empty() {
        let hints = hint_findings(&session.pattern_hints, &session.config_path, &mut texts);
        by_path
            .entry(session.config_path.clone())
            .or_default()
            .extend(hints);
    }

    // Overlay: an open document whose buffer differs from disk gets its
    // per-file findings from the buffer, not the stale on-disk bytes.
    for (abs, text) in overlays {
        let on_disk = std::fs::read(abs).ok();
        if on_disk.as_deref() == Some(text.as_bytes()) {
            continue;
        }
        let Ok(rel) = abs.strip_prefix(&session.root) else {
            continue;
        };
        if let Ok(fresh) = eval_buffer(&session, abs, rel, text.clone()) {
            let entry = by_path.entry(abs.clone()).or_default();
            entry.retain(|f| !f.per_file);
            entry.extend(fresh);
        }
    }

    Ok(ConfigRun {
        session: Arc::new(session),
        by_path,
        notes,
    })
}

/// Run the per-file rules over an editor buffer and return the findings
/// for that document (baseline applied, columns converted against the
/// buffer). `Err(FileNotInIndex)` ⇒ the file is excluded from linting.
pub(crate) fn eval_buffer(
    session: &Session,
    abs: &Path,
    rel: &Path,
    text: String,
) -> alint_core::Result<Vec<Finding>> {
    let results = session
        .engine
        .run_for_file(&session.root, &session.index, rel, text.as_bytes())
        .map_err(|error| {
            alint_core::redact_environment_values_in_error(error, &session.environment_values)
        })?;
    let results = apply_baseline(session, results, Some((rel, text.as_bytes())));
    let mut texts = TextSource::with_overlays(HashMap::from([(abs.to_path_buf(), text)]));
    let mut by_path = group_findings(&session.group_ctx(), &results, &mut texts);
    Ok(by_path.remove(abs).unwrap_or_default())
}

/// Every workspace folder from the `initialize` params (multi-root),
/// falling back to the (deprecated) `root_uri`.
pub(crate) fn workspace_folders(params: &InitializeParams) -> Vec<PathBuf> {
    if let Some(folders) = &params.workspace_folders {
        let paths: Vec<PathBuf> = folders
            .iter()
            .filter_map(|f| f.uri.to_file_path().ok())
            .collect();
        if !paths.is_empty() {
            return paths;
        }
    }
    #[allow(deprecated)]
    params
        .root_uri
        .as_ref()
        .and_then(|u| u.to_file_path().ok())
        .into_iter()
        .collect()
}

/// Load the config discovered from `start` and build the engine +
/// index. Returns `Ok(None)` (not an error) when no config is present.
///
/// The session is trusted, as before workspace trust existed; tests of the
/// untrusted limits build theirs with [`build_session_with`].
#[cfg(test)]
pub(crate) fn build_session(start: &Path) -> Result<Option<Session>, String> {
    let settings = Settings {
        trusted: true,
        ..Settings::default()
    };
    build_session_with(start, &settings)
}

/// [`build_session`] with explicit settings.
#[cfg(test)]
pub(crate) fn build_session_with(
    start: &Path,
    settings: &Settings,
) -> Result<Option<Session>, String> {
    let Some(key) = folder_key(start, settings.config.as_deref()) else {
        return Ok(None);
    };
    build_session_for_config(&key, settings, &alint_rules::builtin_registry())
        .map(Some)
        .map_err(|error| error.message)
}

/// An environment that holds nothing, for an untrusted session's
/// `{{env.*}}` interpolation. It records whether the config asked for a
/// variable, so the session can tell the user why one resolved to nothing.
#[derive(Debug, Default)]
struct UntrustedEnvironment {
    asked: AtomicBool,
}

impl alint_dsl::EnvironmentProvider for UntrustedEnvironment {
    fn get(&self, _name: &str) -> Option<String> {
        self.asked.store(true, Ordering::Relaxed);
        None
    }
}

/// Where in `config` its YAML error is, when the file itself does not parse.
/// An error from another file of the chain (an `extends:` target, a drop-in)
/// or from a later stage keeps the diagnostic at the file's start.
fn yaml_error_range(config: &Path) -> Option<Range> {
    use alint_dsl::ConfigSourceProvider as _;
    // Read as the loader reads a config: only a regular file within the size
    // cap, so a config refused for its size (or a FIFO) is not read whole here.
    let text = alint_dsl::FilesystemConfigSource
        .read_to_string(config)
        .ok()?;
    let error = alint_core::yaml::from_str::<alint_core::yaml::Value>(&text).err()?;
    let location = error.location()?;
    let line_index = location.line().checked_sub(1)?;
    let line_text = text.lines().nth(line_index).unwrap_or_default();
    let (character, width) = crate::diagnostics::utf16_column(line_text, location.column());
    let line = u32::try_from(line_index).ok()?;
    Some(Range::new(
        Position::new(line, character),
        Position::new(line, character.saturating_add(width)),
    ))
}

/// Load `config_path` as [`build_session_for_config`] needs it: untrusted,
/// with an empty environment and remote `extends:` refused; with
/// `--no-spawn`, refusing a config that spawns. Returns the config and
/// whether loading it asked for an environment value an untrusted session
/// withheld.
fn load_session_config(
    config_path: &Path,
    settings: &Settings,
    registry: &RuleRegistry,
) -> Result<(alint_core::Config, bool), ConfigError> {
    let trusted = settings.trusted;
    let environment = Arc::new(UntrustedEnvironment::default());
    let mut load_options = alint_dsl::LoadOptions::with_catalog(registry.catalog());
    if !trusted {
        load_options.environment = environment.clone();
        load_options.remote_policy = alint_dsl::RemotePolicy::Deny;
    }
    let config = alint_dsl::load_with(config_path, &load_options).map_err(|error| {
        let message = error.to_string();
        let untrusted = !trusted
            && (environment.asked.load(Ordering::Relaxed)
                || message.contains("remote `extends` is disabled"));
        let mut message = format!("loading config: {message}");
        if untrusted {
            message.push_str("\n\n");
            message.push_str(UNTRUSTED_NOTE);
        }
        ConfigError {
            message,
            range: yaml_error_range(config_path),
            untrusted,
        }
    })?;
    if settings.no_spawn {
        let enabled = config
            .rules
            .iter()
            .filter(|spec| !matches!(spec.level, Level::Off));
        alint_dsl::refuse_spawning(enabled, &config.facts, registry.catalog(), "--no-spawn")
            .map_err(|error| format!("loading config: {error}"))?;
    }
    let asked = !trusted && environment.asked.load(Ordering::Relaxed);
    Ok((config, asked))
}

/// The engine entries for a config's enabled rules, from [`build_entries`].
struct BuiltRules {
    entries: Vec<RuleEntry>,
    /// Each rule id's canonical kind, for hover.
    kinds: HashMap<String, String>,
    /// The ids of the rules an `extends:`'d ruleset or a nested config shaped.
    extended: HashSet<String>,
    /// Whether an untrusted session skipped a spawning or Git rule or fix, or
    /// kept a `when:` that reads `env.*`.
    withheld: bool,
}

/// Build the engine entries for a config's enabled rules. An untrusted
/// session skips every rule that spawns a process or runs Git (which runs
/// the commands a repository's `.git/config` names), and keeps a rule whose
/// fix alone spawns (`fix: git_untrack`) without its fix.
fn build_entries(
    rules: &[alint_core::RuleSpec],
    allow_out_of_root: &alint_core::AllowOutOfRoot,
    trusted: bool,
    registry: &RuleRegistry,
) -> Result<BuiltRules, ConfigError> {
    let mut withheld = false;
    let mut entries: Vec<RuleEntry> = Vec::with_capacity(rules.len());
    let mut kinds: HashMap<String, String> = HashMap::new();
    let mut extended: HashSet<String> = HashSet::new();
    for spec in rules {
        if matches!(spec.level, Level::Off) {
            continue;
        }
        let mut without_fix;
        let spec = if !trusted && alint_dsl::rule_spawn_use(spec, registry.catalog()).is_some() {
            withheld = true;
            without_fix = spec.clone();
            without_fix.fix = None;
            if alint_dsl::rule_spawn_use(&without_fix, registry.catalog()).is_some() {
                continue;
            }
            &without_fix
        } else {
            spec
        };
        if !trusted && alint_dsl::rule_git_use(spec, registry.catalog()).is_some() {
            withheld = true;
            continue;
        }
        if spec.extended_source {
            extended.insert(spec.id.clone());
        }
        let mut rule = registry
            .build(spec)
            .map_err(|e| format!("building rule {:?}: {e}", spec.id))?;
        // Match the CLI's load contract: nested `require:` specs must fail at
        // session construction, not only if a later full run happens to select
        // a parent entry and builds them lazily.
        registry
            .validate_nested(spec, rule.as_ref())
            .map_err(|error| format!("building rule {:?}: {error}", spec.id))?;
        // Apply the top-level `allow_out_of_root:` policy (top-level
        // config only; never via `extends:`) to the user's own rules: a
        // rule an extended ruleset or a nested config shaped stays
        // confined. No-op for kinds that don't honor the flag.
        let allow_out_of_root = allow_out_of_root.allows_rule(spec);
        rule.set_allow_out_of_root(allow_out_of_root);
        let mut entry = RuleEntry::new(rule)
            .with_spec(std::sync::Arc::new(spec.clone()))
            .with_allow_out_of_root(allow_out_of_root);
        if let Some(when_src) = &spec.when {
            let expr = alint_core::when::parse(when_src).map_err(|error| {
                format!(
                    "rule {:?}: parsing `when`: {}",
                    spec.id,
                    spec.redact_when_error(error)
                )
            })?;
            withheld |= !trusted && expr.first_env_ref().is_some();
            entry = entry.with_when(expr);
        }
        // Resolve aliases through the session's registry, which may be an
        // injected one, so hover docs find the canonical kind.
        kinds.insert(
            spec.id.clone(),
            registry.canonical_kind(&spec.kind).to_string(),
        );
        entries.push(entry);
    }
    Ok(BuiltRules {
        entries,
        kinds,
        extended,
        withheld,
    })
}

/// Load the session's config and build the engine + index, honoring the same
/// top-level settings `alint check` does (`fix_size_limit`, `baseline:`,
/// `ignore:`, `respect_gitignore:`) and the `--no-gitignore` and
/// `--no-spawn` flags.
///
/// An untrusted session ([`Settings::trusted`]) loads with an empty
/// environment and refuses remote `extends:`, skips each rule and fact that
/// spawns a process ([`alint_dsl::rule_spawn_use`], [`alint_dsl::fact_spawns`])
/// and each rule that runs Git ([`alint_dsl::rule_git_use`]),
/// and gives `env.*` in a `when:` an empty environment. Everything else lints
/// as it would trusted. [`Session::withheld`] records whether any of that
/// changed what this config does.
fn build_session_for_config(
    key: &SessionKey,
    settings: &Settings,
    registry: &RuleRegistry,
) -> Result<Session, ConfigError> {
    let config_path = key.config.as_path();
    let effective_root = key.root.clone();
    let trusted = settings.trusted;
    let (config, mut withheld) = load_session_config(config_path, settings, registry)?;
    // A top-level value read from the environment can reach an error below
    // (an unreadable `baseline:`, an invalid `ignore:` glob in the walk).
    let environment_values = config.environment_values.clone();
    let redact = |message: String| {
        alint_core::redact_environment_values(&message, &environment_values).into_owned()
    };
    let mut facts = config.facts;
    if !trusted {
        let before = facts.len();
        facts.retain(|fact| !alint_dsl::fact_spawns(fact));
        withheld |= facts.len() != before;
    }
    let BuiltRules {
        entries,
        kinds,
        extended: extended_rules,
        withheld: rules_withheld,
    } = build_entries(&config.rules, &config.allow_out_of_root, trusted, registry)?;
    withheld |= rules_withheld;

    // A fact's id may be read from the environment.
    let pattern_hints: Vec<_> = entries
        .iter()
        .flat_map(|entry| entry.pattern_hints(registry))
        .chain(alint_core::pattern_hints::environment_fact_hints(
            &facts,
            &environment_values,
        ))
        .map(|mut hint| {
            hint.id = redact(hint.id);
            hint
        })
        .collect();
    let mut engine = Engine::from_entries(entries, registry.clone())
        .with_facts(facts)
        .with_vars(config.vars)
        .with_fix_size_limit(config.fix_size_limit);
    if !trusted {
        // `env.*` in a `when:` and a legacy `${VAR}` read this, not the host.
        engine = engine.with_condition_environment(BTreeMap::new());
    }
    if let Some(analysis_time) = settings.analysis_time {
        engine = engine.with_analysis_time(analysis_time);
    }

    // `baseline:` resolves against the repo root, exactly like `check`;
    // a missing / malformed baseline is a config error, never a silent
    // "suppress nothing".
    let baseline_path = config.baseline.as_ref().map(|b| effective_root.join(b));
    let baseline = match &baseline_path {
        Some(path) => Some(Baseline::load_file(path, Some(&effective_root)).map_err(redact)?),
        None => None,
    };
    let mut extra_ignores = config.ignore;
    if let Some(path) = &baseline_path
        && let Some(pattern) = alint_core::baseline::walk_exclude(&effective_root, path)
    {
        extra_ignores.push(pattern);
    }

    let walk_opts = WalkOptions::default()
        .with_respect_gitignore(config.respect_gitignore && !settings.no_gitignore)
        .with_extra_ignores(extra_ignores);
    let index = walk(&effective_root, &walk_opts)
        .map_err(|e| redact(format!("walking repository: {e}")))?;

    Ok(Session {
        root: effective_root,
        engine,
        index,
        config_path: config_path.to_path_buf(),
        baseline,
        kinds,
        pattern_hints,
        environment_values,
        withheld,
        extended_rules,
        config_files: config.config_files,
    })
}

/// Drop baseline-grandfathered violations from `results` (no-op without
/// a baseline). Fingerprints read the file's bytes for the line-content
/// discriminator; `overlay` supplies an editor buffer for one file so a
/// keystroke re-evaluation fingerprints what was actually linted.
fn apply_baseline(
    session: &Session,
    results: Vec<RuleResult>,
    overlay: Option<(&Path, &[u8])>,
) -> Vec<RuleResult> {
    let Some(baseline) = &session.baseline else {
        return results;
    };
    let report = Report { results };
    let mut cache: HashMap<PathBuf, Option<Vec<u8>>> = HashMap::new();
    let applied = alint_core::baseline::apply(&report, baseline, |rule_id, v| {
        let bytes: Option<&[u8]> = match v.path.as_deref() {
            Some(p) if overlay.is_some_and(|(op, _)| op == p) => overlay.map(|(_, b)| b),
            _ => alint_core::baseline::fingerprint_content(&session.root, v, &mut cache),
        };
        alint_core::baseline::fingerprint(rule_id, v, bytes)
    });
    applied.live.results
}

#[cfg(test)]
mod baseline_tests {
    use super::*;

    #[test]
    fn the_edited_buffer_fingerprints_from_its_text_and_other_files_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // A finding the baseline tells apart by its line's content.
        let rule = "rules:\n  - id: markers\n    kind: no_merge_conflict_markers\n    \
                    paths: '*.txt'\n    level: error\n";
        std::fs::write(root.join(".alint.yml"), format!("version: 1\n{rule}")).unwrap();
        for file in ["a.txt", "b.txt"] {
            std::fs::write(root.join(file), "<<<<<<< old\n").unwrap();
        }
        // Grandfather both files' findings as they are on disk.
        let session = build_session(root).unwrap().unwrap();
        let report = session.engine.run(&session.root, &session.index).unwrap();
        let baseline = Baseline::from_fingerprints(
            None,
            report.results.iter().flat_map(|result| {
                result.violations.iter().map(|v| {
                    let bytes = std::fs::read(root.join(v.path.as_deref().unwrap())).unwrap();
                    alint_core::baseline::FingerprintedViolation {
                        rule_id: result.rule_id.to_string(),
                        path: None,
                        fingerprint: alint_core::baseline::fingerprint(
                            &result.rule_id,
                            v,
                            Some(&bytes),
                        ),
                        message: None,
                    }
                })
            }),
        );
        assert_eq!(baseline.entries.len(), 2, "one per file");
        std::fs::write(root.join("base.jsonl"), baseline.to_jsonl()).unwrap();
        std::fs::write(
            root.join(".alint.yml"),
            format!("version: 1\nbaseline: base.jsonl\n{rule}"),
        )
        .unwrap();
        let session = build_session(root).unwrap().unwrap();
        let report = session.engine.run(&session.root, &session.index).unwrap();

        // `a.txt`'s buffer now reads differently, so its finding is new;
        // `b.txt` is fingerprinted from disk and stays grandfathered.
        let live = apply_baseline(
            &session,
            report.results,
            Some((Path::new("a.txt"), b"<<<<<<< new\n")),
        );
        let paths: Vec<_> = live
            .iter()
            .flat_map(|result| &result.violations)
            .filter_map(|v| v.path.as_deref())
            .collect();
        assert_eq!(paths, [Path::new("a.txt")]);
    }

    #[test]
    fn a_hidden_path_finding_matches_the_path_free_fingerprint_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut hidden = alint_core::Violation::new("m").with_path(Path::new("any.txt"));
        hidden.path_sensitive = true;
        hidden.redact_message("markers");
        let current = alint_core::baseline::fingerprint("markers", &hidden, None);
        // What v0.21 wrote: the path, hashed. v0.23 no longer matches it.
        let v021 = |path: &str| {
            alint_core::baseline::violation_fingerprint(
                "markers",
                Some(Path::new(path)),
                None,
                Some("sensitive"),
                "",
                None,
            )
        };
        let entry = |fingerprint: String| alint_core::baseline::BaselineEntry {
            rule_id: "markers".into(),
            path: Some("<redacted>".into()),
            fingerprint,
            count: 1,
            message: None,
        };
        std::fs::write(
            root.join(".alint.yml"),
            "version: 1\nbaseline: base.jsonl\nrules:\n  - id: markers\n    \
             kind: no_merge_conflict_markers\n    paths: '*.txt'\n    level: error\n",
        )
        .unwrap();
        std::fs::write(root.join("a.txt"), "<<<<<<< old\n").unwrap();
        for (recorded, live) in [(current, 0), (v021("a.txt"), 1)] {
            let baseline = Baseline {
                alint_version: None,
                entries: vec![entry(recorded.clone())],
            };
            std::fs::write(root.join("base.jsonl"), baseline.to_jsonl()).unwrap();
            let session = build_session(root).unwrap().unwrap();
            let mut report = session.engine.run(&session.root, &session.index).unwrap();
            // As the engine leaves a finding whose path the configuration
            // read from the environment.
            for violation in &mut report.results[0].violations {
                violation.path_sensitive = true;
                violation.redact_message("markers");
            }
            let left = apply_baseline(&session, report.results, None);
            let count: usize = left.iter().map(|result| result.violations.len()).sum();
            assert_eq!(count, live, "baseline records {recorded}");
        }
    }

    /// A `baseline:` key is repository content: the server reads its file
    /// only when it is a regular file within the cap, inside the workspace's
    /// root and outside any Git directory there, as `alint check` does. A
    /// symlink to `/dev/zero` used to be read until memory ran out, and a FIFO
    /// blocked the server.
    #[cfg(unix)]
    #[test]
    fn the_baseline_key_names_a_regular_file_in_the_repository() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let valid = Baseline::from_fingerprints(None, Vec::new()).to_jsonl();
        std::fs::write(dir.path().join("outside.jsonl"), &valid).unwrap();
        std::fs::write(root.join(".git/base.jsonl"), &valid).unwrap();
        std::fs::write(root.join("base.jsonl"), &valid).unwrap();
        std::os::unix::fs::symlink("/dev/zero", root.join("zero.jsonl")).unwrap();
        std::os::unix::fs::symlink("../outside.jsonl", root.join("link.jsonl")).unwrap();
        std::fs::File::create(root.join("sparse.jsonl"))
            .unwrap()
            .set_len(1 << 40)
            .unwrap();
        let fifo = std::process::Command::new("mkfifo")
            .arg(root.join("fifo.jsonl"))
            .status()
            .is_ok_and(|status| status.success());
        let config = |baseline: &str| {
            std::fs::write(
                root.join(".alint.yml"),
                format!("version: 1\nbaseline: {baseline}\nrules: []\n"),
            )
            .unwrap();
        };
        for (baseline, refusal) in [
            ("zero.jsonl", "resolves outside the repository"),
            ("../outside.jsonl", "resolves outside the repository"),
            ("link.jsonl", "resolves outside the repository"),
            (".git/base.jsonl", "is in a Git directory"),
            ("sparse.jsonl", "larger than 256 MiB"),
            ("fifo.jsonl", "not a regular file"),
        ] {
            if baseline == "fifo.jsonl" && !fifo {
                continue;
            }
            config(baseline);
            let error = build_session(&root).err().unwrap_or_default();
            assert!(error.contains(refusal), "{baseline}: {error}");
        }
        config("base.jsonl");
        assert!(build_session(&root).unwrap().unwrap().baseline.is_some());
    }
}
