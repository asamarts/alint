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
    /// Where in the config file the error is, when the file itself has a YAML
    /// error or the error names a key path in this file; otherwise the
    /// diagnostic goes at its start.
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
     rules that run Git (`git_*` kinds, `changeset_requires_path`, `pair_changed_together`, \
     `git_tracked_only`, `changed_since`), custom facts, remote `extends:`, \
     `allow_out_of_root:` and environment values (`{{env.*}}`, `env.*` in `when:`) are off; \
     other rules lint as usual. To turn them on, trust the workspace: in VS Code, grant \
     Workspace Trust; in JetBrains IDEs, trust the project; in other editors, start the server \
     with the initialization option `\"trusted\": true`. See \
     https://alint.org/docs/cli/lsp/#workspace-trust";

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
        // The path once (a `constraint` note's message starts with its path,
        // escaped, which `message_after_path` leaves out), and `<redacted>`
        // when it is hidden, as `alint check --show-notes` hides it. The path
        // is escaped as alint's messages escape text (`\u{1b}`, `\n`), where
        // `--show-notes` writes it with the terminal's escapes (`\x1b`,
        // `\x0a`): either way no control, bidi or zero-width character is
        // left raw, and `render_notes` escapes any control left in a message.
        .map(|n| match n.report_path() {
            Some(p) => format!(
                "{}: {}",
                alint_core::Violation::escaped_for_message(&p.display().to_string()),
                n.message_after_path(p)
            ),
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
    // Cap the on-disk read to the buffer's byte length: a larger regular file
    // cannot equal the buffer, and a FIFO/device is refused before opening.
    for (abs, text) in overlays {
        let on_disk = alint_core::bounded_file::read_regular_capped(abs, text.len() as u64).ok();
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
        .map_err(|error| session.environment_redactor.redact_error(error))?;
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

/// Where in `config` a config load or build error sits (#571):
/// 1. Ensures `message` belongs to `config` itself (not another file in an
///    `extends:` chain, `.alint.d/` drop-in, nested config, or redacted path),
///    returning `None` (file start) for foreign errors.
/// 2. If `message` is a duplicate-key / duplicate-field error, highlights the
///    full repeated key token in `config` as a UTF-16 LSP [`Range`].
/// 3. Otherwise, if `message` names a key path (`rules[3].paths`,
///    `relations.r.fields.name.at`, `selectors.s.paths`, `extends[0].as`,
///    an unknown key, or a rule/fact/template id), resolves that path in
///    `config` to its UTF-16 LSP [`Range`].
/// 4. Falls back to an inline `at line L column C` location when attributed to
///    `config`, else a guarded YAML syntax parse of `config`, else `None`.
fn config_error_range(config: &Path, message: &str) -> Option<Range> {
    use alint_dsl::ConfigSourceProvider as _;
    // Read as the loader reads a config: only a regular file within the size
    // cap, so a config refused for its size (or a FIFO) is not read whole here.
    let text = alint_dsl::FilesystemConfigSource
        .read_to_string(config)
        .ok()?;
    config_error_range_in_text(config, &text, message)
}

fn config_error_range_in_text(config: &Path, text: &str, message: &str) -> Option<Range> {
    // Strip a single leading UTF-8 BOM to match `diagnostics::TextSource` and
    // editor buffer columns.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let body = strip_own_file_prefix(config, message)?;
    if let Some(span) = alint_core::yaml::locate_duplicate_key_span(text, body)
        && let Some(range) = byte_span_to_range(text, span)
    {
        return Some(range);
    }
    if let Some(segments) = extract_error_key_path(body)
        && let Some(span) = alint_core::yaml::locate_config_path_span(text, &segments)
        && let Some(range) = byte_span_to_range(text, span)
    {
        return Some(range);
    }
    if let Some((line, col)) = extract_line_column(body) {
        return line_col_to_range(text, line, col);
    }
    // Last-resort fallback: re-parse `text` for a syntax location only after
    // pre-parse flow/alias guards pass so an alias/flow bomb never materializes
    // here.
    let scan = alint_core::yaml_depth::scan_guards(text);
    if scan.flow_within_limit()
        && scan.expansion_within_limit(text)
        && let Err(error) = alint_core::yaml::from_str::<alint_core::yaml::Value>(text)
        && let Some(location) = error.location()
    {
        return line_col_to_range(text, location.line(), location.column());
    }
    None
}

fn line_col_to_range(text: &str, line: usize, column: usize) -> Option<Range> {
    let line_index = line.checked_sub(1)?;
    let line_text = alint_core::physical_lines(text)
        .nth(line_index)
        .map(|(_, line)| line)
        .unwrap_or_default();
    let (character, width) = crate::diagnostics::utf16_column(line_text, column);
    let line = u32::try_from(line_index).ok()?;
    Some(Range::new(
        Position::new(line, character),
        Position::new(line, character.saturating_add(width)),
    ))
}

fn byte_span_to_range(text: &str, span: alint_core::ByteSpan) -> Option<Range> {
    let index = alint_core::yaml::LineIndex::new(text);
    let (start_line, start_col) = index.utf16_position(span.start)?;
    let end_offset = span.end.max(span.start);
    let (end_line, end_col) = index
        .utf16_position(end_offset)
        .unwrap_or((start_line, start_col));
    let (end_line, end_col) = if (end_line, end_col) == (start_line, start_col) {
        (start_line, start_col.saturating_add(1))
    } else {
        (end_line, end_col)
    };
    Some(Range::new(
        Position::new(start_line, start_col),
        Position::new(end_line, end_col),
    ))
}

/// Strip a leading `"loading config: "` wrapper and, if the error starts with
/// a file path, ensure that path names `config` itself rather than an
/// `extends:` target, `.alint.d/` drop-in, nested config, or redacted path
/// (returning `None` for other files).
fn strip_own_file_prefix<'a>(config: &Path, message: &'a str) -> Option<&'a str> {
    let first_line = message.lines().next().unwrap_or(message);
    let rest = first_line
        .strip_prefix("loading config: ")
        .unwrap_or(first_line);
    if rest.starts_with("<redacted>:") || rest.starts_with("nested config ") {
        return None;
    }
    if is_own_config_message_prefix(rest) {
        return Some(rest.strip_prefix("YAML parse error: ").unwrap_or(rest));
    }
    if let Some((prefix, tail)) = rest.split_once(": ") {
        if prefix_names_config(config, prefix) {
            return Some(tail.strip_prefix("YAML parse error: ").unwrap_or(tail));
        }
        if looks_like_foreign_file_prefix(prefix) {
            return None;
        }
    }
    Some(rest.strip_prefix("YAML parse error: ").unwrap_or(rest))
}

fn is_own_config_message_prefix(rest: &str) -> bool {
    [
        "reading baseline file ",
        "invalid baseline file ",
        "`extends:` entry ",
        "remote `extends` ",
        "resolving ",
        "building rule \"",
        "rule \"",
        "fact \"",
        "template \"",
        "walking repository: ",
        "relation limit ",
    ]
    .iter()
    .any(|prefix| rest.starts_with(prefix))
}

fn looks_like_foreign_file_prefix(prefix: &str) -> bool {
    if prefix.contains('/')
        || prefix.contains('\\')
        || prefix.starts_with("alint://")
        || prefix.starts_with("https://")
        || prefix.starts_with("http://")
    {
        return true;
    }
    if prefix.is_empty() || prefix.contains(char::is_whitespace) || prefix.contains('[') {
        return false;
    }
    let Some(ext) = Path::new(prefix).extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let first_segment = prefix.split('.').next().unwrap_or_default();
    if ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml") {
        return !matches!(
            first_segment,
            "vars" | "relations" | "selectors" | "templates"
        );
    }
    if is_known_top_level_config_key(first_segment) {
        return false;
    }
    (1..=8).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn is_known_top_level_config_key(segment: &str) -> bool {
    matches!(
        segment,
        "version"
            | "requires"
            | "extends"
            | "rules"
            | "relations"
            | "selectors"
            | "facts"
            | "templates"
            | "vars"
            | "limits"
            | "baseline"
            | "ignore"
            | "respect_gitignore"
            | "fix_size_limit"
            | "allow_out_of_root"
    )
}

fn strip_windows_verbatim(path: &str) -> String {
    if let Some(unc) = path
        .strip_prefix("//?/UNC/")
        .or_else(|| path.strip_prefix("//?/unc/"))
    {
        format!("//{unc}")
    } else {
        path.strip_prefix("//?/").unwrap_or(path).to_owned()
    }
}

fn prefix_names_config(config: &Path, prefix: &str) -> bool {
    let norm = |s: &str| strip_windows_verbatim(&s.replace('\\', "/"));
    let p_owned = norm(prefix);
    let p = p_owned.strip_prefix("./").unwrap_or(&p_owned);
    let full = norm(&config.display().to_string());
    if p == full {
        return true;
    }
    if let Some(name) = config.file_name().and_then(|n| n.to_str())
        && p == name
    {
        return true;
    }
    std::fs::canonicalize(config).is_ok_and(|canon| {
        let c = norm(&canon.display().to_string());
        p == c
    })
}

/// Extract the YAML configuration path named by a loader or rule-build error.
fn extract_error_key_path(body: &str) -> Option<Vec<alint_core::yaml::ConfigPathSegment>> {
    use alint_core::yaml::{ConfigPathSegment, parse_config_path};

    // 1. Named rule / fact / template errors:
    //    - `building rule "id": ...`
    //    - `rule "id": ...` / `rule "id" (...): ...`
    //    - `fact "id": ...`
    //    - `template "id": ...`
    for (prefix, section) in [
        ("building rule \"", "rules"),
        ("rule \"", "rules"),
        ("fact \"", "facts"),
        ("template \"", "templates"),
    ] {
        if let Some(after) = body.strip_prefix(prefix)
            && let Some((id, tail)) = extract_quoted_id(after)
        {
            let mut segments = vec![
                ConfigPathSegment::Key(section.to_owned()),
                ConfigPathSegment::RuleId(id),
            ];
            append_rule_subkey(&mut segments, tail);
            return Some(segments);
        }
    }

    // 2. Backticked key path at the start of the error:
    //    - `` `relations.r.fields.name.at`: ... ``
    //    - `` `selectors.s.paths`: ... ``
    //    - `` `extends[0].as`: ... ``
    //    - `` `rules[3].input`: ... ``
    //    - `` `version:` ... ``
    if let Some(after) = body.strip_prefix('`')
        && let Some((raw_path, rest)) = after.split_once('`')
    {
        let cleaned = normalize_backtick_key(raw_path);
        if is_plausible_key_path(cleaned) {
            let mut segments = parse_config_path(cleaned);
            if let Some(unknown) = extract_unknown_field_name(rest) {
                segments.push(ConfigPathSegment::Key(unknown));
            }
            if !segments.is_empty() {
                return Some(segments);
            }
        }
    }

    // 3. Top-level unknown key/field without a parent prefix:
    //    - ``unknown key `relation` in a version 2 config...``
    //    - ``unknown field `bogus`, expected one of ...``
    if (body.starts_with("unknown key `") || body.starts_with("unknown field `"))
        && let Some(name) = extract_unknown_field_name(body)
    {
        return Some(vec![ConfigPathSegment::Key(name)]);
    }

    // 4. Unquoted colon-delimited path prefix from serde / ayaml / dsl:
    //    - `rules[3].paths: ...`
    //    - `vars.a: ...`
    //    - `facts[0].cmd: ...`
    //    - `limits.findings_per_rule: ...`
    if let Some((head, rest)) = body.split_once(": ")
        && is_plausible_key_path(head)
    {
        let mut segments = parse_config_path(head);
        if let Some(unknown) = extract_unknown_field_name(rest) {
            segments.push(ConfigPathSegment::Key(unknown));
        }
        if !segments.is_empty() {
            return Some(segments);
        }
    }

    // 5. Structured `limits` errors and narrowed top-level keyword fallbacks.
    extract_limit_key_path(body).or_else(|| fallback_top_level_key_path(body))
}

fn normalize_backtick_key(raw: &str) -> &str {
    let trimmed = raw.trim_end_matches(':');
    if let Some((before_colon, _)) = trimmed.split_once(": ") {
        before_colon.trim()
    } else {
        trimmed
    }
}

fn extract_limit_key_path(body: &str) -> Option<Vec<alint_core::yaml::ConfigPathSegment>> {
    use alint_core::yaml::{ConfigPathSegment, parse_config_path};
    let marker = "limit `";
    let at = body.find(marker)?;
    let before = &body[..at];
    if before.ends_with('_') || before.ends_with('-') {
        return None;
    }
    let rest = &body[at + marker.len()..];
    let (field, _) = rest.split_once('`')?;
    let mut segments = vec![ConfigPathSegment::Key("limits".to_owned())];
    if is_plausible_key_path(field) {
        segments.extend(parse_config_path(field));
    }
    Some(segments)
}

fn fallback_top_level_key_path(body: &str) -> Option<Vec<alint_core::yaml::ConfigPathSegment>> {
    use alint_core::yaml::ConfigPathSegment;

    if body.contains("baseline file") || body.contains("`baseline`") || body.contains("`baseline:`")
    {
        return Some(vec![ConfigPathSegment::Key("baseline".to_owned())]);
    }
    if body.contains("ignore pattern") || body.contains("`ignore`") || body.contains("`ignore:`") {
        return Some(vec![ConfigPathSegment::Key("ignore".to_owned())]);
    }
    if body.contains("fix_size_limit") {
        return Some(vec![ConfigPathSegment::Key("fix_size_limit".to_owned())]);
    }
    if body.contains("remote `extends`") || body.starts_with("resolving ") {
        return Some(vec![ConfigPathSegment::Key("extends".to_owned())]);
    }
    if body.contains("unsupported config version") || body.contains("config version") {
        return Some(vec![ConfigPathSegment::Key("version".to_owned())]);
    }
    if body.contains("requires alint ") || body.contains("requires.alint") {
        return Some(vec![
            ConfigPathSegment::Key("requires".to_owned()),
            ConfigPathSegment::Key("alint".to_owned()),
        ]);
    }
    if body.contains("`limits`") || body.contains("`limits:`") || body.starts_with("relation limit")
    {
        return Some(vec![ConfigPathSegment::Key("limits".to_owned())]);
    }
    None
}

fn extract_quoted_id(after_opening_quote: &str) -> Option<(String, &str)> {
    let mut escaped = false;
    let mut out = String::new();
    for (idx, ch) in after_opening_quote.char_indices() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return Some((out, &after_opening_quote[idx + 1..])),
            _ => out.push(ch),
        }
    }
    None
}

