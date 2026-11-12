//! Config discovery, per-config sessions, and baseline filtering.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp::lsp_types::{InitializeParams, Url};

use alint_core::baseline::Baseline;
use alint_core::{Engine, Level, Report, RuleEntry, RuleResult, WalkOptions, walk};

use crate::diagnostics::{TextSource, group_findings, hint_findings};
use alint_core::RuleRegistry;

use crate::{ConfigRun, Finding, Session, WorkspaceCheck};

/// Run every config relevant to the workspace: the config each open
/// document resolves to, plus each workspace folder's own config (so a
/// config error surfaces even when the open file lives elsewhere).
pub(crate) fn check_workspace(
    folders: &[PathBuf],
    docs: &[(Url, Option<String>)],
    analysis_time: Option<alint_core::AnalysisTime>,
    registry: &RuleRegistry,
) -> WorkspaceCheck {
    let mut check = WorkspaceCheck::default();
    let mut configs: Vec<PathBuf> = Vec::new();
    for folder in folders {
        if let Some(config) = alint_dsl::discover(folder)
            && !configs.contains(&config)
        {
            configs.push(config);
        }
    }
    for (uri, _) in docs {
        if let Ok(abs) = uri.to_file_path()
            && let Some(config) = config_for_document_with_registry(&abs, folders, registry)
        {
            if !configs.contains(&config) {
                configs.push(config.clone());
            }
            check.doc_config.insert(uri.clone(), config);
        }
    }
    for config in configs {
        let overlays: Vec<(PathBuf, String)> = docs
            .iter()
            .filter(|(uri, _)| check.doc_config.get(uri) == Some(&config))
            .filter_map(|(uri, text)| Some((uri.to_file_path().ok()?, text.clone()?)))
            .collect();
        let run = run_config(&config, &overlays, analysis_time, registry);
        check.runs.push((config, run));
    }
    check
}

/// The config that governs `doc`: the nearest `.alint.yml` walking up
/// from the document's directory — the same search the CLI does from
/// its working directory. A document outside every workspace folder is
/// not linted (`None`). The walk may continue above the folder, so a
/// client rooted at a subdirectory still finds the repo's config.
///
/// `nested_configs: true` follows the CLI: run from the workspace folder,
/// the CLI loads the folder's config, which lifts every nested
/// `.alint.yml` into one rule set. So when a config between the nearest
/// one and the folder's own config (inclusive) enables
/// `nested_configs:`, the outermost such config governs the document
/// (its rules AND the nested config's, scoped), not the nearest config
/// alone.
#[cfg(test)]
pub(crate) fn config_for_document(doc: &Path, folders: &[PathBuf]) -> Option<PathBuf> {
    config_for_document_with_registry(doc, folders, &alint_rules::builtin_registry())
}

pub(crate) fn config_for_document_with_registry(
    doc: &Path,
    folders: &[PathBuf],
    registry: &RuleRegistry,
) -> Option<PathBuf> {
    let folder = folders
        .iter()
        .filter(|f| doc.starts_with(f))
        .max_by_key(|f| f.components().count())?;
    let load_options = alint_dsl::LoadOptions::with_catalog(registry.catalog());
    let nearest = alint_dsl::discover(doc.parent()?)?;
    let Some(top) = alint_dsl::discover(folder) else {
        return Some(nearest);
    };
    let Some(top_dir) = top.parent() else {
        return Some(nearest);
    };
    // Walk the config chain upward from the nearest config to the
    // folder's own config, remembering the outermost that enables
    // nested configs.
    let mut governing = nearest.clone();
    let mut current = nearest;
    loop {
        if alint_dsl::nested_configs_enabled_with(&current, &load_options).unwrap_or(false) {
            governing.clone_from(&current);
        }
        if current == top {
            break;
        }
        let Some(next) = current
            .parent()
            .and_then(Path::parent)
            .and_then(alint_dsl::discover)
        else {
            break;
        };
        // Stop once the walk leaves the folder's own config's tree.
        if !next.parent().is_some_and(|d| d.starts_with(top_dir)) {
            break;
        }
        current = next;
    }
    Some(governing)
}

