//! Language Server Protocol server for alint.
//!
//! A thin `tower-lsp` backend that runs the alint engine over the
//! workspace and publishes the resulting violations as LSP diagnostics.
//! It is driven by the `alint lsp` subcommand, speaking LSP over stdio
//! (see [`run_stdio`]).
//!
//! Config discovery is **per workspace folder**: each folder the client
//! reports (multi-root) is linted by its own `.alint.yml`, rooted at that
//! config's directory, as `alint check` there. A folder without a config of
//! its own (a directory holding several repositories) lints each file with
//! the config at the top of the nearest repository above it inside the
//! folder, rooted there. A deeper config joins only as a nested config,
//! under the governing config's `nested_configs: true`, and a file outside
//! every folder is not linted.
//!
//! Until the client says the workspace is trusted (the `trusted`
//! initialization option), the server runs no rule, fix or fact that
//! starts a process or runs Git, fetches no remote `extends:` and reads no
//! environment value; every other rule lints as usual.
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
//! Workspace trust: a workspace's config is the repository's, so until the
//! client vouches for the workspace (the `trusted: true` initialization
//! option; the VS Code extension sends its Workspace Trust state) the server
//! builds no rule or fact that spawns a process or runs Git, fetches no remote
//! `extends:`, and reads nothing from the environment. Every other rule lints
//! as usual, and the user is told once what was left out (see
//! `config::Settings`).
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
    CodeActionProviderCapability, CodeActionResponse, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams, Hover,
    HoverContents, HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
    InitializedParams, MarkupContent, MarkupKind, MessageType, OneOf, Position, Range,
    ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, jsonrpc::Result as JsonRpcResult};

use alint_core::baseline::Baseline;
use alint_core::located_fix::{self, LocatedEdit, LocatedOutcome};
use alint_core::{Applicability, CollectedEdit, Engine, Error, FileIndex, Violation};

mod config;
mod diagnostics;
mod edits;
mod render;

#[cfg(test)]
mod registry_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod trust_tests;

use config::{ConfigError, SessionKey, Settings, check_workspace, eval_buffer, workspace_folders};
use diagnostics::{GroupCtx, finding_to_diagnostic, range_contains, ranges_overlap};
use edits::{fix_edit_to_workspace_edit, located_edits_to_workspace_edit};
use render::{render_finding, render_notes};

/// Server options, set from the `alint lsp` command line.
///
/// Built with [`LspOptions::default`] and the `with_*` setters, so a later
/// field is an addition rather than a breaking change. The fields stay public,
/// to read and to assign.
// Each bool is a command-line flag.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct LspOptions {
    /// `--show-notes`: list every informational note on stderr after a
    /// full check, instead of the default one-line count.
    pub show_notes: bool,
    /// Explicit reference date shared by full checks and buffer re-evaluation.
    pub analysis_time: Option<alint_core::AnalysisTime>,
    /// Injected rule registry, or the built-in catalog when `None`.
    pub registry: Option<alint_core::RuleRegistry>,
    /// `--config`: lint every document with this config (an absolute path),
    /// rooted at its workspace folder, in place of the folder's own config.
    /// The user chose it, so it runs trusted, as `alint check --config` does,
    /// whatever the client says about the workspace.
    pub config: Option<PathBuf>,
    /// `--no-gitignore`: walk without `.gitignore` handling.
    pub no_gitignore: bool,
    /// `--no-spawn`: a config with a rule, fix or fact that spawns a process
    /// fails to load, as with `alint check --no-spawn`.
    pub no_spawn: bool,
    /// `--untrusted`: every workspace is untrusted, whatever the client says
    /// and even with `--config`.
    pub untrusted: bool,
}

impl LspOptions {
    /// Sets [`Self::show_notes`].
    #[must_use]
    pub fn with_show_notes(mut self, show_notes: bool) -> Self {
        self.show_notes = show_notes;
        self
    }

    /// Sets [`Self::analysis_time`].
    #[must_use]
    pub fn with_analysis_time(mut self, analysis_time: Option<alint_core::AnalysisTime>) -> Self {
        self.analysis_time = analysis_time;
        self
    }