fn append_rule_subkey(segments: &mut Vec<alint_core::yaml::ConfigPathSegment>, tail: &str) {
    use alint_core::yaml::{ConfigPathSegment, parse_config_path};

    let tail = tail.trim_start();
    // e.g. `rule "tracked" (git_tracked_only) ...`, `rule "r" (custom:) ...`,
    // `rule "r" (fix: git_untrack) ...`, `rule "r" (require[0] kind: command) ...`.
    if let Some(after_paren) = tail.strip_prefix('(')
        && let Some((inside, _)) = after_paren.split_once(')')
        && append_paren_subkey(segments, inside.trim())
    {
        return;
    }
    let tail = tail.strip_prefix(':').unwrap_or(tail).trim_start();
    // `build_entries` wraps `Error::RuleConfig` as
    // `building rule "id": rule "id": ...`; strip the repeated inner prefix so
    // the actual option key or wrapper phrase is visible below.
    let tail = strip_repeated_named_prefix(tail);
    if tail.contains("parsing `when`") {
        segments.push(ConfigPathSegment::Key("when".to_owned()));
        return;
    }
    if let Some(unknown) = extract_unknown_field_name(tail) {
        segments.push(ConfigPathSegment::Key(unknown));
        return;
    }
    if tail.contains("unknown rule kind") || tail.contains("unknown kind") {
        segments.push(ConfigPathSegment::Key("kind".to_owned()));
        return;
    }
    // Unwrap wrapper phrases like `could not deserialize merged rule: level: ...`
    // or `invalid options: YAML parse error: pattern: ...`.
    let inner = tail
        .strip_prefix("could not deserialize merged rule: ")
        .or_else(|| tail.strip_prefix("invalid options: YAML parse error: "))
        .or_else(|| tail.strip_prefix("invalid options: "))
        .unwrap_or(tail);
    if let Some(after_bt) = inner.strip_prefix('`')
        && let Some((raw, _)) = after_bt.split_once('`')
    {
        let field = normalize_backtick_key(raw);
        if is_plausible_key_path(field) {
            segments.extend(parse_config_path(field));
            return;
        }
    }
    if let Some((field, _)) = inner.split_once(": ")
        && is_plausible_key_path(field)
    {
        segments.extend(parse_config_path(field));
    }
}

