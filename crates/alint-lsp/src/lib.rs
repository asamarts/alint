//! Language Server Protocol server for alint.
//!
//! A thin `tower-lsp` backend that runs the alint engine over the
//! workspace and publishes the resulting violations as LSP diagnostics.
//! It is driven by the `alint lsp` subcommand, speaking LSP over stdio
//! (see [`run_stdio`]).
//!
//! Config discovery is **per document**: each open file is linted by the
//! nearest `.alint.yml` found walking up from its directory (the same
//! search the CLI does from its working directory), so a monorepo with
//! nested package configs gets one session per config. Every workspace
//! folder the client reports (multi-root) is honored; a file outside all
//! of them is not linted.
//!
//! Evaluation paths:
//!
//! - **Open / save** run the full [`alint_core::Engine`] for every
//!   relevant config (cross-file rules included) and publish per-file
//!   diagnostics for every open document. An open document whose
//!   unsaved buffer differs from disk has its per-file rules re-run
//!   against the buffer (an overlay), so a save elsewhere never paints
//!   stale on-disk results over unsaved edits.
//! - **Change** uses the single-file hot path
//!   ([`alint_core::Engine::run_for_file`]) against the editor's
//!   in-memory bytes, so per-keystroke feedback costs one file's
//!   evaluation, not the whole tree's. Cross-file rules are not
//!   re-run on change (they refresh on the next save), matching
//!   `docs/design/v0.11/single_file_reevaluation.md`.
//! - **Baseline**: a config's `baseline:` file is honored exactly as
//!   `alint check` honors it, so grandfathered findings stay hidden.
//! - **Hover** over a violation marker renders the rule id, message,
//!   the rule kind's description, fix availability, a rule-reference
//!   link, and the `policy_url` from the per-file cache of the
//!   last-published findings.
//! - **Code actions** offer an "Apply fix" quick-fix for any violation
//!   whose rule declares a fixer, returning a `WorkspaceEdit` the editor
//!   applies to the buffer. A whole-file fixer maps via
//!   [`alint_core::Fixer::fix_edit`] → [`alint_core::FixEdit`]; a *located*
//!   fixer (e.g. `replace`) maps its `collect_edits` byte ranges to UTF-16
//!   `TextEdit`s (one per match) so a single action rewrites every occurrence.
//!   A buffer larger than the config's `fix_size_limit` gets no fix, as
//!   `alint fix` would skip it.
//! - **Watched files** (`didChangeWatchedFiles`) reload the session, so
//!   `.alint.yml` edits take effect without saving an open document.
//!
//! Diagnostic positions use the LSP default UTF-16 encoding: alint's
//! 1-based character columns are converted against the document text.
//!
//! The "add rule to ignore" action is deferred to a later slice of the
//! LSP epic.

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context as TaskContext, Poll};

use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CodeDescription, CreateFile, DeleteFile,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentChangeOperation, DocumentChanges, Hover, HoverContents,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    MarkupContent, MarkupKind, MessageType, NumberOrString, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, RenameFile, ResourceOp,
    ServerCapabilities, ServerInfo, TextDocumentEdit, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Url, WorkspaceEdit, WorkspaceFoldersServerCapabilities,
    WorkspaceServerCapabilities,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, jsonrpc::Result as JsonRpcResult};

use alint_core::baseline::Baseline;
use alint_core::located_fix::{self, LocatedEdit, LocatedOutcome};
use alint_core::{
    Applicability, CollectedEdit, Engine, Error, FileIndex, FixEdit, Level, Report, RuleEntry,
    RuleResult, Violation, WalkOptions, walk,
};

/// Server options, set from the `alint lsp` command line.
#[derive(Debug, Clone, Copy, Default)]
pub struct LspOptions {
    /// `--show-notes`: list every informational note on stderr after a
    /// full check, instead of the default one-line count.
    pub show_notes: bool,
}

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
    /// The rule kind's one-sentence description, for the hover.
    description: Option<&'static str>,
    /// The rule kind's alint.org reference page, for the hover.
    docs_url: Option<String>,
}

/// Per-file findings keyed by absolute path.
type FindingsByPath = HashMap<PathBuf, Vec<Finding>>;

/// A loaded config: the config-built engine plus the walked index.
/// Cached on open/save and reused by the change hot path so a keystroke
/// doesn't re-load the config or re-walk the tree. One per discovered
/// config file.
#[derive(Debug)]
struct Session {
    root: PathBuf,
    engine: Engine,
    index: FileIndex,
    /// The discovered `.alint.yml` (absolute). Used to anchor path-less
    /// findings and config errors as diagnostics.
    config_path: PathBuf,
    /// The config's `baseline:` file, loaded — grandfathered findings
    /// are filtered out exactly as `alint check` filters them.
    baseline: Option<Baseline>,
    /// Rule id → rule kind, for the hover's description + docs link.
    kinds: HashMap<String, String>,
}

impl Session {
    fn group_ctx(&self) -> GroupCtx<'_> {
        GroupCtx {
            root: &self.root,
            engine: &self.engine,
            config_path: &self.config_path,
            kinds: &self.kinds,
        }
    }
}

