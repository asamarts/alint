//! Language Server Protocol server for alint.
//!
//! A thin `tower-lsp` backend that runs the alint engine over the
//! workspace and publishes the resulting violations as LSP diagnostics.
//! It is driven by the `alint lsp` subcommand, speaking LSP over stdio
//! (see [`run_stdio`]).
//!
//! Evaluation paths:
//!
//! - **Open / save** run the full [`alint_core::Engine`] over the
//!   workspace (cross-file rules included) and publish per-file
//!   diagnostics for every open document.
//! - **Change** uses the single-file hot path
//!   ([`alint_core::Engine::run_for_file`]) against the editor's
//!   in-memory bytes, so per-keystroke feedback costs one file's
//!   evaluation, not the whole tree's. Cross-file rules are not
//!   re-run on change (they refresh on the next save), matching
//!   `docs/design/v0.11/single_file_reevaluation.md`.
//! - **Hover** over a violation marker renders the rule id, message,
//!   and `policy_url` from the per-file cache of the last-published
//!   findings.
//! - **Code actions** offer an "Apply fix" quick-fix for any violation
//!   whose rule declares a fixer, returning a `WorkspaceEdit`
//!   ([`alint_core::Fixer::fix_edit`] → [`alint_core::FixEdit`]) the
//!   editor applies to the buffer.
//! - **Watched files** (`didChangeWatchedFiles`) reload the session, so
//!   `.alint.yml` edits take effect without saving an open document.
//!
//! The "add rule to ignore" action is deferred to a later slice of the
//! LSP epic.

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CodeDescription, CreateFile, DeleteFile,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentChangeOperation, DocumentChanges, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams, MarkupContent,
    MarkupKind, MessageType, NumberOrString, OneOf, OptionalVersionedTextDocumentIdentifier,
    Position, Range, RenameFile, ResourceOp, ServerCapabilities, ServerInfo, TextDocumentEdit,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url, WorkspaceEdit,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, jsonrpc::Result as JsonRpcResult};

use alint_core::{
    Engine, Error, FileIndex, FixEdit, Level, RuleEntry, RuleResult, Violation, WalkOptions, walk,
};

/// One cached finding for a file: enough to publish a diagnostic and to
/// render a hover. Kept per URI in [`State::diagnostics`] so `hover`
/// can answer from the last-published set without re-running rules.
#[derive(Debug, Clone)]
struct Finding {
    range: Range,
    severity: DiagnosticSeverity,
    rule_id: String,
    message: String,
    /// The violation's original 1-indexed line/column (if any), kept so
    /// a code-action fixer that is range-scoped sees the same location
    /// the rule reported — `range` alone can't distinguish a real
    /// (1,1) anchor from a path-less finding anchored at the file start.
    line: Option<usize>,
    column: Option<usize>,
    policy_url: Option<String>,
    /// Whether the rule declares a fixer — gates the "Apply fix" code
    /// action without re-deriving the rule set.
    fixable: bool,
    /// Whether this came from a per-file rule (re-evaluated on every
    /// edit) vs a cross-file rule (only on save). On `didChange` we keep
    /// the cached cross-file findings and replace only the per-file ones,
    /// so cross-file markers don't flicker away while typing.
    per_file: bool,
}

/// Per-file findings keyed by absolute path.
type FindingsByPath = HashMap<PathBuf, Vec<Finding>>;

/// A loaded workspace: the config-built engine plus the walked index.
/// Cached on open/save and reused by the change hot path so a keystroke
/// doesn't re-load the config or re-walk the tree.
#[derive(Debug)]
struct Session {
    root: PathBuf,
    engine: Engine,
    index: FileIndex,
    /// The discovered `.alint.yml` (relative to root). Used to anchor
    /// path-less findings and config errors as diagnostics.
    config_path: PathBuf,
}

/// A failure building the session, carrying the config file (if known)
/// so the server can surface it as a diagnostic on `.alint.yml`.
#[derive(Debug)]
struct BuildError {
    config_path: Option<PathBuf>,
    message: String,
}

/// Build a tokio runtime and serve the alint language server over
/// stdio until the client disconnects. Called by the `alint lsp`
/// subcommand so the CLI itself stays synchronous.
pub fn run_stdio() -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = LspService::new(Backend::new);
        Server::new(stdin, stdout, socket).serve(service).await;
    });
    Ok(())
}