/// Build one config's session, run it over the tree, apply the
/// baseline, and overlay unsaved buffers. `overlays` are the open
/// documents this config governs, with their editor text.
fn run_config(
    config_path: &Path,
    overlays: &[(PathBuf, String)],
    analysis_time: Option<alint_core::AnalysisTime>,
    registry: &RuleRegistry,
) -> Result<ConfigRun, String> {
    let session = build_session_for_config(config_path, analysis_time, registry)?;
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
#[cfg(test)]
pub(crate) fn build_session(start: &Path) -> Result<Option<Session>, String> {
    match alint_dsl::discover(start) {
        Some(config_path) => {
            build_session_for_config(&config_path, None, &alint_rules::builtin_registry()).map(Some)
        }
        None => Ok(None),
    }
}

/// Load `config_path` and build the engine + index, honoring the same
/// top-level settings `alint check` does (`fix_size_limit`, `baseline:`,
/// `ignore:`, `respect_gitignore:`).
fn build_session_for_config(
    config_path: &Path,
    analysis_time: Option<alint_core::AnalysisTime>,
    registry: &RuleRegistry,
) -> Result<Session, String> {
    // The config's directory is the effective repo root, so relative
    // paths in rules resolve from there, matching the CLI.
    let effective_root = config_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let config = alint_dsl::load_with(
        config_path,
        &alint_dsl::LoadOptions::with_catalog(registry.catalog()),
    )
    .map_err(|e| format!("loading config: {e}"))?;
    // A top-level value read from the environment can reach an error below
    // (an unreadable `baseline:`, an invalid `ignore:` glob in the walk).
    let environment_values = config.environment_values.clone();
    let redact = |message: String| {
        alint_core::redact_environment_values(&message, &environment_values).into_owned()
    };
    let mut entries: Vec<RuleEntry> = Vec::with_capacity(config.rules.len());
    let mut kinds: HashMap<String, String> = HashMap::new();
    for spec in &config.rules {
        if matches!(spec.level, Level::Off) {
            continue;
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
        // config only; never via `extends:`). No-op for kinds that
        // don't honor the flag.
        let allow_out_of_root = config.allow_out_of_root.allows(&spec.id, &spec.kind);
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

    // A fact's id may be read from the environment.
    let pattern_hints: Vec<_> = entries
        .iter()
        .flat_map(|entry| entry.pattern_hints(registry))
        .chain(alint_core::pattern_hints::environment_fact_hints(
            &config.facts,
            &environment_values,
        ))
        .map(|mut hint| {
            hint.id = redact(hint.id);
            hint
        })
        .collect();
    let mut engine = Engine::from_entries(entries, registry.clone())
        .with_facts(config.facts)
        .with_vars(config.vars)
        .with_fix_size_limit(config.fix_size_limit);
    if let Some(analysis_time) = analysis_time {
        engine = engine.with_analysis_time(analysis_time);
    }

    // `baseline:` resolves against the repo root, exactly like `check`;
    // a missing / malformed baseline is a config error, never a silent
    // "suppress nothing".
    let baseline_path = config.baseline.as_ref().map(|b| effective_root.join(b));
    let baseline = match &baseline_path {
        Some(path) => Some(load_baseline(path).map_err(redact)?),
        None => None,
    };
    let mut extra_ignores = config.ignore;
    if let Some(path) = &baseline_path
        && let Some(pattern) = alint_core::baseline::walk_exclude(&effective_root, path)
    {
        extra_ignores.push(pattern);
    }

    let walk_opts = WalkOptions {
        respect_gitignore: config.respect_gitignore,
        extra_ignores,
    };
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
    })
}

/// Read + parse a baseline file (same contract as `alint check`).
fn load_baseline(path: &Path) -> Result<Baseline, String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "reading baseline file {} (run `alint baseline` to create it): {e}",
            path.display()
        )
    })?;
    Baseline::load(&text).map_err(|e| format!("invalid baseline file {}: {e}", path.display()))
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
}