/// The outcome of one config's full check.
#[derive(Debug)]
struct ConfigRun {
    session: Arc<Session>,
    by_path: FindingsByPath,
    /// Informational notes, rendered `path: message` (for stderr).
    notes: Vec<String>,
}

/// One full workspace check: which config governs each open document,
/// and each config's run (or its load/build error message).
#[derive(Debug, Default)]
struct WorkspaceCheck {
    doc_config: HashMap<Url, PathBuf>,
    runs: Vec<(PathBuf, Result<ConfigRun, String>)>,
}

/// Build a tokio runtime and serve the alint language server over
/// stdio until the client sends `exit` (or disconnects). Called by the
/// `alint lsp` subcommand so the CLI itself stays synchronous.
///
/// Returns the process exit code the LSP specification mandates: `0`
/// when `exit` follows a `shutdown` request (or the client simply
/// closed the stream), `1` when `exit` arrives without a prior
/// `shutdown`. The runtime is shut down without waiting on the blocked
/// stdin reader, so the process terminates promptly on `exit` even
/// while the client keeps stdin open.
pub fn run_stdio(options: LspOptions) -> std::io::Result<i32> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_flag = Arc::clone(&shutdown);
    let exit_received = runtime.block_on(async move {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = LspService::new(move |client| Backend::new(client, options));
        let (exit_tx, exit_rx) = tokio::sync::oneshot::channel();
        let service = ExitInterceptor {
            inner: service,
            shutdown: shutdown_flag,
            exit: Some(exit_tx),
        };
        tokio::select! {
            () = Server::new(stdin, stdout, socket).serve(service) => false,
            Ok(()) = exit_rx => true,
        }
    });
    // `tokio::io::stdin` reads on a blocking thread that can't be
    // interrupted; waiting for it would hang until the client closes
    // stdin. Detach it instead.
    runtime.shutdown_background();
    Ok(exit_code(exit_received, shutdown.load(Ordering::SeqCst)))
}

/// The LSP-mandated process exit code: `1` for an `exit` notification
/// that was not preceded by `shutdown`, else `0`.
fn exit_code(exit_received: bool, shutdown_received: bool) -> i32 {
    i32::from(exit_received && !shutdown_received)
}

/// A pass-through `tower` service that records the client's `shutdown`
/// request and signals when its `exit` notification has been handed to
/// the inner `tower-lsp` service. `tower-lsp` itself only stops serving
/// when the *next* message is read after `exit`, so without this the
/// server lingers while stdin is open. `shutdown` is recorded here, by
/// method, because `tower-lsp` answers a `shutdown` carrying
/// `"params": null` with an error without reaching the backend — the
/// client still asked to shut down.
struct ExitInterceptor<S> {
    inner: S,
    shutdown: Arc<AtomicBool>,
    exit: Option<tokio::sync::oneshot::Sender<()>>,
}

impl<S> tower_service::Service<Request> for ExitInterceptor<S>
where
    S: tower_service::Service<Request>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let is_exit = req.method() == "exit";
        if req.method() == "shutdown" {
            self.shutdown.store(true, Ordering::SeqCst);
        }
        let fut = self.inner.call(req);
        if is_exit && let Some(tx) = self.exit.take() {
            let _ = tx.send(());
        }
        fut
    }
}

#[derive(Debug, Default)]
struct State {
    /// Workspace folders, from the `initialize` handshake (and
    /// `didChangeWorkspaceFolders`).
    folders: Vec<PathBuf>,
    /// URIs of documents the editor currently has open. Diagnostics
    /// are published (and cleared) for these.
    open: HashSet<Url>,
    /// Cached engine + index per config path from the last full check.
    /// The change hot path and code actions need them.
    sessions: HashMap<PathBuf, Arc<Session>>,
    /// Configs that failed to load/build, with the error. Documents
    /// governed by a broken config show no findings (only the config
    /// error, on the config file) until it loads again.
    broken: HashMap<PathBuf, String>,
    /// Which config governs each open document (nearest ancestor).
    doc_config: HashMap<Url, PathBuf>,
    /// Config-file URIs we published diagnostics to (findings anchored
    /// there, or a config error) — cleared when no longer relevant.
    config_uris: HashSet<Url>,
    /// Last-published findings per URI, so `hover` can answer by
    /// position without re-running rules.
    diagnostics: HashMap<Url, Vec<Finding>>,
    /// In-memory text per open URI (the editor's authoritative buffer),
    /// so `codeAction` can compute a fix edit against unsaved content
    /// and full checks can overlay unsaved edits.
    documents: HashMap<Url, String>,
    /// The editor's version per open URI.
    versions: HashMap<Url, i32>,
    /// The last notes block printed per config, so an unchanged set
    /// isn't re-printed on every save.
    last_notes: HashMap<PathBuf, String>,
}