#[derive(Debug)]
struct State {
    /// Workspace root, from the `initialize` handshake.
    root: Option<PathBuf>,
    /// URIs of documents the editor currently has open. Diagnostics
    /// are published (and cleared) for these.
    open: HashSet<Url>,
    /// Cached engine + index from the last full check. `None` until the
    /// first open/save; the change hot path needs it.
    session: Option<Arc<Session>>,
    /// Last-published findings per open URI, so `hover` can answer by
    /// position without re-running rules.
    diagnostics: HashMap<Url, Vec<Finding>>,
    /// In-memory text per open URI (the editor's authoritative buffer),
    /// so `codeAction` can compute a fix edit against unsaved content.
    documents: HashMap<Url, String>,
}

#[derive(Debug)]
struct Backend {
    client: Client,
    state: Mutex<State>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            state: Mutex::new(State {
                root: None,
                open: HashSet::new(),
                session: None,
                diagnostics: HashMap::new(),
                documents: HashMap::new(),
            }),
        }
    }

    /// Full check: (re)build the session and publish per-file
    /// diagnostics for every open document, clearing those that no
    /// longer have findings. Runs on open and save.
    async fn check_and_publish(&self) {
        let (root, open) = {
            let state = self.state.lock();
            (
                state.root.clone(),
                state.open.iter().cloned().collect::<Vec<_>>(),
            )
        };
        let Some(root) = root else {
            return;
        };

        match tokio::task::spawn_blocking(move || build_and_run(&root)).await {
            Ok(Ok(Some((session, by_path)))) => {
                let config_uri = Url::from_file_path(&session.config_path).ok();
                let to_publish = {
                    let mut state = self.state.lock();
                    state.session = Some(session);
                    cache_and_collect(&mut state, &open, &by_path)
                };
                self.publish_all(to_publish).await;
                // Clear any stale "config error" diagnostic now that the
                // config loaded cleanly.
                if let Some(uri) = config_uri {
                    self.client.publish_diagnostics(uri, Vec::new(), None).await;
                }
            }
            Ok(Ok(None)) => {
                // No `.alint.yml` — clear any stale diagnostics.
                let to_publish = {
                    let mut state = self.state.lock();
                    state.session = None;
                    cache_and_collect(&mut state, &open, &FindingsByPath::new())
                };
                self.publish_all(to_publish).await;
            }
            Ok(Err(build_err)) => {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("alint: {}", build_err.message),
                    )
                    .await;
                // Surface a malformed/unbuildable config as a diagnostic
                // on `.alint.yml` so it's visible, not just logged.
                if let Some(uri) = build_err
                    .config_path
                    .as_ref()
                    .and_then(|p| Url::from_file_path(p).ok())
                {
                    let diagnostic = Diagnostic {
                        range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some("alint".to_string()),
                        message: build_err.message,
                        ..Diagnostic::default()
                    };
                    self.client
                        .publish_diagnostics(uri, vec![diagnostic], None)
                        .await;
                }
            }
            Err(join_err) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("alint: check panicked: {join_err}"),
                    )
                    .await;
            }
        }
    }

    /// Single-file hot path: re-evaluate per-file rules against the
    /// editor's in-memory `text` and publish diagnostics for just this
    /// document. Per-file findings replace the previous per-file ones,
    /// but cached cross-file findings (from the last full run) are
    /// preserved so they don't flicker away while typing — they refresh
    /// on the next save. `version` ties the diagnostics to the edit.
    async fn reeval_file(&self, uri: Url, text: String, version: i32) {
        let session = self.state.lock().session.clone();
        let Some(session) = session else {
            return; // No cached session yet — open/save will populate it.
        };
        let Ok(abs) = uri.to_file_path() else {
            return;
        };
        let Ok(rel) = abs.strip_prefix(&session.root).map(Path::to_path_buf) else {
            return;
        };

        let abs_key = abs.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            session
                .engine
                .run_for_file(&session.root, &session.index, &rel, text.as_bytes())
                .map(|results| {
                    group_findings(
                        &session.root,
                        &results,
                        &session.engine,
                        &session.config_path,
                    )
                })
        })
        .await;

        match outcome {
            Ok(Ok(by_path)) => {
                let per_file = by_path.get(&abs_key).cloned().unwrap_or_default();
                let diagnostics = {
                    let mut state = self.state.lock();
                    // Keep cross-file findings from the last full run;
                    // replace the per-file ones with the fresh results.
                    let mut merged: Vec<Finding> = state
                        .diagnostics
                        .get(&uri)
                        .map(|prev| prev.iter().filter(|f| !f.per_file).cloned().collect())
                        .unwrap_or_default();
                    merged.extend(per_file);
                    let diags: Vec<Diagnostic> = merged.iter().map(finding_to_diagnostic).collect();
                    state.diagnostics.insert(uri.clone(), merged);
                    diags
                };
                self.client
                    .publish_diagnostics(uri, diagnostics, Some(version))
                    .await;
            }
            Ok(Err(Error::FileNotInIndex { .. })) => {
                // Excluded from linting (or not yet walked) — clear.
                self.state.lock().diagnostics.remove(&uri);
                self.client
                    .publish_diagnostics(uri, Vec::new(), Some(version))
                    .await;
            }
            Ok(Err(err)) => {
                self.client
                    .log_message(MessageType::WARNING, format!("alint: {err}"))
                    .await;
            }
            Err(join_err) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("alint: re-eval panicked: {join_err}"),
                    )
                    .await;
            }
        }
    }

    async fn publish_all(&self, items: Vec<(Url, Vec<Diagnostic>)>) {
        for (uri, diagnostics) in items {
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> JsonRpcResult<InitializeResult> {
        if let Some(root) = workspace_root(&params) {
            self.state.lock().root = Some(root);
        }
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "alint-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "alint language server ready")
            .await;
    }

    async fn shutdown(&self) -> JsonRpcResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        {
            let mut state = self.state.lock();
            state.open.insert(params.text_document.uri.clone());
            state
                .documents
                .insert(params.text_document.uri, params.text_document.text);
        }
        self.check_and_publish().await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        // FULL document sync → the last content change carries the
        // whole new text. Re-evaluate per-file rules against it.
        let Some(change) = params.content_changes.pop() else {
            return;
        };
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        self.state
            .lock()
            .documents
            .insert(uri.clone(), change.text.clone());
        self.reeval_file(uri, change.text, version).await;
    }

    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        // Rebuild the session (the tree / config may have changed) and
        // re-run everything, including cross-file rules.
        self.check_and_publish().await;
    }

    async fn did_change_watched_files(&self, _: DidChangeWatchedFilesParams) {
        // A watched file changed outside the editor's edit flow — most
        // importantly `.alint.yml`. Rebuild the session and re-run so
        // config edits take effect without needing to save an open doc.
        self.check_and_publish().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        {
            let mut state = self.state.lock();
            state.open.remove(&uri);
            state.diagnostics.remove(&uri);
            state.documents.remove(&uri);
        }
        // Clear any diagnostics the editor is still showing.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn hover(&self, params: HoverParams) -> JsonRpcResult<Option<Hover>> {
        let pos = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;

        let findings = {
            let state = self.state.lock();
            state.diagnostics.get(&uri).cloned()
        };
        let Some(findings) = findings else {
            return Ok(None);
        };
        let matching: Vec<&Finding> = findings
            .iter()
            .filter(|f| range_contains(f.range, pos))
            .collect();
        if matching.is_empty() {
            return Ok(None);
        }

        let value = matching
            .iter()
            .map(|f| render_finding(f))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: matching.first().map(|f| f.range),
        }))
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> JsonRpcResult<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let selection = params.range;

        // Respect the client's kind filter: if it asked for a specific
        // set of action kinds that doesn't admit quick-fixes, return none.
        if let Some(only) = &params.context.only {
            let admits_quickfix = only
                .iter()
                .any(|kind| *kind == CodeActionKind::QUICKFIX || *kind == CodeActionKind::EMPTY);
            if !admits_quickfix {
                return Ok(None);
            }
        }

        let (session, findings, text) = {
            let state = self.state.lock();
            (
                state.session.clone(),
                state.diagnostics.get(&uri).cloned(),
                state.documents.get(&uri).cloned(),
            )
        };
        let (Some(session), Some(findings), Some(text)) = (session, findings, text) else {
            return Ok(None);
        };
        let Ok(abs) = uri.to_file_path() else {
            return Ok(None);
        };
        let Ok(rel) = abs.strip_prefix(&session.root).map(Path::to_path_buf) else {
            return Ok(None);
        };

        let bytes = text.as_bytes();
        let mut actions: CodeActionResponse = Vec::new();
        for finding in &findings {
            if !finding.fixable || !ranges_overlap(finding.range, selection) {
                continue;
            }
            let Some(fixer) = session.engine.fixer_for(&finding.rule_id) else {
                continue;
            };
            let mut violation = Violation::new(finding.message.clone()).with_path(rel.clone());
            // Preserve the reported location so range-scoped fixers act on
            // the right line/column (not just whole-file fixers).
            violation.line = finding.line;
            violation.column = finding.column;
            let Some(edit) = fixer.fix_edit(&violation, bytes, &session.root) else {
                continue;
            };
            let Some(workspace_edit) = fix_edit_to_workspace_edit(&edit, &session.root) else {
                continue;
            };
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("alint: fix `{}`", finding.rule_id),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![finding_to_diagnostic(finding)]),
                edit: Some(workspace_edit),
                ..CodeAction::default()
            }));
        }
        if actions.is_empty() {
            return Ok(None);
        }
        // A single fix is the obvious one to apply.
        if actions.len() == 1 {
            if let CodeActionOrCommand::CodeAction(action) = &mut actions[0] {
                action.is_preferred = Some(true);
            }
        }
        Ok(Some(actions))
    }
}