    /// Sets [`Self::registry`].
    #[must_use]
    pub fn with_registry(mut self, registry: Option<alint_core::RuleRegistry>) -> Self {
        self.registry = registry;
        self
    }

    /// Sets [`Self::config`].
    #[must_use]
    pub fn with_config(mut self, config: Option<PathBuf>) -> Self {
        self.config = config;
        self
    }

    /// Sets [`Self::no_gitignore`].
    #[must_use]
    pub fn with_no_gitignore(mut self, no_gitignore: bool) -> Self {
        self.no_gitignore = no_gitignore;
        self
    }

    /// Sets [`Self::no_spawn`].
    #[must_use]
    pub fn with_no_spawn(mut self, no_spawn: bool) -> Self {
        self.no_spawn = no_spawn;
        self
    }

    /// Sets [`Self::untrusted`].
    #[must_use]
    pub fn with_untrusted(mut self, untrusted: bool) -> Self {
        self.untrusted = untrusted;
        self
    }

    fn registry(&self) -> alint_core::RuleRegistry {
        self.registry
            .clone()
            .unwrap_or_else(alint_rules::builtin_registry)
    }

    /// The session settings for a workspace the client did (`trusted`) or
    /// did not vouch for.
    fn settings(&self, trusted: bool) -> Settings {
        Settings {
            analysis_time: self.analysis_time,
            trusted: !self.untrusted && (trusted || self.config.is_some()),
            no_spawn: self.no_spawn,
            no_gitignore: self.no_gitignore,
            config: self.config.clone(),
        }
    }
}

/// The types alint-lsp seals, as a crate that depends on it sees them
/// (ADR-0046 decision 6).
///
/// The first block must compile: it uses the constructor, a setter and a
/// field. The second must fail to compile: it does the same with a functional
/// record update, which a sealed struct refuses. With the first block
/// compiling, the seal is the only reason the second can fail, so a second
/// block that compiles means the type lost `#[non_exhaustive]`.
///
/// ```
/// let mut options = alint_lsp::LspOptions::default().with_no_spawn(true);
/// options.untrusted = true;
/// ```
///
/// ```compile_fail
/// let _ = alint_lsp::LspOptions { untrusted: true, ..alint_lsp::LspOptions::default() };
/// ```
#[cfg(doctest)]
pub struct SealedTypes;

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
    /// Sensitive findings expose diagnostics but never public replacement bytes.
    sensitive: bool,
    /// Whether this came from a per-file rule (re-evaluated on every
    /// edit) vs a cross-file rule (only on save). On `didChange` we keep
    /// the cached cross-file findings and replace only the per-file ones,
    /// so cross-file markers don't flicker away while typing.
    per_file: bool,
    /// The rule kind's one-sentence description, for the hover.
    description: Option<&'static str>,
    /// The rule kind's alint.org reference page, for the hover.
    docs_url: Option<String>,
    related_information: Vec<tower_lsp::lsp_types::DiagnosticRelatedInformation>,
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
    /// Rule id → canonical rule kind (aliases resolved through the session's
    /// registry), for the hover's description + docs link.
    kinds: HashMap<String, String>,
    /// Load-time pattern hints, shown as warnings on the config file.
    pattern_hints: Vec<alint_core::pattern_hints::ConfigHint>,
    /// The config's [`alint_core::Config::environment_values`], removed from
    /// any error the session reports (a rule run that fails on an `ignore:`
    /// or `facts:` value read from the environment).
    environment_values: Vec<String>,
    /// The session is untrusted and left out something this config uses: a
    /// rule or fact that spawns, or an environment value (see
    /// [`config::UNTRUSTED_NOTE`]).
    withheld: bool,
    /// The ids of the rules an `extends:`'d ruleset or a nested config shaped
    /// ([`alint_core::RuleSpec::extended_source`]): their code actions never
    /// edit alint's configuration, as `alint fix` never writes it for them.
    extended_rules: HashSet<String>,
    /// The resolved local files the configuration came from
    /// ([`alint_core::Config::config_files`]).
    config_files: Vec<PathBuf>,
}