fn append_paren_subkey(
    segments: &mut Vec<alint_core::yaml::ConfigPathSegment>,
    inside: &str,
) -> bool {
    use alint_core::yaml::parse_config_path;

    // Handle `require[0] kind: command` / `require[0] fix: git_untrack`.
    if let Some((first, rest)) = inside.split_once(' ')
        && first.starts_with("require[")
        && is_plausible_key_path(first)
    {
        segments.extend(parse_config_path(first));
        let sub = rest
            .split_once(": ")
            .map_or(rest, |(head, _)| head)
            .trim_end_matches(':')
            .trim();
        if is_plausible_key_path(sub) {
            segments.extend(parse_config_path(sub));
        }
        return true;
    }
    let field = inside
        .split_once(": ")
        .map_or(inside, |(head, _)| head)
        .trim_end_matches(':')
        .trim();
    if is_plausible_key_path(field) {
        segments.extend(parse_config_path(field));
        return true;
    }
    false
}

fn strip_repeated_named_prefix(tail: &str) -> &str {
    for prefix in ["rule \"", "fact \"", "template \""] {
        if let Some(after) = tail.strip_prefix(prefix)
            && let Some((_, rest)) = extract_quoted_id(after)
        {
            return rest
                .trim_start()
                .strip_prefix(':')
                .unwrap_or(rest)
                .trim_start();
        }
    }
    tail
}