/// Cache the findings for each open document and collect the
/// `(uri, diagnostics)` pairs to publish. Documents absent from
/// `by_path` are cached empty and cleared.
fn cache_and_collect(
    state: &mut State,
    open: &[Url],
    by_path: &FindingsByPath,
) -> Vec<(Url, Vec<Diagnostic>)> {
    let mut out = Vec::with_capacity(open.len());
    for uri in open {
        let findings = uri
            .to_file_path()
            .ok()
            .and_then(|abs| by_path.get(&abs).cloned())
            .unwrap_or_default();
        let diagnostics = findings.iter().map(finding_to_diagnostic).collect();
        state.diagnostics.insert(uri.clone(), findings);
        out.push((uri.clone(), diagnostics));
    }
    out
}

/// Resolve the workspace root from the `initialize` params, preferring
/// the first workspace folder and falling back to the (deprecated)
/// `root_uri`.
fn workspace_root(params: &InitializeParams) -> Option<PathBuf> {
    if let Some(folders) = &params.workspace_folders {
        if let Some(first) = folders.first() {
            if let Ok(path) = first.uri.to_file_path() {
                return Some(path);
            }
        }
    }
    #[allow(deprecated)]
    params.root_uri.as_ref().and_then(|u| u.to_file_path().ok())
}