impl Session {
    /// `message` without the config's environment values.
    fn redact(&self, message: &str) -> String {
        alint_core::redact_environment_values(message, &self.environment_values).into_owned()
    }

    /// Why a code action of `rule_id` may not edit `path` (root-relative), as
    /// [`alint_core::FixContext::write_refusal`] refuses an `alint fix` write:
    /// a path in a Git directory, and for a rule an `extends:`'d ruleset
    /// shaped, alint's own configuration.
    fn edit_refusal(&self, rule_id: &str, path: &Path) -> Option<String> {
        alint_core::FixContext::new(&self.root, false)
            .with_extended_source(self.extended_rules.contains(rule_id))
            .with_config_files(&self.config_files)
            .write_refusal(path)
    }

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
    doc_config: HashMap<Url, SessionKey>,
    runs: Vec<(SessionKey, Result<ConfigRun, ConfigError>)>,
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
    // Rules and fixers recurse over parsed documents; give the workers and
    // the blocking pool the stack a main thread has, not tokio's 2 MiB.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024)
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
    /// Whether the client accepts `Diagnostic.relatedInformation`.
    related_information_supported: bool,
    /// The client vouched for the workspace (`trusted: true` in its
    /// initialization options). See [`Settings::trusted`].
    trusted: bool,
    /// The untrusted-workspace note was shown (once per server).
    untrusted_note_shown: bool,
    /// URIs of documents the editor currently has open. Diagnostics
    /// are published (and cleared) for these.
    open: HashSet<Url>,
    /// Cached engine + index per session from the last full check.
    /// The change hot path and code actions need them.
    sessions: HashMap<SessionKey, Arc<Session>>,
    /// Configs that failed to load/build, with the error. Documents
    /// governed by a broken config show no findings (only the config
    /// error, on the config file) until it loads again.
    broken: HashMap<SessionKey, ConfigError>,
    /// Which session governs each open document (its workspace folder's).
    doc_config: HashMap<Url, SessionKey>,
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
    /// The last notes block printed per session, so an unchanged set
    /// isn't re-printed on every save.
    last_notes: HashMap<SessionKey, String>,
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
    /// engine runs off-lock inside `spawn_blocking`, which reads it (an
    /// `Arc`) to skip a buffer that a newer edit superseded while it waited.
    state: Arc<Mutex<State>>,
    /// Serialises the "is this result still current?" check with the
    /// publish that follows it in [`Backend::reeval_file`], so an older
    /// buffer's diagnostics can never reach the client after a newer
    /// buffer's.
    reeval_publish: tokio::sync::Mutex<()>,
    /// Buffer evaluations started, for the tests of `reeval_file`'s
    /// coalescing.
    #[cfg(test)]
    evaluations: Arc<std::sync::atomic::AtomicUsize>,
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
    /// Show the untrusted-workspace note ([`config::UNTRUSTED_NOTE`]).
    untrusted_note: bool,
}

impl Backend {
    fn new(client: Client, options: LspOptions) -> Self {
        Self {
            client,
            options,
            state: Arc::new(Mutex::new(State::default())),
            reeval_publish: tokio::sync::Mutex::new(()),
            #[cfg(test)]
            evaluations: Arc::default(),
        }
    }

