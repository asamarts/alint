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
//!   whose rule declares a fixer, returning a `WorkspaceEdit` the editor
//!   applies to the buffer. A whole-file fixer maps via
//!   [`alint_core::Fixer::fix_edit`] → [`alint_core::FixEdit`]; a *located*
//!   fixer (e.g. `replace`) maps its `collect_edits` byte ranges to UTF-16
//!   `TextEdit`s (one per match) so a single action rewrites every occurrence.
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

use alint_core::located_fix::{self, LocatedEdit, LocatedOutcome};
use alint_core::{
    Applicability, CollectedEdit, Engine, Error, FileIndex, FixEdit, Level, RuleEntry, RuleResult,
    Violation, WalkOptions, walk,
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
    /// Shared server state behind a `parking_lot::Mutex` (no poisoning), so a
    /// panic in one async handler can't permanently wedge the session the way
    /// a poisoned `std::sync::Mutex` would. Note the trade-off: `parking_lot`
    /// has no poison signal, so if a handler panics *while holding the guard*,
    /// the lock simply releases and the next handler reads whatever partial
    /// state the panic left — the session survives (good for an editor) but
    /// consistency-on-panic is NOT guaranteed. Critical sections are kept tiny
    /// and infallible (map inserts/clones) to keep that window closed; the
    /// engine runs off-lock inside `spawn_blocking`.
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
        // Parallel to `actions`: whether each offered fix is Unsafe-tier. An Unsafe
        // quick-fix is labeled `(unsafe)` and never auto-preferred, so the click is
        // a visible, explicit opt-in (the LSP analogue of `--unsafe-fixes`) rather
        // than a silent one-click apply of a behavior-changing edit (e.g. a file
        // delete). Only Safe and Unsafe tiers reach here -- Suggestion / demoted
        // fixes are already filtered out below.
        let mut unsafe_flags: Vec<bool> = Vec::new();
        for finding in &findings {
            if !finding.fixable || !ranges_overlap(finding.range, selection) {
                continue;
            }
            let Some(fixer) = session.engine.fixer_for(&finding.rule_id) else {
                continue;
            };
            let unsafe_fix = fixer.applicability() == Applicability::Unsafe;
            let mut violation = Violation::new(finding.message.clone()).with_path(rel.clone());
            // Preserve the reported location so range-scoped fixers act on
            // the right line/column (not just whole-file fixers).
            violation.line = finding.line;
            violation.column = finding.column;
            // A LOCATED fixer (Phase 1 `replace`) emits byte-range edits via
            // `collect_edits`, not `fix_edit`. Collect ALL of them for this file and
            // map each byte range to a UTF-16 `TextEdit` -- one multi-edit
            // `WorkspaceEdit`, so the code action rewrites every occurrence, matching
            // `alint fix` (the diagnostic is one-per-file but the located fix is
            // per-match). The synthetic `violation` carries NO baseline_key, so the
            // structured fixer's W4 violation-set correlation finds an empty budget
            // and falls back to fixing every occurrence -- exactly the "fix all"
            // action wanted here. A whole-file fixer keeps the `fix_edit` -> edit path.
            let workspace_edit = if fixer.collects_located_edits() {
                // Run the SAME pipeline `alint fix` uses (tier-filter -> overlap-skip
                // -> post-splice verify/demote) and offer ONLY the surviving edits.
                // Without this the LSP would one-click-apply a fix the engine
                // refuses -- a partial removal that leaves the violation, an
                // unverifiable `replace`, or a below-threshold / W2-demoted
                // (Suggestion-tier) untrusted-remote content fixer.
                let batch: Vec<LocatedEdit> = fixer
                    .collect_edits(std::slice::from_ref(&violation), &rel, bytes, &session.root)
                    .into_iter()
                    .enumerate()
                    .map(|(i, ce)| LocatedEdit {
                        rule_index: 0,
                        violation_index: i,
                        collected: ce,
                    })
                    .collect();
                let (_, outcomes) =
                    located_fix::apply_file_edits(bytes, batch, Applicability::Unsafe);
                let applied: Vec<CollectedEdit> = outcomes
                    .into_iter()
                    .filter(|(_, outcome)| *outcome == LocatedOutcome::Applied)
                    .map(|(le, _)| le.collected)
                    .collect();
                match located_edits_to_workspace_edit(&applied, &text, &rel, &session.root) {
                    Some(we) => we,
                    None => continue,
                }
            } else {
                // Whole-file / file-op fixer: gate on its tier so a Suggestion-tier
                // fixer -- notably a W2-demoted content fixer from an untrusted
                // remote `extends:` -- is not offered as an ordinary quick-fix
                // (demotion drops it to Suggestion; a bare `alint fix` only
                // suggests it, never auto-writes).
                if !fixer.applicability().applies_at(Applicability::Unsafe) {
                    continue;
                }
                let Some(edit) = fixer.fix_edit(&violation, bytes, &session.root) else {
                    continue;
                };
                match fix_edit_to_workspace_edit(&edit, &session.root) {
                    Some(we) => we,
                    None => continue,
                }
            };
            let title = if unsafe_fix {
                format!("alint: fix `{}` (unsafe)", finding.rule_id)
            } else {
                format!("alint: fix `{}`", finding.rule_id)
            };
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![finding_to_diagnostic(finding)]),
                edit: Some(workspace_edit),
                ..CodeAction::default()
            }));
            unsafe_flags.push(unsafe_fix);
        }
        if actions.is_empty() {
            return Ok(None);
        }
        // A single SAFE fix is the obvious one to apply (some clients auto-apply
        // the preferred action); an Unsafe fix is never auto-preferred -- applying
        // it must stay a deliberate choice.
        if actions.len() == 1 && !unsafe_flags[0] {
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
        let allow_out_of_root = config.allow_out_of_root.allows(&spec.id, &spec.kind);
        rule.set_allow_out_of_root(allow_out_of_root);
        let mut entry = RuleEntry::new(rule).with_allow_out_of_root(allow_out_of_root);
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

/// Convert a byte offset into `text` (valid UTF-8) to an LSP [`Position`]:
/// 0-indexed line, and a character column counted in UTF-16 code units (the LSP
/// default position encoding). A non-BMP scalar (e.g. an emoji) is two UTF-16
/// units, so a byte or `char` count would misplace the edit. `'\n'` ends a line and
/// resets the column; a lone `'\r'` counts as an ordinary character. A range that
/// spans several lines (a multi-line `replace` pattern, e.g. `(?s)foo.bar`) is
/// handled -- start and end are converted independently. An offset at or past the
/// end of `text` clamps to the final position.
fn byte_offset_to_position(text: &str, byte_offset: usize) -> Position {
    let mut line: u32 = 0;
    let mut character: u32 = 0;
    for (idx, ch) in text.char_indices() {
        if idx >= byte_offset {
            return Position::new(line, character);
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += u32::try_from(ch.len_utf16()).unwrap_or(0);
        }
    }
    Position::new(line, character)
}

/// Map a located fixer's collected byte-range edits (Phase 1 `replace`) to an LSP
/// [`WorkspaceEdit`] for one file: each [`FixEdit::ReplaceRange`] becomes a
/// [`TextEdit`] whose range is the byte offsets converted to UTF-16 positions
/// against `text` (the current buffer, which is what the offsets index).
/// `collect_edits` yields DISJOINT, left-to-right matches, so the `TextEdit`s never
/// overlap -- exactly what the LSP requires of a single edit set. Returns `None`
/// when there are no range edits, a replacement isn't UTF-8, or the path can't
/// become a URI, so the caller offers no action rather than a partial one.
fn located_edits_to_workspace_edit(
    edits: &[CollectedEdit],
    text: &str,
    rel: &Path,
    root: &Path,
) -> Option<WorkspaceEdit> {
    let uri = Url::from_file_path(root.join(rel)).ok()?;
    let mut text_edits = Vec::new();
    for ce in edits {
        // Every located fixer today (ReplaceFixer, the only one) emits ReplaceRange.
        // A future located fixer that emitted a whole-file edit here would have it
        // silently dropped -- pin the invariant so that regresses LOUDLY in debug,
        // and skip (never mis-apply) in release.
        let FixEdit::ReplaceRange { range, content, .. } = &ce.edit else {
            debug_assert!(false, "located edit is not a ReplaceRange: {:?}", ce.edit);
            continue;
        };
        let new_text = String::from_utf8(content.clone()).ok()?;
        let start = byte_offset_to_position(text, range.start);
        let end = byte_offset_to_position(text, range.end);
        text_edits.push(TextEdit {
            range: Range::new(start, end),
            new_text,
        });
    }
    if text_edits.is_empty() {
        return None;
    }
    let mut changes = HashMap::new();
    changes.insert(uri, text_edits);
    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
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
        // A `ReplaceRange` reaches the LSP through `collect_edits` (a located
        // fixer's real path), mapped to UTF-16 `TextEdit`s by
        // `located_edits_to_workspace_edit` -- NOT through `fix_edit`, which no
        // located fixer implements (it returns `None`). So a `ReplaceRange` here
        // is unreachable, and there is no buffer context to map its byte offsets
        // anyway. A `chmod` (`SetMode`) has no LSP `WorkspaceEdit` representation at
        // all. Both map to `None`.
        FixEdit::ReplaceRange { .. } | FixEdit::SetMode { .. } => None,
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
            baseline_key: None,
            is_fixable: false,
            proposed_edits: Vec::new(),
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

    #[test]
    fn byte_offset_to_position_counts_utf16_code_units() {
        // ASCII, single line.
        assert_eq!(byte_offset_to_position("hello", 0), Position::new(0, 0));
        assert_eq!(byte_offset_to_position("hello", 3), Position::new(0, 3));
        assert_eq!(byte_offset_to_position("hello", 5), Position::new(0, 5)); // clamps at end
        // Multi-line: the offset just after '\n' is line 1, character 0.
        let two = "ab\ncd";
        assert_eq!(byte_offset_to_position(two, 2), Position::new(0, 2)); // before '\n'
        assert_eq!(byte_offset_to_position(two, 3), Position::new(1, 0)); // 'c'
        assert_eq!(byte_offset_to_position(two, 5), Position::new(1, 2)); // end of "cd"
        // R-UTF16: U+1F600 is 4 UTF-8 bytes but TWO UTF-16 code units, so a byte- or
        // `char`-based column would misplace an edit after it.
        let emoji = "a\u{1F600}b";
        assert_eq!(byte_offset_to_position(emoji, 1), Position::new(0, 1)); // before the emoji
        assert_eq!(
            byte_offset_to_position(emoji, 5),
            Position::new(0, 3),
            "the emoji is two UTF-16 units, so 'b' is at character 3"
        );
        // CRLF: '\r' counts as an ordinary character; a position after the CRLF is
        // line 1, character 0 (so a match on the next line lands correctly).
        let crlf = "ab\r\ncd";
        assert_eq!(byte_offset_to_position(crlf, 2), Position::new(0, 2)); // end of line-0 text
        assert_eq!(byte_offset_to_position(crlf, 4), Position::new(1, 0)); // 'c' after \r\n
        // Empty text: only the origin is reachable.
        assert_eq!(byte_offset_to_position("", 0), Position::new(0, 0));
    }

    #[test]
    fn located_edits_map_to_utf16_text_edits() {
        // A non-BMP char BEFORE the edit range: a byte- or char-based column would be
        // wrong. The located edit replaces the 4-byte "TODO" with "DONE".
        let root = repo_root();
        let text = "x\u{1F600} TODO\n";
        let todo = text.find("TODO").unwrap();
        let edit = CollectedEdit {
            edit: FixEdit::ReplaceRange {
                path: PathBuf::from("a.txt"),
                range: todo..todo + 4,
                content: b"DONE".to_vec(),
            },
            applicability: alint_core::Applicability::Unsafe,
            verify: alint_core::EditVerifier::None,
            isolation_group: None,
        };
        let ws = located_edits_to_workspace_edit(&[edit], text, Path::new("a.txt"), &root).unwrap();
        let changes = ws.changes.expect("a located edit uses the changes map");
        let uri = Url::from_file_path(root.join("a.txt")).unwrap();
        let edits = &changes[&uri];
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "DONE");
        // 'x'=0, emoji=cols 1-2 (two units), ' '=3, so "TODO" starts at character 4.
        assert_eq!(
            edits[0].range.start,
            Position::new(0, 4),
            "the emoji counts as two UTF-16 units"
        );
        assert_eq!(edits[0].range.end, Position::new(0, 8));
        assert!(ws.document_changes.is_none());
    }

    #[test]
    fn located_edit_spanning_a_newline_maps_to_a_multi_line_range() {
        // A `(?s)`-style pattern can match across a line break; the resulting
        // `TextEdit` range must cross lines. Replace "foo\nbar" (bytes 0..7) with "X".
        let root = repo_root();
        let text = "foo\nbar\n";
        let edit = CollectedEdit {
            edit: FixEdit::ReplaceRange {
                path: PathBuf::from("a.txt"),
                range: 0..7,
                content: b"X".to_vec(),
            },
            applicability: alint_core::Applicability::Unsafe,
            verify: alint_core::EditVerifier::None,
            isolation_group: None,
        };
        let ws = located_edits_to_workspace_edit(&[edit], text, Path::new("a.txt"), &root).unwrap();
        let uri = Url::from_file_path(root.join("a.txt")).unwrap();
        let te = &ws.changes.unwrap()[&uri][0];
        assert_eq!(te.range.start, Position::new(0, 0));
        assert_eq!(
            te.range.end,
            Position::new(1, 3),
            "the range crosses into line 1, character 3 (past 'bar')"
        );
        assert_eq!(te.new_text, "X");
    }

    #[test]
    fn located_edits_with_no_range_edits_yield_none() {
        // A located fixer that collected nothing offers NO action (not an empty one).
        assert!(
            located_edits_to_workspace_edit(&[], "abc", Path::new("a.txt"), &repo_root()).is_none()
        );
    }

    #[tokio::test]
    async fn code_action_offers_the_located_replace_fix_end_to_end() {
        // Drive the real async `code_action` for a located fixer: a genuine
        // `file_content_forbidden` + `replace` session, an open buffer with the
        // forbidden pattern, and a diagnostic over it. The response must carry a
        // quick-fix whose `WorkspaceEdit` replaces the match with a UTF-16 `TextEdit`.
        // Gates the routing (`collects_located_edits` branch) + state handling that
        // the mapper unit tests don't reach. `code_action` touches only `self.state`
        // (never `self.client`), so it runs without a live LSP socket.
        use tower_lsp::lsp_types::{
            CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
        };

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::write(
            root.join(".alint.yml"),
            "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
             paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n    \
             fix: { replace: { replacement: \"DONE\" } }\n",
        )
        .unwrap();
        let session = build_session(&root)
            .expect("build_session ok")
            .expect("config present");

        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();

        let uri = Url::from_file_path(root.join("a.txt")).unwrap();
        let finding = Finding {
            range: Range::new(Position::new(0, 2), Position::new(0, 3)),
            severity: DiagnosticSeverity::ERROR,
            rule_id: "no-todo".to_string(),
            message: "forbidden".to_string(),
            line: Some(1),
            column: Some(3),
            policy_url: None,
            fixable: true,
            per_file: true,
        };
        {
            let mut st = backend.state.lock();
            st.root = Some(root.clone());
            st.session = Some(Arc::new(session));
            st.open.insert(uri.clone());
            st.documents.insert(uri.clone(), "x TODO\n".to_string());
            st.diagnostics.insert(uri.clone(), vec![finding]);
        }

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(0, 2), Position::new(0, 3)),
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let resp = backend
            .code_action(params)
            .await
            .expect("code_action ok")
            .expect("an action is offered");
        assert_eq!(
            resp.len(),
            1,
            "exactly one quick-fix for the replace violation"
        );
        let CodeActionOrCommand::CodeAction(action) = &resp[0] else {
            panic!("expected a CodeAction, not a Command");
        };
        let ws = action
            .edit
            .as_ref()
            .expect("the action carries a workspace edit");
        let changes = ws
            .changes
            .as_ref()
            .expect("a located edit uses the changes map");
        let tes = &changes[&uri];
        assert_eq!(tes.len(), 1, "one TextEdit for the single TODO");
        assert_eq!(tes[0].new_text, "DONE");
        // "x TODO": 'x'=0, ' '=1, "TODO" occupies characters 2..6.
        assert_eq!(tes[0].range.start, Position::new(0, 2));
        assert_eq!(tes[0].range.end, Position::new(0, 6));
    }

    #[tokio::test]
    async fn code_action_offers_a_structured_set_value_fix() {
        // Follow-up 4: the structured located fixers (`set_value`/`remove_value`/
        // `replace` on the `*_path_*` kinds) reach the editor through the SAME
        // `collects_located_edits` branch as `replace`. Drive `code_action` for a
        // real `json_path_equals` + `set_value` session: the response must carry a
        // quick-fix whose `TextEdit` rewrites just the value span.
        use tower_lsp::lsp_types::{
            CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
        };
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::write(
            root.join(".alint.yml"),
            "version: 1\nrules:\n  - id: port\n    kind: json_path_equals\n    \
             paths: \"*.json\"\n    path: \"$.port\"\n    equals: 9090\n    level: error\n    \
             fix: { set_value: {} }\n",
        )
        .unwrap();
        let session = build_session(&root)
            .expect("build_session ok")
            .expect("config present");
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = Url::from_file_path(root.join("app.json")).unwrap();
        let finding = Finding {
            range: Range::new(Position::new(0, 9), Position::new(0, 13)),
            severity: DiagnosticSeverity::ERROR,
            rule_id: "port".to_string(),
            message: "value at path does not equal expected".to_string(),
            line: Some(1),
            column: Some(10),
            policy_url: None,
            fixable: true,
            per_file: true,
        };
        {
            let mut st = backend.state.lock();
            st.root = Some(root.clone());
            st.session = Some(Arc::new(session));
            st.open.insert(uri.clone());
            st.documents
                .insert(uri.clone(), "{\"port\": 8080}".to_string());
            st.diagnostics.insert(uri.clone(), vec![finding]);
        }
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(0, 9), Position::new(0, 13)),
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let resp = backend
            .code_action(params)
            .await
            .expect("code_action ok")
            .expect("an action is offered");
        let CodeActionOrCommand::CodeAction(action) = &resp[0] else {
            panic!("expected a CodeAction");
        };
        let ws = action.edit.as_ref().expect("workspace edit");
        let tes = &ws.changes.as_ref().expect("changes map")[&uri];
        assert_eq!(tes.len(), 1, "one TextEdit for the value span");
        assert_eq!(tes[0].new_text, "9090");
        // `{"port": 8080}`: `8080` occupies UTF-16 columns 9..13.
        assert_eq!(tes[0].range.start, Position::new(0, 9));
        assert_eq!(tes[0].range.end, Position::new(0, 13));
    }

    #[tokio::test]
    async fn code_action_withholds_a_fix_the_pipeline_would_demote() {
        // Audit HIGH: the LSP must not offer a quick-fix the engine refuses. It
        // now routes located edits through `apply_file_edits` (the same verify /
        // overlap / tier pipeline `alint fix` uses) and offers only survivors. A
        // `remove_value` batch that can only partially remove is demoted
        // all-or-nothing (the `Absent` verify fails), so `alint fix` writes
        // nothing -- the code action must offer nothing, never a partial (here:
        // secret-leaking) removal.
        use tower_lsp::lsp_types::{
            CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
        };
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::write(
            root.join(".alint.yml"),
            "version: 1\nrules:\n  - id: no-secret\n    kind: hcl_path_absent\n    \
             paths: \"*.tf\"\n    path: \"$..secret\"\n    level: error\n    \
             fix: { remove_value: { applicability: safe } }\n",
        )
        .unwrap();
        let session = build_session(&root)
            .expect("build_session ok")
            .expect("config present");
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = Url::from_file_path(root.join("main.tf")).unwrap();
        // A top-level secret + two block secrets: the block members can't be
        // removed, so the whole batch demotes.
        let content =
            "secret = \"top\"\nitem {\n  secret = \"a\"\n}\nitem {\n  secret = \"b\"\n}\n";
        let finding = Finding {
            range: Range::new(Position::new(0, 0), Position::new(0, 6)),
            severity: DiagnosticSeverity::ERROR,
            rule_id: "no-secret".to_string(),
            message: "secret present".to_string(),
            line: Some(1),
            column: Some(1),
            policy_url: None,
            fixable: true,
            per_file: true,
        };
        {
            let mut st = backend.state.lock();
            st.root = Some(root.clone());
            st.session = Some(Arc::new(session));
            st.open.insert(uri.clone());
            st.documents.insert(uri.clone(), content.to_string());
            st.diagnostics.insert(uri.clone(), vec![finding]);
        }
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(0, 0), Position::new(0, 6)),
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let resp = backend.code_action(params).await.expect("code_action ok");
        assert!(
            resp.is_none(),
            "a fix the pipeline demotes must not be offered; got: {resp:?}"
        );
    }

    #[tokio::test]
    async fn code_action_labels_an_unsafe_fix_and_does_not_prefer_it() {
        // An Unsafe fix (here `replace`, default Unsafe) IS offered as a quick-fix
        // (human-in-the-loop), but its title is labeled `(unsafe)` and it is NOT
        // marked preferred -- so clicking it is a visible, deliberate opt-in (the
        // LSP analogue of `--unsafe-fixes`), never an editor auto-apply.
        use tower_lsp::lsp_types::{
            CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
        };
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::write(
            root.join(".alint.yml"),
            "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
             paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n    \
             fix: { replace: { replacement: \"DONE\" } }\n",
        )
        .unwrap();
        let session = build_session(&root)
            .expect("build_session ok")
            .expect("config present");
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = Url::from_file_path(root.join("a.txt")).unwrap();
        let finding = Finding {
            range: Range::new(Position::new(0, 2), Position::new(0, 6)),
            severity: DiagnosticSeverity::ERROR,
            rule_id: "no-todo".to_string(),
            message: "forbidden".to_string(),
            line: Some(1),
            column: Some(3),
            policy_url: None,
            fixable: true,
            per_file: true,
        };
        {
            let mut st = backend.state.lock();
            st.root = Some(root.clone());
            st.session = Some(Arc::new(session));
            st.open.insert(uri.clone());
            st.documents.insert(uri.clone(), "x TODO\n".to_string());
            st.diagnostics.insert(uri.clone(), vec![finding]);
        }
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(0, 2), Position::new(0, 6)),
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let resp = backend
            .code_action(params)
            .await
            .expect("code_action ok")
            .expect("an action is offered");
        let CodeActionOrCommand::CodeAction(action) = &resp[0] else {
            panic!("expected a CodeAction");
        };
        assert_eq!(action.title, "alint: fix `no-todo` (unsafe)");
        assert_ne!(
            action.is_preferred,
            Some(true),
            "an Unsafe fix must not be auto-preferred"
        );
    }

    #[test]
    fn located_lsp_path_maps_a_real_replace_fixer_to_utf16_text_edits() {
        // End-to-end for the located `code_action` path: a REAL `file_content_forbidden`
        // + `replace` rule, its `ReplaceFixer::collect_edits`, mapped to UTF-16
        // `TextEdit`s -- the exact sequence `code_action` runs for a located fixer.
        // A non-BMP char precedes the matches so the UTF-16 column mapping is
        // load-bearing, and there are TWO occurrences so the per-match multi-edit
        // behavior (not just the diagnostic's first match) is exercised.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join(".alint.yml"),
            "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
             paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n    \
             fix: { replace: { replacement: \"DONE\" } }\n",
        )
        .unwrap();
        let session = build_session(root)
            .expect("build_session succeeds")
            .expect("a config is present");
        let fixer = session
            .engine
            .fixer_for("no-todo")
            .expect("no-todo declares a fixer");
        assert!(
            fixer.collects_located_edits(),
            "replace is a located fixer, so code_action takes the collect_edits path"
        );
        let text = "x\u{1F600} TODO and TODO\n";
        let violation = Violation::new("forbidden").with_path(PathBuf::from("a.txt"));
        let edits = fixer.collect_edits(
            std::slice::from_ref(&violation),
            Path::new("a.txt"),
            text.as_bytes(),
            root,
        );
        let ws = located_edits_to_workspace_edit(&edits, text, Path::new("a.txt"), root)
            .expect("the located edits map to a workspace edit");
        let changes = ws.changes.expect("located edits use the changes map");
        let uri = Url::from_file_path(root.join("a.txt")).unwrap();
        let tes = &changes[&uri];
        assert_eq!(tes.len(), 2, "both TODO occurrences become TextEdits");
        assert!(tes.iter().all(|te| te.new_text == "DONE"));
        // 'x'=0, emoji=cols 1-2 (two UTF-16 units), ' '=3 -> first TODO at character 4.
        assert_eq!(tes[0].range.start, Position::new(0, 4));
    }

    #[test]
    fn code_action_offers_every_match_for_a_path_matches_replace() {
        // Regression (audit 2026-09-20): the located `replace` fixer now correlates
        // its edits to the violation SET (W4). `code_action` synthesizes a
        // positional violation with NO baseline_key, so the fixer falls back to
        // fixing every occurrence -- otherwise the empty correlation budget would
        // leave the LSP offering NO `*_path_matches` fix.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join(".alint.yml"),
            "version: 1\nrules:\n  - id: v-pins\n    kind: json_path_matches\n    \
             paths: \"*.json\"\n    path: \"$.deps.*\"\n    matches: \"^v\"\n    level: error\n    \
             fix: { replace: { pattern: \"^\", replacement: \"v\", applicability: safe } }\n",
        )
        .unwrap();
        let session = build_session(root)
            .expect("build_session succeeds")
            .expect("a config is present");
        let fixer = session
            .engine
            .fixer_for("v-pins")
            .expect("v-pins declares a fixer");
        assert!(fixer.collects_located_edits());
        let text = "{\"deps\": {\"a\": \"1.0\", \"b\": \"2.0\"}}";
        // As `code_action` builds it: a positional path-bearing violation, NO key.
        let violation = Violation::new("v-pins").with_path(PathBuf::from("app.json"));
        let edits = fixer.collect_edits(
            std::slice::from_ref(&violation),
            Path::new("app.json"),
            text.as_bytes(),
            root,
        );
        let ws = located_edits_to_workspace_edit(&edits, text, Path::new("app.json"), root)
            .expect("the located edits map to a workspace edit");
        let changes = ws.changes.expect("located edits use the changes map");
        let uri = Url::from_file_path(root.join("app.json")).unwrap();
        assert_eq!(
            changes[&uri].len(),
            2,
            "both failing nodes are offered (the keyless fix-all fallback)"
        );
    }
}