/// Load the workspace config and build the engine + index. Returns
/// `Ok(None)` (not an error) when no config is present so callers clear
/// stale diagnostics.
fn build_session(root: &Path) -> Result<Option<Session>, String> {
    let Some(config_path) = alint_dsl::discover(root) else {
        return Ok(None);
    };
    // The discovered config's directory is the effective repo root.
    // `discover` walks up from the client-provided root, so a client that
    // rooted at a subfolder (Sublime/Eglot/Helix have no uniform root
    // marker) still gets the ancestor `.alint.yml` governing the whole
    // repo — and relative paths in rules resolve from there, matching the
    // CLI. When the client already rooted at the config's dir (the common
    // case), this is a no-op.
    let effective_root = config_path.parent().unwrap_or(root).to_path_buf();
    let config = alint_dsl::load(&config_path).map_err(|e| format!("loading config: {e}"))?;

    let registry = alint_rules::builtin_registry();
    let mut entries: Vec<RuleEntry> = Vec::with_capacity(config.rules.len());
    for spec in &config.rules {
        if matches!(spec.level, Level::Off) {
            continue;
        }
        let mut rule = registry
            .build(spec)
            .map_err(|e| format!("building rule {:?}: {e}", spec.id))?;
        // Apply the top-level `allow_out_of_root:` policy (top-level
        // config only; never via `extends:`). No-op for kinds that
        // don't honor the flag.
        rule.set_allow_out_of_root(config.allow_out_of_root.allows(&spec.id, &spec.kind));
        let mut entry = RuleEntry::new(rule);
        if let Some(when_src) = &spec.when {
            let expr = alint_core::when::parse(when_src)
                .map_err(|e| format!("rule {:?}: parsing `when`: {e}", spec.id))?;
            entry = entry.with_when(expr);
        }
        entries.push(entry);
    }

    let engine = Engine::from_entries(entries, registry)
        .with_facts(config.facts)
        .with_vars(config.vars);

    let walk_opts = WalkOptions {
        respect_gitignore: config.respect_gitignore,
        extra_ignores: config.ignore,
    };
    let index =
        walk(&effective_root, &walk_opts).map_err(|e| format!("walking repository: {e}"))?;

    Ok(Some(Session {
        root: effective_root,
        engine,
        index,
        config_path,
    }))
}