    /// Full check: (re)build every relevant session and publish per-file
    /// diagnostics for every open document (plus each config file),
    /// clearing those that no longer have findings. Runs on open, save,
    /// watched-file changes, and workspace-folder changes.
    async fn check_and_publish(&self) {
        let (folders, docs, trusted) = {
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
            (state.folders.clone(), docs, state.trusted)
        };
        if folders.is_empty() {
            return;
        }

        let task_docs: Vec<(Url, Option<String>)> = docs
            .iter()
            .map(|(u, t, _)| (u.clone(), t.clone()))
            .collect();
        let settings = self.options.settings(trusted);
        let registry = self.options.registry();
        let check = match tokio::task::spawn_blocking(move || {
            check_workspace(&folders, &task_docs, &settings, &registry)
        })
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
            apply_check(&mut state, &docs, check, &self.options)
        };
        for message in applied.logs {
            self.client
                .log_message(MessageType::WARNING, format!("alint: {message}"))
                .await;
        }
        for block in applied.notes {
            eprintln!("{block}");
        }
        if applied.untrusted_note {
            self.client
                .show_message(MessageType::INFO, config::UNTRUSTED_NOTE)
                .await;
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
    ///
    /// Only the latest version of a document is evaluated: a version that a
    /// newer edit superseded is skipped before it starts, and again if it
    /// waited in the blocking pool while the newer edit arrived. Rapid typing
    /// therefore costs one evaluation per edit at most, and usually far fewer.
    async fn reeval_file(&self, uri: Url, text: String, version: i32) {
        let session = {
            let state = self.state.lock();
            if state.versions.get(&uri) != Some(&version) {
                // Superseded (its successor re-evaluates), or closed.
                return;
            }
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

        let shared = Arc::clone(&self.state);
        let task_uri = uri.clone();
        #[cfg(test)]
        let evaluations = Arc::clone(&self.evaluations);
        let outcome = tokio::task::spawn_blocking(move || {
            if shared.lock().versions.get(&task_uri) != Some(&version) {
                return None;
            }
            #[cfg(test)]
            evaluations.fetch_add(1, Ordering::SeqCst);
            Some(eval_buffer(&session, &abs, &rel, text))
        })
        .await;

        // tower-lsp runs `didChange` handlers concurrently, so a slow
        // evaluation of an older buffer can finish after a newer one. Drop
        // a result whose version is no longer the document's current one
        // (or whose document closed): publishing or caching it would leave
        // stale diagnostics on screen and in the state hover / code
        // actions read.
        let is_current = |state: &State| state.versions.get(&uri).copied() == Some(version);
        let _publish_guard = self.reeval_publish.lock().await;

        let per_file = match outcome {
            // Superseded while it waited for a blocking thread.
            Ok(None) => return,
            Ok(Some(Ok(per_file))) => per_file,
            // Excluded from the walk, or not walked yet: no per-file rule
            // applies, but a finding from the last full run still does. A
            // `git_tracked_only` rule reports a tracked file that an ignore
            // file hides (#503).
            Ok(Some(Err(Error::FileNotInIndex { .. }))) => Vec::new(),
            Ok(Some(Err(err))) => {
                self.client
                    .log_message(MessageType::WARNING, format!("alint: {err}"))
                    .await;
                return;
            }
            Err(join_err) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("alint: re-eval panicked: {join_err}"),
                    )
                    .await;
                return;
            }
        };
        let diagnostics = {
            let mut state = self.state.lock();
            if !is_current(&state) {
                return;
            }
            // Keep cross-file findings from the last full run; replace the
            // per-file ones with the fresh results.
            let mut merged: Vec<Finding> = state
                .diagnostics
                .get(&uri)
                .map(|prev| prev.iter().filter(|f| !f.per_file).cloned().collect())
                .unwrap_or_default();
            merged.extend(per_file);
            let diags: Vec<Diagnostic> = merged
                .iter()
                .map(|finding| finding_to_diagnostic(finding, state.related_information_supported))
                .collect();
            if merged.is_empty() {
                state.diagnostics.remove(&uri);
            } else {
                state.diagnostics.insert(uri.clone(), merged);
            }
            diags
        };
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    /// The governing session, cached findings, buffer and buffer version
    /// for `uri`: everything `codeAction` needs, or `None` when any is
    /// missing.
    fn code_action_inputs(&self, uri: &Url) -> Option<CodeActionInputs> {
        let state = self.state.lock();
        let session = state
            .doc_config
            .get(uri)
            .and_then(|c| state.sessions.get(c).cloned())?;
        Some(CodeActionInputs {
            session,
            findings: state.diagnostics.get(uri).cloned()?,
            text: state.documents.get(uri).cloned()?,
            version: state.versions.get(uri).copied(),
            related_information_supported: state.related_information_supported,
        })
    }

    async fn publish_all(&self, items: Vec<Publication>) {
        for (uri, diagnostics, version) in items {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
    }
}

#[allow(clippy::too_many_lines)]
#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> JsonRpcResult<InitializeResult> {
        // Strictly `true`: a missing or malformed value leaves the workspace
        // untrusted.
        let trusted = params
            .initialization_options
            .as_ref()
            .and_then(|options| options.get("trusted"))
            .and_then(tower_lsp::lsp_types::LSPAny::as_bool)
            .unwrap_or(false);
        let related_information_supported = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|text| text.publish_diagnostics.as_ref())
            .and_then(|publish| publish.related_information)
            .unwrap_or(false);
        let mut state = self.state.lock();
        state.folders = workspace_folders(&params);
        state.related_information_supported = related_information_supported;
        state.trusted = trusted;
        drop(state);
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