#[derive(Debug)]
struct Backend {
    client: Client,
    options: LspOptions,
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
    /// Serialises the "is this result still current?" check with the
    /// publish that follows it in [`Backend::reeval_file`], so an older
    /// buffer's diagnostics can never reach the client after a newer
    /// buffer's.
    reeval_publish: tokio::sync::Mutex<()>,
}

/// A document to publish: URI, diagnostics, and the buffer version they
/// were computed against (if any).
type Publication = (Url, Vec<Diagnostic>, Option<i32>);

/// What applying a [`WorkspaceCheck`] to the state produced.
#[derive(Debug, Default)]
struct Applied {
    publish: Vec<Publication>,
    /// Config errors to log to the client.
    logs: Vec<String>,
    /// Notes blocks to print on stderr.
    notes: Vec<String>,
    /// Open documents edited while the full check ran: re-evaluate
    /// their current buffer afterwards so the newest edit wins.
    reeval: Vec<(Url, String, i32)>,
}

impl Backend {
    fn new(client: Client, options: LspOptions) -> Self {
        Self {
            client,
            options,
            state: Mutex::new(State::default()),
            reeval_publish: tokio::sync::Mutex::new(()),
        }
    }

    /// Full check: (re)build every relevant session and publish per-file
    /// diagnostics for every open document (plus each config file),
    /// clearing those that no longer have findings. Runs on open, save,
    /// watched-file changes, and workspace-folder changes.
    async fn check_and_publish(&self) {
        let (folders, docs) = {
            let state = self.state.lock();
            let docs: Vec<(Url, Option<String>, Option<i32>)> = state
                .open
                .iter()
                .map(|u| {
                    (
                        u.clone(),
                        state.documents.get(u).cloned(),
                        state.versions.get(u).copied(),
                    )
                })
                .collect();
            (state.folders.clone(), docs)
        };
        if folders.is_empty() {
            return;
        }

        let task_docs: Vec<(Url, Option<String>)> = docs
            .iter()
            .map(|(u, t, _)| (u.clone(), t.clone()))
            .collect();
        let check = match tokio::task::spawn_blocking(move || check_workspace(&folders, &task_docs))
            .await
        {
            Ok(check) => check,
            Err(join_err) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("alint: check panicked: {join_err}"),
                    )
                    .await;
                return;
            }
        };

        let applied = {
            let mut state = self.state.lock();
            apply_check(&mut state, &docs, check, self.options)
        };
        for message in applied.logs {
            self.client
                .log_message(MessageType::WARNING, format!("alint: {message}"))
                .await;
        }
        for block in applied.notes {
            eprintln!("{block}");
        }
        self.publish_all(applied.publish).await;
        for (uri, text, version) in applied.reeval {
            self.reeval_file(uri, text, version).await;
        }
    }

    /// Single-file hot path: re-evaluate per-file rules against the
    /// editor's in-memory `text` and publish diagnostics for just this
    /// document. Per-file findings replace the previous per-file ones,
    /// but cached cross-file findings (from the last full run) are
    /// preserved so they don't flicker away while typing — they refresh
    /// on the next save. `version` ties the diagnostics to the edit.
    async fn reeval_file(&self, uri: Url, text: String, version: i32) {
        let session = {
            let state = self.state.lock();
            state
                .doc_config
                .get(&uri)
                .filter(|c| !state.broken.contains_key(*c))
                .and_then(|c| state.sessions.get(c).cloned())
        };
        let Some(session) = session else {
            // No session yet (open/save will populate it), or the
            // governing config is broken (only its error is shown).
            return;
        };
        let Ok(abs) = uri.to_file_path() else {
            return;
        };
        let Ok(rel) = abs.strip_prefix(&session.root).map(Path::to_path_buf) else {
            return;
        };

        let outcome =
            tokio::task::spawn_blocking(move || eval_buffer(&session, &abs, &rel, text)).await;

        // tower-lsp runs `didChange` handlers concurrently, so a slow
        // evaluation of an older buffer can finish after a newer one. Drop
        // a result whose version is no longer the document's current one
        // (or whose document closed): publishing or caching it would leave
        // stale diagnostics on screen and in the state hover / code
        // actions read.
        let is_current = |state: &State| state.versions.get(&uri).copied() == Some(version);
        let _publish_guard = self.reeval_publish.lock().await;

        match outcome {
            Ok(Ok(per_file)) => {
                let diagnostics = {
                    let mut state = self.state.lock();
                    if !is_current(&state) {
                        return;
                    }
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
                {
                    let mut state = self.state.lock();
                    if !is_current(&state) {
                        return;
                    }
                    state.diagnostics.remove(&uri);
                }
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

    /// The governing session, cached findings, and buffer for `uri` —
    /// everything `codeAction` needs, or `None` when any is missing.
    fn code_action_inputs(&self, uri: &Url) -> Option<(Arc<Session>, Vec<Finding>, String)> {
        let state = self.state.lock();
        let session = state
            .doc_config
            .get(uri)
            .and_then(|c| state.sessions.get(c).cloned())?;
        Some((
            session,
            state.diagnostics.get(uri).cloned()?,
            state.documents.get(uri).cloned()?,
        ))
    }

    async fn publish_all(&self, items: Vec<Publication>) {
        for (uri, diagnostics, version) in items {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> JsonRpcResult<InitializeResult> {
        self.state.lock().folders = workspace_folders(&params);
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
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
            let uri = params.text_document.uri;
            state.open.insert(uri.clone());
            state
                .versions
                .insert(uri.clone(), params.text_document.version);
            state.documents.insert(uri, params.text_document.text);
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
        {
            let mut state = self.state.lock();
            state.documents.insert(uri.clone(), change.text.clone());
            state.versions.insert(uri.clone(), version);
        }
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

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        {
            let mut state = self.state.lock();
            let removed: Vec<PathBuf> = params
                .event
                .removed
                .iter()
                .filter_map(|f| f.uri.to_file_path().ok())
                .collect();
            state.folders.retain(|f| !removed.contains(f));
            for added in &params.event.added {
                if let Ok(path) = added.uri.to_file_path()
                    && !state.folders.contains(&path)
                {
                    state.folders.push(path);
                }
            }
        }
        self.check_and_publish().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let is_config = {
            let mut state = self.state.lock();
            state.open.remove(&uri);
            state.documents.remove(&uri);
            state.versions.remove(&uri);
            state.doc_config.remove(&uri);
            let is_config = state.config_uris.contains(&uri);
            if !is_config {
                state.diagnostics.remove(&uri);
            }
            is_config
        };
        // Clear any diagnostics the editor is still showing — except on a
        // config file, whose anchored findings / load error stay visible
        // whether or not it is open.
        if !is_config {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
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

        let Some((session, findings, text)) = self.code_action_inputs(&uri) else {
            return Ok(None);
        };
        let Ok(abs) = uri.to_file_path() else {
            return Ok(None);
        };
        let Ok(rel) = abs.strip_prefix(&session.root).map(Path::to_path_buf) else {
            return Ok(None);
        };

        let bytes = text.as_bytes();
        // Honor the config's `fix_size_limit`: `alint fix` skips a file
        // this large, so the editor must not offer to fix it either.
        if let Some(limit) = session.engine.fix_size_limit()
            && u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit
        {
            return Ok(None);
        }
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
        if actions.len() == 1
            && !unsafe_flags[0]
            && let CodeActionOrCommand::CodeAction(action) = &mut actions[0]
        {
            action.is_preferred = Some(true);
        }
        Ok(Some(actions))
    }
}

/// Fold a finished [`WorkspaceCheck`] into the server state and work out
/// what to publish. `docs` is the `(uri, buffer, version)` snapshot the
/// check ran against.
///
/// Every open document gets the findings of the config that governs it;
/// a document under a broken config gets none (the config error is
/// shown on the config file instead of stale findings from the old
/// engine). Each config file gets its own anchored findings — published
/// in the SAME message, so clearing a previous config error can never
/// wipe them.
fn apply_check(
    state: &mut State,
    docs: &[(Url, Option<String>, Option<i32>)],
    check: WorkspaceCheck,
    options: LspOptions,
) -> Applied {
    let mut applied = Applied::default();
    let mut by_config: HashMap<PathBuf, FindingsByPath> = HashMap::new();
    state.sessions.clear();
    state.broken.clear();
    for (config, run) in check.runs {
        match run {
            Ok(run) => {
                let block = render_notes(&config, &run.notes, options.show_notes);
                if state.last_notes.get(&config) != Some(&block) {
                    if !block.is_empty() {
                        applied.notes.push(block.clone());
                    }
                    state.last_notes.insert(config.clone(), block);
                }
                state.sessions.insert(config.clone(), run.session);
                by_config.insert(config, run.by_path);
            }
            Err(message) => {
                applied.logs.push(message.clone());
                state.broken.insert(config, message);
            }
        }
    }
    state.doc_config = check.doc_config;

    let mut published: HashSet<Url> = HashSet::new();
    for (uri, _, snapshot_version) in docs {
        if !state.open.contains(uri) {
            continue; // closed while the check ran
        }
        let findings = uri
            .to_file_path()
            .ok()
            .and_then(|abs| {
                let config = state.doc_config.get(uri)?;
                by_config.get(config)?.get(&abs).cloned()
            })
            .unwrap_or_default();
        let diagnostics = findings.iter().map(finding_to_diagnostic).collect();
        state.diagnostics.insert(uri.clone(), findings);
        published.insert(uri.clone());
        applied
            .publish
            .push((uri.clone(), diagnostics, *snapshot_version));
        // The buffer moved on while the check ran: re-evaluate the
        // current text so the newest edit's per-file findings win.
        let current = state.versions.get(uri).copied();
        if current != *snapshot_version
            && let (Some(version), Some(text)) = (current, state.documents.get(uri))
        {
            applied.reeval.push((uri.clone(), text.clone(), version));
        }
    }

    // Config files: anchored findings, or the load/build error.
    let mut config_uris: HashSet<Url> = HashSet::new();
    let configs: Vec<PathBuf> = state
        .sessions
        .keys()
        .chain(state.broken.keys())
        .cloned()
        .collect();
    for config in configs {
        let Ok(uri) = Url::from_file_path(&config) else {
            continue;
        };
        let (findings, diagnostics) = if let Some(message) = state.broken.get(&config) {
            let diagnostic = Diagnostic {
                range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("alint".to_string()),
                message: message.clone(),
                ..Diagnostic::default()
            };
            (Vec::new(), vec![diagnostic])
        } else {
            let findings = by_config
                .get(&config)
                .and_then(|m| m.get(&config).cloned())
                .unwrap_or_default();
            let diagnostics = findings.iter().map(finding_to_diagnostic).collect();
            (findings, diagnostics)
        };
        state.diagnostics.insert(uri.clone(), findings);
        let version = state.versions.get(&uri).copied();
        if published.insert(uri.clone()) {
            applied.publish.push((uri.clone(), diagnostics, version));
        } else if let Some(entry) = applied.publish.iter_mut().find(|(u, _, _)| *u == uri) {
            // An open config file: its governing config is itself, so the
            // findings agree; a broken config's error replaces them.
            entry.1 = diagnostics;
        }
        config_uris.insert(uri);
    }
    // Config files we published to before that are no longer relevant.
    let no_longer_configs: Vec<Url> = state
        .config_uris
        .difference(&config_uris)
        .filter(|u| !state.open.contains(*u))
        .cloned()
        .collect();
    for uri in no_longer_configs {
        state.diagnostics.remove(&uri);
        applied.publish.push((uri, Vec::new(), None));
    }
    state.config_uris = config_uris;
    applied
}

/// Run every config relevant to the workspace: the config each open
/// document resolves to, plus each workspace folder's own config (so a
/// config error surfaces even when the open file lives elsewhere).
fn check_workspace(folders: &[PathBuf], docs: &[(Url, Option<String>)]) -> WorkspaceCheck {
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
fn config_for_document(doc: &Path, folders: &[PathBuf]) -> Option<PathBuf> {
    if !folders.iter().any(|f| doc.starts_with(f)) {
        return None;
    }
    alint_dsl::discover(doc.parent()?)
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
fn eval_buffer(
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
fn workspace_folders(params: &InitializeParams) -> Vec<PathBuf> {
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
fn build_session(start: &Path) -> Result<Option<Session>, String> {
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

/// Document text for UTF-16 column conversion: the editor's buffer for
/// open documents (what was linted), else the file on disk (cached).
#[derive(Debug, Default)]
struct TextSource {
    overlays: HashMap<PathBuf, String>,
    disk: HashMap<PathBuf, Option<String>>,
}

impl TextSource {
    fn with_overlays(overlays: HashMap<PathBuf, String>) -> Self {
        Self {
            overlays,
            disk: HashMap::new(),
        }
    }

    fn get(&mut self, abs: &Path) -> Option<&str> {
        if self.overlays.contains_key(abs) {
            return self.overlays.get(abs).map(String::as_str);
        }
        self.disk
            .entry(abs.to_path_buf())
            .or_insert_with(|| {
                let size = std::fs::metadata(abs).ok()?.len();
                let bytes = alint_core::read_capped_or_skip(abs, size)?;
                Some(String::from_utf8_lossy(&bytes).into_owned())
            })
            .as_deref()
    }
}

/// What [`group_findings`] needs from a session.
struct GroupCtx<'a> {
    root: &'a Path,
    engine: &'a Engine,
    config_path: &'a Path,
    kinds: &'a HashMap<String, String>,
}

/// Group rule-result violations into per-file findings keyed by absolute
/// path. Path-less findings (existence / tree-level rules) are anchored
/// to the config file so they're still visible in the editor. Each
/// finding is tagged `per_file` so the change hot path can preserve
/// cross-file findings. Columns are converted to UTF-16 against the
/// document text from `texts`.
fn group_findings(
    ctx: &GroupCtx<'_>,
    results: &[RuleResult],
    texts: &mut TextSource,
) -> FindingsByPath {
    let mut by_path = FindingsByPath::new();
    for result in results {
        let Some(severity) = severity_of(result.level) else {
            continue;
        };
        let policy_url = result.policy_url.as_ref().map(ToString::to_string);
        let per_file = ctx.engine.is_per_file(&result.rule_id);
        let kind = ctx.kinds.get(result.rule_id.as_ref());
        let description = kind.and_then(|k| kind_description(k));
        let docs_url = kind.and_then(|k| rule_docs_url(k));
        for violation in &result.violations {
            // Anchor path-less (tree/file-level) findings to the config
            // file so a "missing required file" still shows somewhere.
            let abs = match &violation.path {
                Some(rel) => ctx.root.join(rel.as_ref()),
                None => ctx.config_path.to_path_buf(),
            };
            let text = if violation.path.is_some() && violation.column.is_some() {
                texts.get(&abs)
            } else {
                None
            };
            let range = violation_range(violation, text);
            by_path.entry(abs).or_default().push(Finding {
                range,
                severity,
                rule_id: result.rule_id.to_string(),
                message: violation.message.to_string(),
                line: violation.line,
                column: violation.column,
                policy_url: policy_url.clone(),
                fixable: result.is_fixable,
                per_file,
                description,
                docs_url: docs_url.clone(),
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

/// alint line/column are 1-indexed and optional, and the column counts
/// Unicode scalar values (`char`s); LSP positions are 0-indexed with the
/// character offset in UTF-16 code units (the protocol default). With
/// the document `text`, the column is converted exactly (an emoji
/// before the marker counts as two units) and the range spans the
/// marked character's full UTF-16 width. Without text it falls back to
/// the raw column. File- and tree-level findings (no line) anchor at
/// the start of the file. The range is one character wide so the editor
/// has something to attach the marker (and hover) to.
fn violation_range(violation: &Violation, text: Option<&str>) -> Range {
    let line_idx = violation.line.map_or(0, |l| l.saturating_sub(1));
    let line = u32::try_from(line_idx).unwrap_or(0);
    let (col, width) = match violation.column {
        None => (0, 1),
        Some(column) => match text.and_then(|t| t.split('\n').nth(line_idx)) {
            Some(line_text) => utf16_column(line_text, column),
            None => (u32::try_from(column.saturating_sub(1)).unwrap_or(0), 1),
        },
    };
    Range::new(
        Position::new(line, col),
        Position::new(line, col.saturating_add(width)),
    )
}

/// Convert a 1-based `char` column within `line_text` to a 0-based
/// UTF-16 offset, plus the UTF-16 width of the character there (1 past
/// the end of the line).
fn utf16_column(line_text: &str, column: usize) -> (u32, u32) {
    let skip = column.saturating_sub(1);
    let mut chars = line_text.chars();
    let mut units: usize = 0;
    let mut consumed = 0usize;
    for c in chars.by_ref().take(skip) {
        units += c.len_utf16();
        consumed += 1;
    }
    // A column past the end of the line: one unit per missing char.
    units += skip - consumed;
    let width = chars.next().map_or(1, char::len_utf16);
    (
        u32::try_from(units).unwrap_or(u32::MAX),
        u32::try_from(width).unwrap_or(1),
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
/// message, the rule kind's description, fix availability, and links to
/// the rule reference and (when declared) the rule's policy.
fn render_finding(f: &Finding) -> String {
    let mut s = format!(
        "**alint** · `{}` ({})\n\n{}",
        f.rule_id,
        severity_label(f.severity),
        f.message
    );
    if let Some(description) = f.description {
        s.push_str("\n\n");
        s.push_str(description);
    }
    s.push_str(if f.fixable {
        "\n\nFix: available (quick-fix, or `alint fix`)"
    } else {
        "\n\nFix: none (this rule has no auto-fix)"
    });
    let mut links: Vec<String> = Vec::new();
    if let Some(url) = &f.docs_url {
        links.push(format!("[Rule reference →]({url})"));
    }
    if let Some(url) = &f.policy_url {
        links.push(format!("[Policy →]({url})"));
    }
    if !links.is_empty() {
        s.push_str("\n\n");
        s.push_str(&links.join(" · "));
    }
    s
}

/// Resolve an alias kind spelling to its canonical kind (identity for a
/// canonical or unknown kind) — the kind that owns the reference page.
fn canonical_kind(kind: &str) -> &str {
    alint_rules::categories::ALIAS_TO_CANONICAL
        .iter()
        .find(|(alias, _)| *alias == kind)
        .map_or(kind, |(_, canonical)| canonical)
}

/// The alint.org rule-reference page for a rule kind — the same URL
/// `alint explain` prints (family = the kind's primary category).
fn rule_docs_url(kind: &str) -> Option<String> {
    let canonical = canonical_kind(kind);
    let family = alint_rules::categories::KIND_CATEGORIES
        .iter()
        .find(|(k, _)| *k == canonical)
        .and_then(|(_, cats)| cats.first())?
        .slug();
    Some(format!(
        "https://alint.org/docs/rules/{family}/{canonical}/"
    ))
}

/// The rule kind's one-sentence description (as `alint explain` shows).
fn kind_description(kind: &str) -> Option<&'static str> {
    let canonical = canonical_kind(kind);
    alint_rules::kind_docs::KIND_DESCRIPTIONS
        .iter()
        .find(|(k, _)| *k == canonical)
        .map(|(_, d)| *d)
        .filter(|d| !d.is_empty())
}

/// The stderr block for a config's informational notes: a one-line
/// count by default, the full list with `--show-notes`. Empty when there
/// are no notes. Control characters are escaped so a note can't inject
/// terminal escapes into a log.
fn render_notes(config: &Path, notes: &[String], show_notes: bool) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let config = escape_controls(&config.display().to_string());
    if show_notes {
        let mut block = format!("alint: {} informational note(s) ({config}):", notes.len());
        for note in notes {
            block.push_str("\n  note: ");
            block.push_str(&escape_controls(note));
        }
        block
    } else {
        format!(
            "alint: {} informational note(s) ({config}); run `alint lsp --show-notes` to list.",
            notes.len()
        )
    }
}

/// Escape control characters (C0/C1, incl. ESC) as `\u{..}`.
fn escape_controls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn test_backend(client: Client) -> Backend {
        Backend::new(client, LspOptions::default())
    }

    /// Wire a built session into the state as the governing config of `uri`.
    fn install_session(st: &mut State, root: &Path, uri: &Url, session: Session) {
        st.folders = vec![root.to_path_buf()];
        st.doc_config
            .insert(uri.clone(), session.config_path.clone());
        st.sessions
            .insert(session.config_path.clone(), Arc::new(session));
    }

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
            description: None,
            docs_url: None,
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
        let r = violation_range(&violation(Some(4), Some(7)), None);
        assert_eq!(r.start, Position::new(3, 6));
        assert_eq!(r.end, Position::new(3, 7));
    }

    #[test]
    fn violation_range_without_line_anchors_at_file_start() {
        let r = violation_range(&violation(None, None), None);
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
    fn build_session_preserves_expect_matches_findings() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join(".alint.yml");
        std::fs::write(
            &config_path,
            r"version: 1
rules:
  - id: required-doc
    kind: file_content_forbidden
    paths: required.md
    pattern: stale
    expect_matches: true
    level: error
",
        )
        .unwrap();

        let session = build_session(dir.path())
            .expect("build_session succeeds")
            .expect("config present");
        let report = session
            .engine
            .run(&session.root, &session.index)
            .expect("scope assertion evaluates");
        assert_eq!(report.results.len(), 1, "{report:?}");
        assert_eq!(report.results[0].violations.len(), 1, "{report:?}");
        assert!(
            report.results[0].violations[0]
                .message
                .contains("matched none"),
            "{report:?}"
        );

        let by_path = group_findings(
            &session.group_ctx(),
            &report.results,
            &mut TextSource::default(),
        );
        assert_eq!(by_path[&config_path].len(), 1, "{by_path:?}");
        assert!(!by_path[&config_path][0].fixable);
    }

    #[test]
    fn build_session_rejects_invalid_deeply_nested_rule() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".alint.yml"),
            r#"version: 1
rules:
  - id: outer
    kind: for_each_dir
    select: packages/*
    require:
      - kind: for_each_dir
        select: "{path}/*"
        require:
          - kind: file_exists
            paths: "{path}/README.md"
            expect_matches: true
    level: error
"#,
        )
        .unwrap();

        let error = build_session(dir.path()).expect_err("invalid descendant must fail loading");
        assert!(error.contains("expect_matches"), "{error}");
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
        let kinds = HashMap::new();
        let ctx = GroupCtx {
            root: &root,
            engine: &engine,
            config_path: &config,
            kinds: &kinds,
        };
        let by_path = group_findings(&ctx, &results, &mut TextSource::default());
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
        let kinds = HashMap::new();
        let ctx = GroupCtx {
            root: &root,
            engine: &engine,
            config_path: &config,
            kinds: &kinds,
        };
        let by_path = group_findings(&ctx, &results, &mut TextSource::default());
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

        let (service, _socket) = LspService::new(test_backend);
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
            description: None,
            docs_url: None,
        };
        {
            let mut st = backend.state.lock();
            install_session(&mut st, &root, &uri, session);
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
        let (service, _socket) = LspService::new(test_backend);
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
            description: None,
            docs_url: None,
        };
        {
            let mut st = backend.state.lock();
            install_session(&mut st, &root, &uri, session);
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
        let (service, _socket) = LspService::new(test_backend);
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
            description: None,
            docs_url: None,
        };
        {
            let mut st = backend.state.lock();
            install_session(&mut st, &root, &uri, session);
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
        let (service, _socket) = LspService::new(test_backend);
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
            description: None,
            docs_url: None,
        };
        {
            let mut st = backend.state.lock();
            install_session(&mut st, &root, &uri, session);
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

    #[test]
    fn violation_range_converts_char_columns_to_utf16() {
        // Audit HIGH: alint columns count `char`s; LSP positions are UTF-16
        // units. Two emoji (2 units each) + 'x' put the ZWSP (char column 4)
        // at UTF-16 character 5 — not 3.
        let text = "\u{1F600}\u{1F600}x\u{200B}y\n";
        let r = violation_range(&violation(Some(1), Some(4)), Some(text));
        assert_eq!(r.start, Position::new(0, 5));
        assert_eq!(r.end, Position::new(0, 6));
        // A marker ON a non-BMP char spans both of its units.
        let r = violation_range(&violation(Some(1), Some(2)), Some(text));
        assert_eq!((r.start, r.end), (Position::new(0, 2), Position::new(0, 4)));
        // Second line, CRLF-free; a column past EOL stays one unit per char.
        let r = violation_range(&violation(Some(2), Some(3)), Some("ab\n\u{1F600}"));
        assert_eq!(r.start, Position::new(1, 3));
        assert_eq!(utf16_column("ab", 5), (4, 1));
    }

    #[test]
    fn render_finding_shows_description_fix_availability_and_docs_link() {
        let mut f = finding(None);
        f.fixable = true;
        f.description = kind_description("no_zero_width_chars");
        f.docs_url = rule_docs_url("no_zero_width_chars");
        let md = render_finding(&f);
        assert!(md.contains("Fix: available"), "{md}");
        assert!(
            md.contains("https://alint.org/docs/rules/") && md.contains("/no_zero_width_chars/"),
            "{md}"
        );
        assert!(f.description.is_some_and(|d| md.contains(d)), "{md}");
        f.fixable = false;
        assert!(render_finding(&f).contains("Fix: none"));
    }

    #[test]
    fn rule_docs_url_resolves_aliases_to_the_canonical_page() {
        let (alias, canonical) = alint_rules::categories::ALIAS_TO_CANONICAL[0];
        let url = rule_docs_url(alias).expect("alias has a docs page");
        assert!(url.ends_with(&format!("/{canonical}/")), "{url}");
        assert!(rule_docs_url("no-such-kind").is_none());
    }

    #[test]
    fn render_notes_counts_by_default_and_lists_with_show_notes() {
        let config = Path::new("/r/.alint.yml");
        assert_eq!(render_notes(config, &[], true), "");
        let notes = vec!["a.txt: skipped \u{1b}[31m${MODULE}".to_string()];
        let short = render_notes(config, &notes, false);
        assert!(short.contains("1 informational note(s)"), "{short}");
        assert!(short.contains("--show-notes"), "{short}");
        let long = render_notes(config, &notes, true);
        assert!(long.contains("note: a.txt: skipped"), "{long}");
        assert!(!long.contains('\u{1b}'), "control chars escaped: {long:?}");
    }

    #[test]
    fn exit_code_follows_the_lsp_spec() {
        assert_eq!(exit_code(true, true), 0, "exit after shutdown");
        assert_eq!(exit_code(true, false), 1, "exit without shutdown");
        assert_eq!(exit_code(false, false), 0, "stream closed");
    }

    #[test]
    fn workspace_folders_keeps_every_root() {
        use tower_lsp::lsp_types::WorkspaceFolder;
        let a = repo_root().join("a");
        let b = repo_root().join("b");
        let params = InitializeParams {
            workspace_folders: Some(vec![
                WorkspaceFolder {
                    uri: Url::from_file_path(&a).unwrap(),
                    name: "a".into(),
                },
                WorkspaceFolder {
                    uri: Url::from_file_path(&b).unwrap(),
                    name: "b".into(),
                },
            ]),
            ..InitializeParams::default()
        };
        assert_eq!(workspace_folders(&params), vec![a, b]);
    }

    #[test]
    fn config_for_document_ignores_files_outside_every_folder() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".alint.yml"), "version: 1\n").unwrap();
        let doc = tmp.path().join("a.txt");
        assert_eq!(
            config_for_document(&doc, &[tmp.path().to_path_buf()]),
            Some(tmp.path().join(".alint.yml"))
        );
        assert_eq!(config_for_document(&doc, &[repo_root()]), None);
    }

    #[tokio::test]
    async fn code_action_withholds_fixes_over_the_fix_size_limit() {
        // The LSP ignored `fix_size_limit`: `alint fix` skips an oversized
        // file, so the editor must not offer to fix it either.
        use tower_lsp::lsp_types::{
            CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
        };
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::write(
            root.join(".alint.yml"),
            "version: 1\nfix_size_limit: 4\nrules:\n  - id: no-todo\n    \
             kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: \"TODO\"\n    \
             level: error\n    fix: { replace: { replacement: \"DONE\" } }\n",
        )
        .unwrap();
        let session = build_session(&root).unwrap().unwrap();
        assert_eq!(session.engine.fix_size_limit(), Some(4));
        let (service, _socket) = LspService::new(test_backend);
        let backend = service.inner();
        let uri = Url::from_file_path(root.join("a.txt")).unwrap();
        let mut f = finding(None);
        f.rule_id = "no-todo".to_string();
        f.fixable = true;
        f.range = Range::new(Position::new(0, 2), Position::new(0, 3));
        {
            let mut st = backend.state.lock();
            install_session(&mut st, &root, &uri, session);
            st.open.insert(uri.clone());
            st.documents.insert(uri.clone(), "x TODO\n".to_string());
            st.diagnostics.insert(uri.clone(), vec![f]);
        }
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range::new(Position::new(0, 2), Position::new(0, 3)),
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let resp = backend.code_action(params).await.expect("code_action ok");
        assert!(resp.is_none(), "oversized buffer gets no fix: {resp:?}");
    }

    #[test]
    fn build_session_rejects_a_missing_baseline() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".alint.yml"),
            "version: 1\nbaseline: missing.json\nrules: []\n",
        )
        .unwrap();
        let err = build_session(dir.path()).expect_err("missing baseline is a config error");
        assert!(err.contains("baseline"), "{err}");
    }
}