/// Build a session and run the full engine over it. `Ok(None)` ⇒ no
/// config (caller clears diagnostics). `Err` carries the config path so
/// the caller can surface a load/build failure as a diagnostic.
fn build_and_run(root: &Path) -> Result<Option<(Arc<Session>, FindingsByPath)>, BuildError> {
    let config_path = alint_dsl::discover(root);
    let session = match build_session(root) {
        Ok(Some(s)) => s,
        Ok(None) => return Ok(None),
        Err(message) => {
            return Err(BuildError {
                config_path,
                message,
            });
        }
    };
    let report = session
        .engine
        .run(&session.root, &session.index)
        .map_err(|e| BuildError {
            config_path: Some(session.config_path.clone()),
            message: format!("running rules: {e}"),
        })?;
    let by_path = group_findings(
        &session.root,
        &report.results,
        &session.engine,
        &session.config_path,
    );
    Ok(Some((Arc::new(session), by_path)))
}

/// Group rule-result violations into per-file findings keyed by absolute
/// path. Path-less findings (existence / tree-level rules) are anchored
/// to the config file so they're still visible in the editor. Each
/// finding is tagged `per_file` so the change hot path can preserve
/// cross-file findings.
fn group_findings(
    root: &Path,
    results: &[RuleResult],
    engine: &Engine,
    config_path: &Path,
) -> FindingsByPath {
    let mut by_path = FindingsByPath::new();
    for result in results {
        let Some(severity) = severity_of(result.level) else {
            continue;
        };
        let policy_url = result.policy_url.as_ref().map(ToString::to_string);
        let per_file = engine.is_per_file(&result.rule_id);
        for violation in &result.violations {
            // Anchor path-less (tree/file-level) findings to the config
            // file so a "missing required file" still shows somewhere.
            let abs = match &violation.path {
                Some(rel) => root.join(rel.as_ref()),
                None => config_path.to_path_buf(),
            };
            by_path.entry(abs).or_default().push(Finding {
                range: violation_range(violation),
                severity,
                rule_id: result.rule_id.to_string(),
                message: violation.message.to_string(),
                line: violation.line,
                column: violation.column,
                policy_url: policy_url.clone(),
                fixable: result.is_fixable,
                per_file,
            });
        }
    }
    by_path
}

fn severity_of(level: Level) -> Option<DiagnosticSeverity> {
    match level {
        Level::Error => Some(DiagnosticSeverity::ERROR),
        Level::Warning => Some(DiagnosticSeverity::WARNING),
        Level::Info => Some(DiagnosticSeverity::INFORMATION),
        Level::Off => None,
    }
}

fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::ERROR => "error",
        DiagnosticSeverity::WARNING => "warning",
        _ => "info",
    }
}

/// alint line/column are 1-indexed and optional; LSP positions are
/// 0-indexed. File- and tree-level findings (no line) anchor at the
/// start of the file. The range is one character wide so the editor has
/// something to attach the marker (and hover) to.
fn violation_range(violation: &Violation) -> Range {
    let line = violation
        .line
        .map_or(0, |l| u32::try_from(l.saturating_sub(1)).unwrap_or(0));
    let col = violation
        .column
        .map_or(0, |c| u32::try_from(c.saturating_sub(1)).unwrap_or(0));
    Range::new(
        Position::new(line, col),
        Position::new(line, col.saturating_add(1)),
    )
}

fn finding_to_diagnostic(f: &Finding) -> Diagnostic {
    let code_description = f
        .policy_url
        .as_deref()
        .and_then(|u| Url::parse(u).ok())
        .map(|href| CodeDescription { href });
    Diagnostic {
        range: f.range,
        severity: Some(f.severity),
        code: Some(NumberOrString::String(f.rule_id.clone())),
        code_description,
        source: Some("alint".to_string()),
        message: f.message.clone(),
        ..Diagnostic::default()
    }
}

