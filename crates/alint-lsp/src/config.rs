//! Config discovery, per-config sessions, and baseline filtering.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp::lsp_types::{InitializeParams, Url};

use alint_core::baseline::Baseline;
use alint_core::{Engine, Level, Report, RuleEntry, RuleResult, WalkOptions, walk};

use crate::diagnostics::{TextSource, group_findings};
use crate::{ConfigRun, Finding, Session, WorkspaceCheck};

/// Run every config relevant to the workspace: the config each open
/// document resolves to, plus each workspace folder's own config (so a
/// config error surfaces even when the open file lives elsewhere).
pub(crate) fn check_workspace(
    folders: &[PathBuf],
    docs: &[(Url, Option<String>)],
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
            && let Some(config) = config_for_document(&abs, folders)
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
        let run = run_config(&config, &overlays);
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
pub(crate) fn config_for_document(doc: &Path, folders: &[PathBuf]) -> Option<PathBuf> {
    let folder = folders
        .iter()
        .filter(|f| doc.starts_with(f))
        .max_by_key(|f| f.components().count())?;
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
        if alint_dsl::nested_configs_enabled(&current).unwrap_or(false) {
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
fn run_config(config_path: &Path, overlays: &[(PathBuf, String)]) -> Result<ConfigRun, String> {
    let session = build_session_for_config(config_path)?;
    let report = session
        .engine
        .run(&session.root, &session.index)
        .map_err(|e| format!("running rules: {e}"))?;
    let notes: Vec<String> = report
        .results
        .iter()
        .flat_map(|r| r.notes.iter())
        .map(|n| match &n.path {
            Some(p) => format!("{}: {}", p.display(), n.message),
            None => n.message.to_string(),
        })
        .collect();
    let results = apply_baseline(&session, report.results, None);
    let mut texts = TextSource::with_overlays(overlays.iter().cloned().collect());
    let mut by_path = group_findings(&session.group_ctx(), &results, &mut texts);

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
    let results =
        session
            .engine
            .run_for_file(&session.root, &session.index, rel, text.as_bytes())?;
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
        Some(config_path) => build_session_for_config(&config_path).map(Some),
        None => Ok(None),
    }
}

/// Load `config_path` and build the engine + index, honoring the same
/// top-level settings `alint check` does (`fix_size_limit`, `baseline:`,
/// `ignore:`, `respect_gitignore:`).
fn build_session_for_config(config_path: &Path) -> Result<Session, String> {
    // The config's directory is the effective repo root, so relative
    // paths in rules resolve from there, matching the CLI.
    let effective_root = config_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let config = alint_dsl::load(config_path).map_err(|e| format!("loading config: {e}"))?;

    let registry = alint_rules::builtin_registry();
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
        rule.validate_nested(&registry)
            .map_err(|e| format!("building rule {:?}: {e}", spec.id))?;
        // Apply the top-level `allow_out_of_root:` policy (top-level
        // config only; never via `extends:`). No-op for kinds that
        // don't honor the flag.
        let allow_out_of_root = config.allow_out_of_root.allows(&spec.id, &spec.kind);
        rule.set_allow_out_of_root(allow_out_of_root);
        let mut entry = RuleEntry::new(rule)
            .with_spec(std::sync::Arc::new(spec.clone()))
            .with_allow_out_of_root(allow_out_of_root);
        if let Some(when_src) = &spec.when {
            let expr = alint_core::when::parse(when_src)
                .map_err(|e| format!("rule {:?}: parsing `when`: {e}", spec.id))?;
            entry = entry.with_when(expr);
        }
        kinds.insert(spec.id.clone(), spec.kind.clone());
        entries.push(entry);
    }

    let engine = Engine::from_entries(entries, registry)
        .with_facts(config.facts)
        .with_vars(config.vars)
        .with_fix_size_limit(config.fix_size_limit);

    // `baseline:` resolves against the repo root, exactly like `check`;
    // a missing / malformed baseline is a config error, never a silent
    // "suppress nothing".
    let baseline_path = config.baseline.as_ref().map(|b| effective_root.join(b));
    let baseline = match &baseline_path {
        Some(path) => Some(load_baseline(path)?),
        None => None,
    };
    let mut extra_ignores = config.ignore;
    if let Some(path) = &baseline_path
        && let Some(pattern) = baseline_walk_exclude(&effective_root, path)
    {
        extra_ignores.push(pattern);
    }

    let walk_opts = WalkOptions {
        respect_gitignore: config.respect_gitignore,
        extra_ignores,
    };
    let index =
        walk(&effective_root, &walk_opts).map_err(|e| format!("walking repository: {e}"))?;

    Ok(Session {
        root: effective_root,
        engine,
        index,
        config_path: config_path.to_path_buf(),
        baseline,
        kinds,
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

/// The baseline file as a root-anchored walk-exclude pattern, so a
/// broad content rule doesn't lint alint's own artifact (mirrors the
/// CLI's `baseline_walk_exclude`).
fn baseline_walk_exclude(root: &Path, baseline: &Path) -> Option<String> {
    let root_abs = root.canonicalize().ok()?;
    let base_abs = baseline.canonicalize().ok()?;
    let rel = base_abs.strip_prefix(&root_abs).ok()?;
    Some(format!("/{}", rel.to_string_lossy().replace('\\', "/")))
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
            Some(p) => cache
                .entry(p.to_path_buf())
                .or_insert_with(|| {
                    let full = session.root.join(p);
                    let size = std::fs::metadata(&full).map_or(0, |m| m.len());
                    alint_core::read_capped_or_skip(&full, size)
                })
                .as_deref(),
            None => None,
        };
        alint_core::baseline::fingerprint(rule_id, v, bytes)
    });
    applied.live.results
}