        let Some(inputs) = self.code_action_inputs(&uri) else {
            return Ok(None);
        };
        let Ok(abs) = uri.to_file_path() else {
            return Ok(None);
        };
        let version = inputs.version;
        // Fixers recurse over the parsed buffer: compute off the async
        // workers, on a blocking thread with a full-sized stack.
        let computed =
            tokio::task::spawn_blocking(move || fix_actions(&inputs, &abs, selection)).await;
        let Ok(Some((mut actions, unsafe_flags))) = computed else {
            return Ok(None);
        };
        // The buffer changed while the edits were computed: they would
        // apply to text the editor no longer has.
        if self.state.lock().versions.get(&uri).copied() != version {
            return Ok(None);
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

/// What `codeAction` computes its fixes from, read under the state lock.
struct CodeActionInputs {
    session: Arc<Session>,
    findings: Vec<Finding>,
    text: String,
    /// The buffer's version, to drop edits computed for an older buffer.
    version: Option<i32>,
    related_information_supported: bool,
}

/// The quick-fixes for the findings of `abs` that overlap `selection`, and
/// whether each is Unsafe-tier; `None` when the file gets no fix at all.
/// Runs on a blocking thread (`codeAction` awaits it).
fn fix_actions(
    inputs: &CodeActionInputs,
    abs: &Path,
    selection: Range,
) -> Option<(CodeActionResponse, Vec<bool>)> {
    let CodeActionInputs {
        session,
        findings,
        text,
        related_information_supported,
        ..
    } = inputs;
    let related_information_supported = *related_information_supported;
    let Ok(rel) = abs.strip_prefix(&session.root).map(Path::to_path_buf) else {
        return None;
    };
    // A sibling rule may have read secret-bearing evidence from this file
    // even when the selected diagnostic itself is ordinary. Whole-file and
    // spanning fixes can otherwise copy unchanged secret bytes into the LSP
    // response, so suppress every public edit for a sensitive input path.
    if session
        .engine
        .path_contains_sensitive_evidence(&session.root, &session.index, &rel)
    {
        return None;
    }

    let bytes = text.as_bytes();
    // Honor the config's `fix_size_limit`: `alint fix` skips a file
    // this large, so the editor must not offer to fix it either.
    if let Some(limit) = session.engine.fix_size_limit()
        && u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit
    {
        return None;
    }
    let mut actions: CodeActionResponse = Vec::new();
    // Parallel to `actions`: whether each offered fix is Unsafe-tier. An Unsafe
    // quick-fix is labeled `(unsafe)` and never auto-preferred, so the click is
    // a visible, explicit opt-in (the LSP analogue of `--unsafe-fixes`) rather
    // than a silent one-click apply of a behavior-changing edit (e.g. a file
    // delete). Only Safe and Unsafe tiers reach here -- Suggestion / demoted
    // fixes are already filtered out below.
    let mut unsafe_flags: Vec<bool> = Vec::new();
    for finding in findings {
        if !finding.fixable || finding.sensitive || !ranges_overlap(finding.range, selection) {
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
            // A located fix edits this document only.
            if session.edit_refusal(&finding.rule_id, &rel).is_some() {
                continue;
            }
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
                    sensitive: false,
                    collected: ce,
                })
                .collect();
            let (_, outcomes) = located_fix::apply_file_edits(bytes, batch, Applicability::Unsafe);
            let applied: Vec<CollectedEdit> = outcomes
                .into_iter()
                .filter(|(_, outcome)| *outcome == LocatedOutcome::Applied)
                .map(|(le, _)| le.collected)
                .collect();
            match located_edits_to_workspace_edit(&applied, text, &rel, &session.root) {
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
            if edit_paths(&edit).any(|path| session.edit_refusal(&finding.rule_id, path).is_some())
            {
                continue;
            }
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
            diagnostics: Some(vec![finding_to_diagnostic(
                finding,
                related_information_supported,
            )]),
            edit: Some(workspace_edit),
            ..CodeAction::default()
        }));
        unsafe_flags.push(unsafe_fix);
    }
    Some((actions, unsafe_flags))
}