/// True when `pos` falls within `range` (inclusive of both ends so a
/// hover on the single-character marker registers).
fn range_contains(range: Range, pos: Position) -> bool {
    let after_start = (pos.line, pos.character) >= (range.start.line, range.start.character);
    let before_end = (pos.line, pos.character) <= (range.end.line, range.end.character);
    after_start && before_end
}

/// True when two ranges intersect (the code-action selection vs. a
/// finding's marker).
fn ranges_overlap(a: Range, b: Range) -> bool {
    let a_start = (a.start.line, a.start.character);
    let a_end = (a.end.line, a.end.character);
    let b_start = (b.start.line, b.start.character);
    let b_end = (b.end.line, b.end.character);
    a_start <= b_end && b_start <= a_end
}

/// A range that covers any whole document. LSP clients clamp positions
/// past EOF, so this replaces the full file regardless of its length —
/// sidestepping UTF-16 column counting for a full-document edit.
fn whole_document() -> Range {
    Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX))
}

/// Map a core [`FixEdit`] to an LSP [`WorkspaceEdit`]. Content edits use
/// the widely-supported `changes` map; create/delete/rename use resource
/// operations (the client must advertise `resourceOperations` support).
/// Returns `None` when content isn't UTF-8 or a path can't become a URI.
fn fix_edit_to_workspace_edit(edit: &FixEdit, root: &Path) -> Option<WorkspaceEdit> {
    match edit {
        FixEdit::SetContent { path, content } => {
            let new_text = String::from_utf8(content.clone()).ok()?;
            let uri = Url::from_file_path(root.join(path)).ok()?;
            let mut changes = HashMap::new();
            changes.insert(
                uri,
                vec![TextEdit {
                    range: whole_document(),
                    new_text,
                }],
            );
            Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            })
        }
        FixEdit::CreateFile { path, content } => {
            let new_text = String::from_utf8(content.clone()).ok()?;
            let uri = Url::from_file_path(root.join(path)).ok()?;
            let ops = vec![
                DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                    uri: uri.clone(),
                    options: None,
                    annotation_id: None,
                })),
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
                    edits: vec![OneOf::Left(TextEdit {
                        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                        new_text,
                    })],
                }),
            ];
            Some(operations(ops))
        }
        FixEdit::DeleteFile { path } => {
            let uri = Url::from_file_path(root.join(path)).ok()?;
            Some(operations(vec![DocumentChangeOperation::Op(
                ResourceOp::Delete(DeleteFile { uri, options: None }),
            )]))
        }
        FixEdit::RenameFile { from, to } => {
            let old_uri = Url::from_file_path(root.join(from)).ok()?;
            let new_uri = Url::from_file_path(root.join(to)).ok()?;
            Some(operations(vec![DocumentChangeOperation::Op(
                ResourceOp::Rename(RenameFile {
                    old_uri,
                    new_uri,
                    options: None,
                    annotation_id: None,
                }),
            )]))
        }
    }
}

fn operations(ops: Vec<DocumentChangeOperation>) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: None,
        document_changes: Some(DocumentChanges::Operations(ops)),
        change_annotations: None,
    }
}