fn extract_unknown_field_name(text: &str) -> Option<String> {
    for marker in ["unknown key `", "unknown field `"] {
        if let Some(at) = text.find(marker) {
            let rest = &text[at + marker.len()..];
            if let Some(end) = rest.find('`') {
                let name = &rest[..end];
                if !name.is_empty() {
                    return Some(name.to_owned());
                }
            }
        }
    }
    None
}

fn is_plausible_key_path(token: &str) -> bool {
    if token.is_empty() || token.contains(char::is_whitespace) {
        return false;
    }
    let first_segment = token.split(['.', '[']).next().unwrap_or_default();
    !first_segment.is_empty()
        && first_segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn extract_line_column(text: &str) -> Option<(usize, usize)> {
    let at = text.rfind("at line ")?;
    let rest = &text[at + "at line ".len()..];
    let (line_str, after_line) = rest.split_once(" column ")?;
    let line = line_str.parse::<usize>().ok()?;
    let col_str: String = after_line
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let col = col_str.parse::<usize>().ok()?;
    Some((line, col))
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
    let mut load_options = alint_dsl::LoadOptions::with_catalog(registry.catalog())
        .with_global_git_excludes(!cfg!(test));
    if !trusted {
        load_options.environment = environment.clone();
        load_options.remote_policy = alint_dsl::RemotePolicy::Deny;
        load_options.out_of_root_policy = alint_dsl::OutOfRootPolicy::Deny;
    }
    let config = alint_dsl::load_with(config_path, &load_options).map_err(|error| {
        let raw_message = error.to_string();
        let untrusted = !trusted
            && (environment.asked.load(Ordering::Relaxed)
                || raw_message.contains("remote `extends` is disabled")
                || raw_message.contains("an untrusted load"));
        let range = config_error_range(config_path, &raw_message);
        let mut message = format!("loading config: {raw_message}");
        if untrusted {
            message.push_str("\n\n");
            message.push_str(UNTRUSTED_NOTE);
        }
        ConfigError {
            message,
            range,
            untrusted,
        }
    })?;
    if settings.no_spawn {
        let enabled = config
            .rules
            .iter()
            .filter(|spec| !matches!(spec.level, Level::Off));
        alint_dsl::refuse_spawning(enabled, &config.facts, registry.catalog(), "--no-spawn")
            .map_err(|error| {
                let redactor =
                    alint_core::EnvironmentRedactor::from_values(&config.environment_values);
                let message = redactor
                    .redact(&format!("loading config: {error}"))
                    .into_owned();
                let range = config_error_range(config_path, &message);
                ConfigError {
                    message,
                    range,
                    untrusted: false,
                }
            })?;
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
fn session_config_error(config_path: &Path, message: String) -> ConfigError {
    let range = config_error_range(config_path, &message);
    ConfigError {
        message,
        range,
        untrusted: false,
    }
}

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
    let environment_redactor =
        alint_core::EnvironmentRedactor::from_values(&config.environment_values);
    let redact = |message: String| environment_redactor.redact(&message).into_owned();
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
    } = build_entries(&config.rules, &config.allow_out_of_root, trusted, registry).map_err(
        |mut error| {
            error.message = redact(error.message);
            if error.range.is_none() {
                error.range = config_error_range(config_path, &error.message);
            }
            error
        },
    )?;
    withheld |= rules_withheld;

    // A fact's id may be read from the environment.
    let pattern_hints: Vec<_> = entries
        .iter()
        .flat_map(|entry| entry.pattern_hints(registry))
        .chain(alint_core::pattern_hints::environment_fact_hints_with(
            &facts,
            &environment_redactor,
        ))
        .map(|mut hint| {
            hint.id = redact(hint.id);
            hint
        })
        .collect();
    let mut engine = Engine::from_entries(entries, registry.clone())
        .with_facts(facts)
        .with_vars(config.vars)
        .with_limits(&config.limits)
        .map_err(|error| {
            session_config_error(config_path, redact(format!("loading config: {error}")))
        })?
        .with_fix_size_limit(config.fix_size_limit)
        .with_config_files(config.config_files.clone())
        .with_environment_config_files(config.environment_config_files.clone());
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
        Some(path) => Some(
            Baseline::load_file_redacted(
                path,
                Some(&effective_root),
                config.baseline_from_environment,
            )
            .map_err(|error| session_config_error(config_path, redact(error)))?,
        ),
        None => None,
    };
    let mut extra_ignores = config.ignore;
    if let Some(path) = &baseline_path
        && let Some(pattern) = alint_core::baseline::walk_exclude(&effective_root, path)
    {
        extra_ignores.push(pattern);
    }

    // This crate's unit tests walk without the host's global excludes file
    // (CONTRIBUTING.md), as they load without it.
    let walk_opts = WalkOptions::default()
        .with_respect_gitignore(config.respect_gitignore && !settings.no_gitignore)
        .with_global_git_excludes(!cfg!(test))
        .with_extra_ignores(extra_ignores);
    let index = walk(&effective_root, &walk_opts).map_err(|e| {
        session_config_error(config_path, redact(format!("walking repository: {e}")))
    })?;

    Ok(Session {
        root: effective_root,
        engine,
        index,
        config_path: config_path.to_path_buf(),
        baseline,
        kinds,
        pattern_hints,
        environment_redactor,
        withheld,
        extended_rules,
        config_files: config.config_files,
        environment_config_files: config.environment_config_files,
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
                        path_free_identity: false,
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
            paths: Vec::new(),
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
        // One byte past the cap, so a regression that read it whole takes
        // 256 MiB of this process, not the host's memory.
        std::fs::File::create(root.join("sparse.jsonl"))
            .unwrap()
            .set_len((256 << 20) + 1)
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
            // On another thread, with a deadline: a regression that opened
            // the FIFO would block, and fails here instead of hanging.
            let (sender, receiver) = std::sync::mpsc::channel();
            let at = root.clone();
            std::thread::spawn(move || {
                drop(sender.send(build_session(&at).err().unwrap_or_default()));
            });
            let error = receiver
                .recv_timeout(std::time::Duration::from_secs(60))
                .unwrap_or_else(|_| panic!("{baseline}: no answer within the deadline"));
            assert!(error.contains(refusal), "{baseline}: {error}");
        }
        config("base.jsonl");
        assert!(build_session(&root).unwrap().unwrap().baseline.is_some());
    }
}