/// Every path `edit` writes, creates, removes or renames.
fn edit_paths(edit: &alint_core::FixEdit) -> impl Iterator<Item = &Path> {
    use alint_core::FixEdit;
    let (first, second) = match edit {
        FixEdit::SetContent { path, .. }
        | FixEdit::CreateFile { path, .. }
        | FixEdit::DeleteFile { path }
        | FixEdit::ReplaceRange { path, .. }
        | FixEdit::SetMode { path, .. } => (path.as_path(), None),
        FixEdit::RenameFile { from, to } => (from.as_path(), Some(to.as_path())),
    };
    std::iter::once(first).chain(second)
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
fn findings_to_diagnostics(
    findings: &[Finding],
    related_information_supported: bool,
) -> Vec<Diagnostic> {
    findings
        .iter()
        .map(|finding| finding_to_diagnostic(finding, related_information_supported))
        .collect()
}

/// Record each config's run in `state` (its session, or its load error) and
/// queue its notes block when it changed. Returns each config's findings by
/// path, and whether an untrusted session withheld anything.
fn record_runs(
    state: &mut State,
    runs: Vec<(SessionKey, Result<ConfigRun, ConfigError>)>,
    options: &LspOptions,
    applied: &mut Applied,
) -> (HashMap<SessionKey, FindingsByPath>, bool) {
    let mut by_config: HashMap<SessionKey, FindingsByPath> = HashMap::new();
    let mut withheld = false;
    state.sessions.clear();
    state.broken.clear();
    for (key, run) in runs {
        match run {
            Ok(run) => {
                let block = render_notes(&key.config, &run.notes, options.show_notes);
                if state.last_notes.get(&key) != Some(&block) {
                    if !block.is_empty() {
                        applied.notes.push(block.clone());
                    }
                    state.last_notes.insert(key.clone(), block);
                }
                withheld |= run.session.withheld;
                state.sessions.insert(key.clone(), run.session);
                by_config.insert(key, run.by_path);
            }
            Err(error) => {
                withheld |= error.untrusted;
                applied.logs.push(error.message.clone());
                state.broken.insert(key, error);
            }
        }
    }
    (by_config, withheld)
}

fn apply_check(
    state: &mut State,
    docs: &[(Url, Option<String>, Option<i32>)],
    check: WorkspaceCheck,
    options: &LspOptions,
) -> Applied {
    let mut applied = Applied::default();
    let (by_config, withheld) = record_runs(state, check.runs, options, &mut applied);
    // Once per server: the note says what the workspace's trust turns on.
    if withheld && !state.untrusted_note_shown {
        state.untrusted_note_shown = true;
        applied.untrusted_note = true;
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
        let diagnostics = findings_to_diagnostics(&findings, state.related_information_supported);
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
    let keys: Vec<SessionKey> = state
        .sessions
        .keys()
        .chain(state.broken.keys())
        .cloned()
        .collect();
    for key in keys {
        let Ok(uri) = Url::from_file_path(&key.config) else {
            continue;
        };
        let (findings, diagnostics) = if let Some(error) = state.broken.get(&key) {
            let diagnostic = Diagnostic {
                range: error
                    .range
                    .unwrap_or_else(|| Range::new(Position::new(0, 0), Position::new(0, 1))),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("alint".to_string()),
                message: error.message.clone(),
                ..Diagnostic::default()
            };
            (Vec::new(), vec![diagnostic])
        } else {
            let findings = by_config
                .get(&key)
                .and_then(|m| m.get(&key.config).cloned())
                .unwrap_or_default();
            let diagnostics =
                findings_to_diagnostics(&findings, state.related_information_supported);
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