/// Markdown hover body for one finding: rule id + severity, the
/// message, and a policy link when the rule declares one.
fn render_finding(f: &Finding) -> String {
    let mut s = format!(
        "**alint** · `{}` ({})\n\n{}",
        f.rule_id,
        severity_label(f.severity),
        f.message
    );
    if let Some(url) = &f.policy_url {
        s.push_str("\n\n[Policy →](");
        s.push_str(url);
        s.push(')');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn violation(line: Option<usize>, column: Option<usize>) -> Violation {
        Violation {
            path: None,
            message: Cow::Borrowed("boom"),
            line,
            column,
            is_note: false,
        }
    }

    fn finding(policy_url: Option<&str>) -> Finding {
        Finding {
            range: Range::new(Position::new(3, 6), Position::new(3, 7)),
            severity: DiagnosticSeverity::ERROR,
            rule_id: "my-rule".to_string(),
            message: "boom".to_string(),
            // 1-indexed location matching the 0-indexed range above.
            line: Some(4),
            column: Some(7),
            policy_url: policy_url.map(ToString::to_string),
            fixable: false,
            per_file: true,
        }
    }

    #[test]
    fn severity_maps_levels_and_drops_off() {
        assert_eq!(severity_of(Level::Error), Some(DiagnosticSeverity::ERROR));
        assert_eq!(
            severity_of(Level::Warning),
            Some(DiagnosticSeverity::WARNING)
        );
        assert_eq!(
            severity_of(Level::Info),
            Some(DiagnosticSeverity::INFORMATION)
        );
        assert_eq!(severity_of(Level::Off), None);
    }

    #[test]
    fn violation_range_converts_one_indexed_to_zero_indexed() {
        let r = violation_range(&violation(Some(4), Some(7)));
        assert_eq!(r.start, Position::new(3, 6));
        assert_eq!(r.end, Position::new(3, 7));
    }

    #[test]
    fn violation_range_without_line_anchors_at_file_start() {
        let r = violation_range(&violation(None, None));
        assert_eq!(r.start, Position::new(0, 0));
        assert_eq!(r.end, Position::new(0, 1));
    }

    #[test]
    fn finding_to_diagnostic_carries_rule_and_policy_link() {
        let d = finding_to_diagnostic(&finding(Some("https://example.com/policy")));
        assert_eq!(d.code, Some(NumberOrString::String("my-rule".to_string())));
        assert_eq!(d.source.as_deref(), Some("alint"));
        assert_eq!(d.message, "boom");
        assert_eq!(
            d.code_description.unwrap().href.as_str(),
            "https://example.com/policy"
        );
    }

    #[test]
    fn finding_to_diagnostic_omits_code_description_for_non_url_policy() {
        let d = finding_to_diagnostic(&finding(Some("not a url")));
        assert!(d.code_description.is_none());
    }

    #[test]
    fn range_contains_is_inclusive_of_both_ends() {
        let r = Range::new(Position::new(3, 6), Position::new(3, 7));
        assert!(range_contains(r, Position::new(3, 6)));
        assert!(range_contains(r, Position::new(3, 7)));
        assert!(!range_contains(r, Position::new(3, 8)));
        assert!(!range_contains(r, Position::new(2, 6)));
    }

    #[test]
    fn render_finding_includes_rule_message_and_policy() {
        let md = render_finding(&finding(Some("https://example.com/p")));
        assert!(md.contains("my-rule"), "{md}");
        assert!(md.contains("(error)"), "{md}");
        assert!(md.contains("boom"), "{md}");
        assert!(md.contains("https://example.com/p"), "{md}");
    }

    #[test]
    fn render_finding_omits_policy_link_when_absent() {
        let md = render_finding(&finding(None));
        assert!(!md.contains("Policy"), "{md}");
    }

    #[test]
    fn build_session_returns_none_when_no_config() {
        let dir = tempfile::tempdir().unwrap();
        assert!(build_session(dir.path()).unwrap().is_none());
    }

    #[test]
    fn group_findings_anchors_pathless_to_config() {
        use alint_core::{Engine, RuleResult};
        let engine = Engine::new(vec![], alint_core::RuleRegistry::new());
        let root = repo_root();
        let config = root.join(".alint.yml");
        let results = vec![RuleResult {
            rule_id: std::sync::Arc::from("missing-license"),
            level: Level::Error,
            policy_url: None,
            violations: vec![Violation::new("LICENSE is missing")],
            notes: Vec::new(),
            is_fixable: false,
        }];
        let by_path = group_findings(&root, &results, &engine, &config);
        // The path-less violation is anchored to the config file.
        assert!(
            by_path.contains_key(&config),
            "anchored to config: {by_path:?}"
        );
        let finding = &by_path[&config][0];
        assert!(
            !finding.per_file,
            "unknown/cross-file rule tagged per_file=false"
        );
        assert_eq!(finding.message, "LICENSE is missing");
        // A path-less finding has no source location.
        assert_eq!(finding.line, None);
        assert_eq!(finding.column, None);
    }

    #[test]
    fn group_findings_threads_violation_line_and_column() {
        use alint_core::{Engine, RuleResult};
        let engine = Engine::new(vec![], alint_core::RuleRegistry::new());
        let root = repo_root();
        let config = root.join(".alint.yml");
        let results = vec![RuleResult {
            rule_id: std::sync::Arc::from("line-rule"),
            level: Level::Warning,
            policy_url: None,
            violations: vec![
                Violation::new("bad line")
                    .with_path(std::path::PathBuf::from("src/x.rs"))
                    .with_location(12, 3),
            ],
            notes: Vec::new(),
            is_fixable: true,
        }];
        let by_path = group_findings(&root, &results, &engine, &config);
        let finding = &by_path[&root.join("src/x.rs")][0];
        // The reported location is carried onto the finding (so a
        // range-scoped code-action fixer sees it), and drives the
        // 1-indexed -> 0-indexed LSP range.
        assert_eq!(finding.line, Some(12));
        assert_eq!(finding.column, Some(3));
        assert_eq!(finding.range.start.line, 11);
    }

    #[test]
    fn ranges_overlap_detects_intersection_and_disjoint() {
        let a = Range::new(Position::new(2, 0), Position::new(2, 10));
        assert!(ranges_overlap(
            a,
            Range::new(Position::new(2, 5), Position::new(2, 6))
        ));
        assert!(ranges_overlap(
            a,
            Range::new(Position::new(0, 0), Position::new(5, 0))
        ));
        assert!(!ranges_overlap(
            a,
            Range::new(Position::new(3, 0), Position::new(3, 1))
        ));
    }

    /// An absolute root valid on the test's OS — `Url::from_file_path`
    /// requires absolute, and `/repo` is NOT absolute on Windows.
    fn repo_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\repo")
        } else {
            PathBuf::from("/repo")
        }
    }

    #[test]
    fn set_content_maps_to_full_document_text_edit() {
        let root = repo_root();
        let edit = FixEdit::SetContent {
            path: PathBuf::from("a.txt"),
            content: b"fixed\n".to_vec(),
        };
        let ws = fix_edit_to_workspace_edit(&edit, &root).unwrap();
        let changes = ws.changes.expect("content edit uses the changes map");
        let uri = Url::from_file_path(root.join("a.txt")).unwrap();
        let edits = &changes[&uri];
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "fixed\n");
        assert_eq!(edits[0].range.start, Position::new(0, 0));
        assert!(ws.document_changes.is_none());
    }

    #[test]
    fn delete_maps_to_a_resource_operation() {
        let edit = FixEdit::DeleteFile {
            path: PathBuf::from("debug.log"),
        };
        let ws = fix_edit_to_workspace_edit(&edit, &repo_root()).unwrap();
        assert!(ws.changes.is_none());
        let Some(DocumentChanges::Operations(ops)) = ws.document_changes else {
            panic!("delete must use resource operations");
        };
        assert_eq!(ops.len(), 1);
        assert!(matches!(
            ops[0],
            DocumentChangeOperation::Op(ResourceOp::Delete(_))
        ));
    }

    #[test]
    fn create_maps_to_create_op_plus_insert_edit() {
        let edit = FixEdit::CreateFile {
            path: PathBuf::from("LICENSE"),
            content: b"Apache-2.0\n".to_vec(),
        };
        let ws = fix_edit_to_workspace_edit(&edit, &repo_root()).unwrap();
        let Some(DocumentChanges::Operations(ops)) = ws.document_changes else {
            panic!("create must use resource operations");
        };
        assert_eq!(ops.len(), 2);
        assert!(matches!(
            ops[0],
            DocumentChangeOperation::Op(ResourceOp::Create(_))
        ));
        assert!(matches!(ops[1], DocumentChangeOperation::Edit(_)));
    }

    #[test]
    fn rename_maps_to_rename_op() {
        let edit = FixEdit::RenameFile {
            from: PathBuf::from("FooBar.rs"),
            to: PathBuf::from("foo_bar.rs"),
        };
        let ws = fix_edit_to_workspace_edit(&edit, &repo_root()).unwrap();
        let Some(DocumentChanges::Operations(ops)) = ws.document_changes else {
            panic!("rename must use resource operations");
        };
        assert!(matches!(
            ops[0],
            DocumentChangeOperation::Op(ResourceOp::Rename(_))
        ));
    }

    #[test]
    fn set_content_with_non_utf8_yields_no_edit() {
        let edit = FixEdit::SetContent {
            path: PathBuf::from("a.bin"),
            content: vec![0xff, 0xfe],
        };
        assert!(fix_edit_to_workspace_edit(&edit, &repo_root()).is_none());
    }
}