#[cfg(test)]
mod note_tests {
    use super::*;

    /// A `constraint` note's message starts with its file's path, escaped;
    /// the note shows that path once, escaped as the message names it, not
    /// the raw path and then the message's escaped one, and `/`-separated
    /// on every platform (a note's path is the resource's). The file's name
    /// holds an escape sequence and a line break (where the filesystem
    /// allows them), a combining accent and a right-to-left override.
    #[test]
    fn a_note_names_its_file_once_and_escaped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join(".alint.yml"),
            "version: 2\nrelations:\n  r:\n    from: { paths: \"d/*.json\" }\n    read: { json: \
             \"$.v\" }\n    fields: { v: value }\nrules:\n  - id: n\n    kind: constraint\n    \
             input: r\n    assert: { unique: { key: [v] } }\n    level: error\n",
        )
        .unwrap();
        std::fs::create_dir(root.join("d")).unwrap();
        let (name, shown) = if cfg!(unix) {
            (
                "a\u{1b}[31m\ne\u{301}\u{202e}Z2.json",
                "a\\u{1b}[31m\\ne\\u{301}\\u{202e}Z2.json",
            )
        } else {
            ("ae\u{301}\u{202e}Z2.json", "ae\\u{301}\\u{202e}Z2.json")
        };
        std::fs::write(root.join("d").join(name), "{\"v\": \"${VAR}\"}").unwrap();
        let check = check_workspace(
            &[root.to_path_buf()],
            &[],
            &Settings::default(),
            &alint_rules::builtin_registry(),
        );
        let notes: Vec<&String> = check
            .runs
            .iter()
            .flat_map(|(_, run)| &run.as_ref().unwrap().notes)
            .collect();
        assert_eq!(
            notes,
            [&format!(
                "d/{shown}: skipped non-literal value \"${{VAR}}\""
            )]
        );
    }

    /// A note of a rule whose relation keeps its rows by a value from the
    /// environment says where that value occurs, so its path is hidden, as
    /// `alint check --show-notes` hides it: `<redacted>`, not the file.
    #[test]
    fn a_note_of_a_rule_whose_rows_the_environment_picks_hides_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join(".alint.yml"),
            "version: 2\nrelations:\n  r:\n    from: { paths: \"d/*.json\" }\n    read: { json: \
             \"$.v\" }\n    fields: { v: value, file: path }\n    where: { file: { in: \
             [\"{{env.ALINT_LSP_NOTE_TARGET | default('d/secret-falcon.json')}}\"] } \
             }\nrules:\n  - id: n\n    kind: constraint\n    input: r\n    assert: { unique: \
             { key: [v] } }\n    level: error\n",
        )
        .unwrap();
        std::fs::create_dir(root.join("d")).unwrap();
        std::fs::write(root.join("d/secret-falcon.json"), "{\"v\": \"${VAR}\"}").unwrap();
        let check = check_workspace(
            &[root.to_path_buf()],
            &[],
            &Settings::default(),
            &alint_rules::builtin_registry(),
        );
        let notes: Vec<&String> = check
            .runs
            .iter()
            .flat_map(|(_, run)| &run.as_ref().unwrap().notes)
            .collect();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("<redacted>: "), "{notes:?}");
        assert!(!notes[0].contains("falcon"), "{notes:?}");
    }

    #[test]
    fn no_spawn_custom_fact_error_redacts_environment_value() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let secret = std::env::var("CARGO_PKG_NAME").expect("cargo sets CARGO_PKG_NAME");
        std::fs::write(
            root.join(".alint.yml"),
            "version: 1\nfacts:\n  - id: '{{env.CARGO_PKG_NAME}}'\n    \
             custom:\n      argv: ['echo', 'hi']\nrules: []\n",
        )
        .unwrap();
        let settings = Settings {
            trusted: true,
            no_spawn: true,
            ..Settings::default()
        };
        let err = build_session_with(root, &settings).unwrap_err();
        assert!(err.contains("--no-spawn"), "{err}");
        assert!(err.contains(alint_core::CONFIGURATION_MASK), "{err}");
        assert!(!err.contains(&secret), "env value leaked: {err}");
    }

    #[test]
    fn lsp_session_honors_top_level_limits() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join(".alint.yml"),
            "version: 1\nlimits:\n  findings_per_rule: 2\nrules:\n  - id: kp\n    \
             kind: key_parity\n    reference: en.json\n    peers: fr.json\n    level: error\n",
        )
        .unwrap();
        std::fs::write(root.join("en.json"), r#"{"a":"1","b":"2","c":"3","d":"4"}"#).unwrap();
        std::fs::write(root.join("fr.json"), "{}").unwrap();
        let check = check_workspace(
            &[root.to_path_buf()],
            &[],
            &Settings {
                trusted: true,
                ..Settings::default()
            },
            &alint_rules::builtin_registry(),
        );
        let (_, run) = &check.runs[0];
        let run = run.as_ref().unwrap();
        let total: usize = run.by_path.values().map(Vec::len).sum();
        // 2 capped findings + 1 truncation analysis failure = 3 diagnostics.
        assert_eq!(total, 3, "diagnostics: {:?}", run.by_path);
    }

    #[test]
    fn config_load_errors_place_diagnostics_at_their_key_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let cfg = root.join(".alint.yml");
        let settings = Settings {
            trusted: true,
            ..Settings::default()
        };
        let registry = alint_rules::builtin_registry();
        let run_err = |yaml: &str| -> ConfigError {
            std::fs::write(&cfg, yaml).unwrap();
            let key = SessionKey {
                config: cfg.clone(),
                root: root.to_path_buf(),
            };
            build_session_for_config(&key, &settings, &registry).unwrap_err()
        };

        // 1. Unknown top-level key in version 2 points at `relation` (line 2, col 0..8).
        let err = run_err("version: 2\nrelation: {}\nrules: []\n");
        assert_eq!(
            err.range,
            Some(Range::new(Position::new(1, 0), Position::new(1, 8))),
            "v2 unknown key: {err:?}"
        );

        // 2. Version 2 selector key path (`selectors.s.paths`) highlights `bogus` on line 5.
        let err = run_err("version: 2\nselectors:\n  s:\n    paths:\n      bogus: 1\nrules: []\n");
        assert_eq!(
            err.range,
            Some(Range::new(Position::new(4, 6), Position::new(4, 11))),
            "v2 selector unknown key: {err:?}"
        );

        // 3. Version 2 relation key path (`relations.r.entries`) highlights `depth` on line 6.
        let err = run_err(
            "version: 2\nrelations:\n  r:\n    from: { paths: '*.yml' }\n    entries:\n      \
             depth: 1\nrules: []\n",
        );
        assert_eq!(
            err.range,
            Some(Range::new(Position::new(5, 6), Position::new(5, 11))),
            "v2 relation unknown key: {err:?}"
        );

        // 4. Version 2 `extends[0].as` highlights `as` on line 4.
        let err = run_err("version: 2\nextends:\n  - url: ./base.yml\n    as: [bad]\nrules: []\n");
        assert_eq!(
            err.range,
            Some(Range::new(Position::new(3, 4), Position::new(3, 6))),
            "v2 extends[0].as: {err:?}"
        );

        // 5. Rule build error (unknown rule kind) points at `kind` of that rule (line 7).
        let err = run_err(
            "version: 1\nrules:\n  - id: ok\n    kind: file_exists\n    paths: README.md\n    \
             level: error\n  - id: broken\n    kind: no_such_kind\n    paths: a\n    level: \
             error\n",
        );
        assert_eq!(
            err.range,
            Some(Range::new(Position::new(7, 4), Position::new(7, 8))),
            "unknown rule kind: {err:?}"
        );

        // 6. Rule `when:` parse error points at `when` inside that rule (line 6).
        let err = run_err(
            "version: 1\nrules:\n  - id: bad-when\n    kind: file_exists\n    paths: a\n    \
             level: error\n    when: '(((broken'\n",
        );
        assert_eq!(
            err.range,
            Some(Range::new(Position::new(6, 4), Position::new(6, 8))),
            "bad when: {err:?}"
        );

        // 7. Duplicate key in `vars:` points at the repeated key (`a` on line 5 col 2).
        let err = run_err("version: 1\nvars:\n  a: one\n  b: two\n  a: three\nrules: []\n");
        assert_eq!(
            err.range,
            Some(Range::new(Position::new(4, 2), Position::new(4, 3))),
            "duplicate vars key: {err:?}"
        );

        // 8. UTF-16 surrogate pair before key in flow mapping converts columns in UTF-16 units.
        //    `vars: {😀: 1, ` -> 7 ASCII + 2 UTF-16 units for 😀 + 5 ASCII = col 14.
        let err = run_err("version: 1\nvars: {😀: 1, 😀: 2}\nrules: []\n");
        assert_eq!(
            err.range,
            Some(Range::new(Position::new(1, 14), Position::new(1, 16))),
            "UTF-16 surrogate pair column: {err:?}"
        );

        // 9. Error originating from an `extends:` target falls back to `None` (file start).
        std::fs::write(
            root.join("base.yml"),
            "version: 2\nselectors:\n  s:\n    bad: 1\nrules: []\n",
        )
        .unwrap();
        let err = run_err("version: 2\nextends:\n  - ./base.yml\nrules: []\n");
        assert_eq!(
            err.range, None,
            "extends child error must fall back to file start: {err:?}"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn config_error_range_handles_edge_cases_and_foreign_paths() {
        let cfg = Path::new("/repo/.alint.yml");

        // 1. Full multi-char duplicate key span (not just 1-char start).
        let dup_yaml = "version: 1\nvars:\n  duplicate_key: 1\n  duplicate_key: 2\nrules: []\n";
        assert_eq!(
            config_error_range_in_text(
                cfg,
                dup_yaml,
                "loading config: /repo/.alint.yml: YAML parse error: vars: duplicate entry with key \"duplicate_key\" at line 4 column 3",
            ),
            Some(Range::new(Position::new(3, 2), Position::new(3, 15)))
        );

        // 2. Leading UTF-8 BOM is stripped before span/column computation.
        let bom_yaml = "\u{feff}version: 2\nbogus: 1\nrules: []\n";
        assert_eq!(
            config_error_range_in_text(
                cfg,
                bom_yaml,
                "loading config: /repo/.alint.yml: unknown key `bogus` in a version 2 config at line 2 column 1",
            ),
            Some(Range::new(Position::new(1, 0), Position::new(1, 5)))
        );

        // 3. Foreign `<redacted>:` and `nested config ...` and non-yaml sibling files return `None`.
        let valid_yaml = "version: 1\nbaseline: base.jsonl\nignore:\n  - dist\nrules: []\n";
        assert_eq!(
            config_error_range_in_text(
                cfg,
                valid_yaml,
                "loading config: <redacted>: unknown key `bogus`"
            ),
            None
        );
        assert_eq!(
            config_error_range_in_text(
                cfg,
                valid_yaml,
                "loading config: nested config sub/.alint.yml: unknown key `bogus`"
            ),
            None
        );
        assert_eq!(
            config_error_range_in_text(
                cfg,
                valid_yaml,
                "loading config: base.txt: unknown field `bogus`"
            ),
            None
        );

        // 4. Own-config messages with paths inside them (`reading baseline file /repo/...`,
        //    `` `extends:` entry ... ``) are NOT rejected as foreign file prefixes.
        assert_eq!(
            config_error_range_in_text(
                cfg,
                valid_yaml,
                "reading baseline file /repo/base.jsonl: No such file or directory",
            ),
            Some(Range::new(Position::new(1, 0), Position::new(1, 8)))
        );
        let extends_yaml = "version: 2\nextends:\n  - url: ./base.yml\nrules: []\n";
        assert_eq!(
            config_error_range_in_text(
                cfg,
                extends_yaml,
                "loading config: `extends:` entry ./base.yml: unknown filter",
            ),
            Some(Range::new(Position::new(1, 0), Position::new(1, 7)))
        );

        // 5. `walking repository: ... .gitignore` must NOT falsely highlight `ignore:`.
        assert_eq!(
            config_error_range_in_text(
                cfg,
                valid_yaml,
                "walking repository: failed to read .gitignore: permission denied",
            ),
            None
        );

        // 6. Double-prefixed rule option error (`building rule "id": rule "id": invalid options: ...`)
        //    reaches the nested option key (`pattern`).
        let rule_yaml = "version: 1\nrules:\n  - id: bad-re\n    kind: regex_forbidden\n    pattern: '['\n    level: error\n";
        assert_eq!(
            config_error_range_in_text(
                cfg,
                rule_yaml,
                "building rule \"bad-re\": rule \"bad-re\": invalid options: pattern: invalid regex",
            ),
            Some(Range::new(Position::new(4, 4), Position::new(4, 11)))
        );

        // 7. `--no-spawn` parenthesized subkeys (`(custom:)`, `(fix: ...)`, `(require[0] kind: ...)`).
        let spawn_yaml = "version: 1\nrules:\n  - id: r1\n    kind: command\n    custom: echo hi\n    fix:\n      command: echo\n    require:\n      - kind: command\n    level: error\n";
        assert_eq!(
            config_error_range_in_text(
                cfg,
                spawn_yaml,
                "loading config: rule \"r1\" (custom:) spawns a process"
            ),
            Some(Range::new(Position::new(4, 4), Position::new(4, 10)))
        );
        assert_eq!(
            config_error_range_in_text(
                cfg,
                spawn_yaml,
                "loading config: rule \"r1\" (fix: command) spawns a process"
            ),
            Some(Range::new(Position::new(5, 4), Position::new(5, 7)))
        );
        assert_eq!(
            config_error_range_in_text(
                cfg,
                spawn_yaml,
                "loading config: rule \"r1\" (require[0] kind: command) spawns a process"
            ),
            Some(Range::new(Position::new(8, 8), Position::new(8, 12)))
        );

        // 8. `LimitConfigError` sub-field highlights `limits.<field>`, and lone `\r` lines match
        //    `physical_lines` indexing.
        let limits_yaml = "version: 1\nlimits:\n  findings_per_rule: 0\nrules: []\n";
        assert_eq!(
            config_error_range_in_text(
                cfg,
                limits_yaml,
                "loading config: relation limit `findings_per_rule` must be >= 1",
            ),
            Some(Range::new(Position::new(2, 2), Position::new(2, 19)))
        );
        let cr_yaml = "version: 1\rbad: 1\rrules: []\r";
        assert_eq!(
            config_error_range_in_text(
                cfg,
                cr_yaml,
                "loading config: /repo/.alint.yml: syntax error at line 2 column 1"
            ),
            Some(Range::new(Position::new(1, 0), Position::new(1, 1)))
        );
    }
}
