//! Language Server Protocol server for alint.
//!
//! A synchronous `lsp-server` backend that runs the alint engine over the
//! workspace on a fixed 8 MiB worker pool and publishes the resulting
//! violations as LSP diagnostics. It is driven by the `alint lsp` subcommand,
//! speaking LSP over stdio (see [`run_stdio`]).
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
//!   [`alint_core::Fixer::fix_edit`] -> [`alint_core::FixEdit`]; a *located*
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

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded};
use lsp_server::{Connection, ErrorCode, Message, Notification, RequestId, Response};
use lsp_types::notification::{
    LogMessage, Notification as LspNotification, PublishDiagnostics, ShowMessage,
};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, Diagnostic, DiagnosticRelatedInformation,
    DiagnosticSeverity, DidCloseTextDocumentParams, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, LogMessageParams, MarkupContent,
    MarkupKind, MessageType, OneOf, Position, PublishDiagnosticsParams, Range, ServerCapabilities,
    ServerInfo, ShowMessageParams, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use parking_lot::Mutex;

use alint_core::baseline::Baseline;
use alint_core::located_fix::{self, LocatedEdit, LocatedOutcome};
use alint_core::{Applicability, CollectedEdit, Engine, Error, FileIndex, Violation};

mod config;
mod diagnostics;
mod edits;
mod render;
mod transport;

#[cfg(test)]
mod protocol_tests;
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
#[cfg(test)]
use transport::run_connection;
use transport::{ConnectionOutcome, run_connection_outcome};

/// Worker pool size for background per-file re-evaluations and code-action
/// computation. Full workspace checks run on their own dedicated thread
/// (`alint-lsp-check`), so slow full checks never starve interactive edits or
/// quick-fix requests while keeping memory and thread count bounded.
const WORKER_COUNT: usize = 4;

/// Rules and fixers recurse over parsed documents; give each LSP worker the
/// stack size a main thread has (8 MiB), matching the CLI.
const WORKER_STACK_SIZE: usize = 8 * 1024 * 1024;

/// Maximum time `run_connection` waits on `exit` / EOF for background worker
/// threads and the outbound relay to drain before detaching them so a slow
/// command/remote rule or stalled client stdout pipe cannot hang shutdown.
const EXIT_JOIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Optional hook registered by the CLI binary (`alint lsp`) so any environment
/// values loaded by LSP sessions are also forwarded to the process-wide stderr
/// tracing redactor (F17).
type TracingEnvHook = fn(&[String]);

static TRACING_ENV_HOOK: std::sync::OnceLock<TracingEnvHook> = std::sync::OnceLock::new();

/// Register a callback invoked whenever an LSP session loads a config with
/// environment-interpolated top-level values so stderr `tracing` output redacts
/// them alongside CLI sessions.
#[doc(hidden)]
pub fn set_tracing_env_hook(hook: fn(&[String])) {
    let _ = TRACING_ENV_HOOK.set(hook);
}

pub(crate) fn notify_tracing_env_values(values: &[String]) {
    if !values.is_empty()
        && let Some(hook) = TRACING_ENV_HOOK.get()
    {
        hook(values);
    }
}

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

    /// Sets [`Self::registry`](field@Self::registry).
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
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
struct Finding {
    range: Range,
    severity: DiagnosticSeverity,
    rule_id: String,
    message: String,
    /// The violation's original 1-indexed line/column (if any), kept so
    /// a code-action fixer that is range-scoped sees the same location
    /// the rule reported - `range` alone can't distinguish a real
    /// (1,1) anchor from a path-less finding anchored at the file start.
    line: Option<usize>,
    column: Option<usize>,
    policy_url: Option<String>,
    /// Whether the rule declares a fixer - gates the "Apply fix" code
    /// action without re-deriving the rule set.
    fixable: bool,
    /// Sensitive findings expose diagnostics but never public replacement bytes.
    sensitive: bool,
    /// Whether this came from a per-file rule (re-evaluated on every
    /// edit) vs a cross-file rule (only on save). On `didChange` we keep
    /// the cached cross-file findings and replace only the per-file ones,
    /// so cross-file markers don't flicker away while typing.
    per_file: bool,
    /// Load-time config pattern hint (deduplicated across roots sharing a
    /// `--config` file).
    is_pattern_hint: bool,
    /// The rule kind's one-sentence description, for the hover.
    description: Option<&'static str>,
    /// The rule kind's alint.org reference page, for the hover.
    docs_url: Option<String>,
    related_information: Vec<DiagnosticRelatedInformation>,
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
    /// The config's `baseline:` file, loaded - grandfathered findings
    /// are filtered out exactly as `alint check` filters them.
    baseline: Option<Baseline>,
    /// Rule id -> canonical rule kind (aliases resolved through the session's
    /// registry), for the hover's description + docs link.
    kinds: HashMap<String, String>,
    /// Load-time pattern hints, shown as warnings on the config file.
    pattern_hints: Vec<alint_core::pattern_hints::ConfigHint>,
    /// Set-backed, single-automaton scrubber built from
    /// [`alint_core::Config::environment_values`], removed from any error the
    /// session reports (a rule run that fails on an `ignore:` or `facts:`
    /// value read from the environment).
    environment_redactor: alint_core::EnvironmentRedactor,
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
    /// Subset of `config_files` reachable only via an environment-chosen
    /// `extends:` entry ([`alint_core::Config::environment_config_files`]).
    environment_config_files: Vec<PathBuf>,
}

impl Session {
    /// `message` without the config's environment values.
    fn redact(&self, message: &str) -> String {
        self.environment_redactor.redact(message).into_owned()
    }

    /// Why a code action of `rule_id` may not edit `path` (root-relative), as
    /// [`alint_core::FixContext::write_refusal`] refuses an `alint fix` write:
    /// a path in a Git directory, and for a rule an `extends:`'d ruleset
    /// shaped, alint's own configuration.
    fn edit_refusal(&self, rule_id: &str, path: &Path) -> Option<String> {
        alint_core::FixContext::new(&self.root, false)
            .with_extended_source(self.extended_rules.contains(rule_id))
            .with_config_files(&self.config_files)
            .with_environment_config_files(&self.environment_config_files)
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

/// Serve the alint language server over stdio until the client sends `exit`
/// (or closes the stream). Called by the `alint lsp` subcommand.
///
/// Returns the process exit code the LSP specification mandates: `0` when
/// `exit` follows a `shutdown` request (or the client simply closed the
/// stream), `1` when `exit` arrives without a prior `shutdown`.
/// `lsp-server`'s stdio reader breaks out immediately on `exit`, so the
/// process flushes stdout and terminates promptly even while the client
/// keeps stdin open.
pub fn run_stdio(options: LspOptions) -> std::io::Result<i32> {
    let (connection, io_threads) = Connection::stdio();
    let outcome = run_connection_outcome(&connection, options)?;
    drop(connection);
    join_io_threads_bounded(io_threads, outcome)
}

/// Join `lsp-server`'s stdio threads within [`EXIT_JOIN_TIMEOUT`], detaching
/// on timeout or when the outbound relay already detached on a stalled stdout
/// pipe (so `LspServerWriter`/`LspMessageDropper` can never hang process exit).
fn join_io_threads_bounded(
    io_threads: lsp_server::IoThreads,
    outcome: ConnectionOutcome,
) -> std::io::Result<i32> {
    if !outcome.relay_drained {
        // The outbound relay timed out on a stalled stdout pipe and still holds
        // a clone of `Connection::sender`; calling `io_threads.join()` would
        // block forever on `LspServerWriter`/`LspMessageDropper`.
        return Ok(outcome.code);
    }
    let (tx, rx) = bounded::<std::io::Result<()>>(1);
    let spawned = std::thread::Builder::new()
        .name("alint-lsp-io-join".to_string())
        .spawn(move || {
            let _ = tx.send(io_threads.join());
        });
    let Ok(_joiner) = spawned else {
        return Ok(outcome.code);
    };
    if let Ok(Err(err)) = rx.recv_timeout(EXIT_JOIN_TIMEOUT) {
        match err.kind() {
            // If the client closed stdout immediately after `exit` (including
            // `exit` without `shutdown`), preserve the LSP-mandated exit code
            // instead of turning BrokenPipe into a transport error.
            std::io::ErrorKind::BrokenPipe => {}
            // `lsp-server` formats malformed JSON-RPC frames with the raw
            // message text (`InvalidData`), which may carry unsaved buffer
            // contents or secrets. Scrub the payload before returning.
            std::io::ErrorKind::InvalidData => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed LSP message on stdin",
                ));
            }
            _ => return Err(err),
        }
    }
    Ok(outcome.code)
}

/// The LSP-mandated process exit code: `1` for an `exit` notification
/// that was not preceded by `shutdown`, else `0`.
fn exit_code(exit_received: bool, shutdown_received: bool) -> i32 {
    i32::from(exit_received && !shutdown_received)
}

/// Extract a concise diagnostic string from a `catch_unwind` panic payload.
fn panic_summary(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
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
    /// there, or a config error) - cleared when no longer relevant.
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
    /// Monotonic server-assigned sequence per open/edit so closing and
    /// reopening a document at the same `i32` client version cannot race.
    doc_seq: HashMap<Url, u64>,
    /// The last notes block printed per session, so an unchanged set
    /// isn't re-printed on every save.
    last_notes: HashMap<SessionKey, String>,
    /// The last walk warnings logged per session, so unchanged parent-ignore
    /// warnings aren't re-logged on every save (#568).
    last_walk_warnings: HashMap<SessionKey, Vec<String>>,
}

/// Per-instance tracking of active `textDocument/codeAction` requests so
/// duplicate client `RequestId`s cannot clobber each other's cancellation flag
/// and `shutdown` can wait asynchronously without blocking the LSP dispatch loop.
#[derive(Debug, Default)]
struct CodeActionsState {
    next_token: u64,
    active: HashMap<u64, (RequestId, bool)>,
    pending_shutdown: Option<Sender<()>>,
}

/// Shared server core driven by the main LSP message loop and worker threads.
///
/// Lock ordering invariant (prevents deadlocks):
/// `check_lock` -> `reeval_publish` -> `state`.
/// Leaf locks (`outbound`, `code_actions`, `pending_reevals`) are never held
/// while acquiring any other lock. `state` is never held across engine/fixer
/// execution or channel sends.
#[derive(Debug)]
struct ServerCore {
    /// Unbounded outbound message sink (closed on `exit`/EOF so the outbound
    /// relay flushes queued responses and drops `Connection::sender` cleanly).
    outbound: Mutex<Option<Sender<Message>>>,
    options: LspOptions,
    /// Shared server state behind a `parking_lot::Mutex` (no poisoning), so a
    /// panic in one worker can't permanently wedge the session. Critical
    /// sections stay tiny and infallible (map inserts/clones).
    state: Arc<Mutex<State>>,
    /// Serializes full workspace checks so an older check cannot finish and
    /// overwrite a newer check's state.
    check_lock: Mutex<()>,
    /// Serializes the "is this buffer version still current?" check with the
    /// `publishDiagnostics` that follows it in [`Self::reeval_file_seq`] and
    /// [`Self::did_close`], so an older buffer's diagnostics can never reach
    /// the client after a newer buffer's or after close.
    reeval_publish: Mutex<()>,
    /// Monotonic generation of requested full checks for coalescing.
    check_gen: AtomicU64,
    /// Highest full-check generation already covered by a completed check.
    completed_check_gen: AtomicU64,
    /// Monotonic per-document sequence assigned on every `didOpen`/`didChange`.
    next_doc_seq: AtomicU64,
    /// Set on `shutdown` (and `exit` / EOF) so background workers stop emitting
    /// new server notifications once `shutdown` begins.
    shutting_down: AtomicBool,
    /// Set on `exit` / disconnect so queued worker jobs skip promptly. Held
    /// behind an `Arc` so the outbound relay can read it without retaining an
    /// `Arc<ServerCore>` reference cycle.
    exiting: Arc<AtomicBool>,
    /// Active `codeAction` requests, cancellation flags, and optional `shutdown`
    /// drain notifier.
    code_actions: Mutex<CodeActionsState>,
    /// URIs currently queued for coalesced single-file re-evaluation so bursts
    /// of `didChange` on one file never flood the worker pool or clone buffers.
    pending_reevals: Mutex<HashSet<Url>>,
    /// Buffer evaluations started, for tests of `reeval_file` coalescing.
    #[cfg(test)]
    evaluations: Arc<std::sync::atomic::AtomicUsize>,
}

/// A document to publish: URI, diagnostics, and the buffer version they
/// were computed against (if any).
type Publication = (Url, Vec<Diagnostic>, Option<i32>);

/// Snapshot of an open document captured before a full workspace check.
type DocSnapshot = (Url, Option<String>, Option<i32>, Option<u64>);

/// Pending single-file re-evaluation queued when an open document changed
/// while a full workspace check was in flight.
type PendingReeval = (Url, String, i32, Option<u64>);

/// What applying a [`WorkspaceCheck`] to the state produced.
#[derive(Debug, Default)]
struct Applied {
    publish: Vec<Publication>,
    /// Config errors and walk warnings to log to the client.
    logs: Vec<String>,
    /// Notes blocks to print on stderr.
    notes: Vec<String>,
    /// Open documents edited while the full check ran: re-evaluate
    /// their current buffer afterwards so the newest edit wins.
    reeval: Vec<PendingReeval>,
    /// Show the untrusted-workspace note ([`config::UNTRUSTED_NOTE`]).
    untrusted_note: bool,
    /// A config file was open (potentially with unsaved edits) when this check
    /// started and closed before it finished: re-run from disk immediately.
    recheck: bool,
}

impl ServerCore {
    fn new(sender: Sender<Message>, options: LspOptions) -> Self {
        Self {
            outbound: Mutex::new(Some(sender)),
            options,
            state: Arc::new(Mutex::new(State::default())),
            check_lock: Mutex::new(()),
            reeval_publish: Mutex::new(()),
            check_gen: AtomicU64::new(0),
            completed_check_gen: AtomicU64::new(0),
            next_doc_seq: AtomicU64::new(1),
            shutting_down: AtomicBool::new(false),
            exiting: Arc::new(AtomicBool::new(false)),
            code_actions: Mutex::new(CodeActionsState::default()),
            pending_reevals: Mutex::new(HashSet::new()),
            #[cfg(test)]
            evaluations: Arc::default(),
        }
    }

    fn is_stopping(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst) || self.exiting.load(Ordering::SeqCst)
    }

    fn send(&self, msg: Message) {
        if let Some(tx) = self.outbound.lock().as_ref() {
            let _ = tx.send(msg);
        }
    }

    fn close_outbound(&self) {
        self.outbound.lock().take();
    }

    fn send_notification<N: LspNotification>(&self, params: N::Params) {
        if self.is_stopping() {
            return;
        }
        if let Ok(params) = serde_json::to_value(params) {
            // Re-check `is_stopping()` under `outbound.lock()` so a background
            // notification (e.g. `LogMessage` on panic/error) cannot be queued
            // after `shutdown` sets `shutting_down = true` and enqueues `Ok(null)`.
            let outbound = self.outbound.lock();
            if self.is_stopping() {
                return;
            }
            if let Some(tx) = outbound.as_ref() {
                let _ = tx.send(Message::Notification(Notification::new(
                    N::METHOD.to_string(),
                    params,
                )));
            }
        }
    }

    fn send_ok(&self, id: RequestId, value: serde_json::Value) {
        if self.exiting.load(Ordering::SeqCst) {
            return;
        }
        self.send(Message::Response(Response::new_ok(id, value)));
    }

    fn send_err(&self, id: RequestId, code: ErrorCode, message: String) {
        if self.exiting.load(Ordering::SeqCst) {
            return;
        }
        self.send(Message::Response(Response::new_err(
            id,
            code as i32,
            message,
        )));
    }

    fn register_code_action(&self, id: &RequestId) -> u64 {
        let mut st = self.code_actions.lock();
        let token = st.next_token;
        st.next_token = st.next_token.wrapping_add(1);
        st.active.insert(token, (id.clone(), false));
        token
    }

    fn finish_code_action(&self, token: u64) {
        let notify = {
            let mut st = self.code_actions.lock();
            st.active.remove(&token);
            if st.active.is_empty() {
                st.pending_shutdown.take()
            } else {
                None
            }
        };
        if let Some(tx) = notify {
            let _ = tx.send(());
        }
    }

    fn cancel_request(&self, id: &RequestId) {
        let mut st = self.code_actions.lock();
        for (req_id, canceled) in st.active.values_mut() {
            if req_id == id {
                *canceled = true;
            }
        }
    }

    fn is_canceled(&self, token: u64) -> bool {
        self.code_actions
            .lock()
            .active
            .get(&token)
            .is_some_and(|(_, canceled)| *canceled)
    }

    /// If any `codeAction` requests are currently in flight, return a oneshot
    /// receiver that fires once the last one finishes; return `None` when none
    /// are active so `shutdown` can reply immediately.
    fn arm_shutdown_wait(&self) -> Option<Receiver<()>> {
        let mut st = self.code_actions.lock();
        if st.active.is_empty() {
            return None;
        }
        let (tx, rx) = bounded(1);
        st.pending_shutdown = Some(tx);
        Some(rx)
    }

    fn enqueue_reeval(&self, reeval_tx: &Sender<Url>, uri: &Url) {
        let should_send = self.pending_reevals.lock().insert(uri.clone());
        if should_send && reeval_tx.send(uri.clone()).is_err() {
            self.pending_reevals.lock().remove(uri);
        }
    }

    fn reeval_latest(&self, uri: Url) {
        self.pending_reevals.lock().remove(&uri);
        if self.is_stopping() {
            return;
        }
        let latest = {
            let state = self.state.lock();
            if !state.open.contains(&uri) {
                return;
            }
            let Some(text) = state.documents.get(&uri).cloned() else {
                return;
            };
            let Some(version) = state.versions.get(&uri).copied() else {
                return;
            };
            let seq = state.doc_seq.get(&uri).copied();
            (text, version, seq)
        };
        self.reeval_file_seq(uri, latest.0, latest.1, latest.2);
    }

    fn log_message(&self, typ: MessageType, message: impl Into<String>) {
        self.send_notification::<LogMessage>(LogMessageParams {
            typ,
            message: message.into(),
        });
    }

    fn show_message(&self, typ: MessageType, message: impl Into<String>) {
        self.send_notification::<ShowMessage>(ShowMessageParams {
            typ,
            message: message.into(),
        });
    }

    fn publish_diagnostics(&self, uri: Url, diagnostics: Vec<Diagnostic>, version: Option<i32>) {
        self.send_notification::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri,
            diagnostics,
            version,
        });
    }

    fn publish_all(&self, items: Vec<Publication>) {
        for (uri, diagnostics, version) in items {
            if self.is_stopping() {
                break;
            }
            self.publish_diagnostics(uri, diagnostics, version);
        }
    }

    fn initialize(&self, params: &InitializeParams) -> InitializeResult {
        // Strictly `true`: a missing or malformed value leaves the workspace
        // untrusted.
        let trusted = params
            .initialization_options
            .as_ref()
            .and_then(|options| options.get("trusted"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let related_information_supported = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|text| text.publish_diagnostics.as_ref())
            .and_then(|publish| publish.related_information)
            .unwrap_or(false);
        {
            let mut state = self.state.lock();
            state.folders = workspace_folders(params);
            state.related_information_supported = related_information_supported;
            state.trusted = trusted;
        }
        InitializeResult {
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
        }
    }

    fn hover(&self, params: HoverParams) -> Option<Hover> {
        let pos = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;

        let findings = self.state.lock().diagnostics.get(&uri).cloned()?;
        let matching: Vec<&Finding> = findings
            .iter()
            .filter(|f| range_contains(f.range, pos))
            .collect();
        if matching.is_empty() {
            return None;
        }

        let value = matching
            .iter()
            .map(|f| render_finding(f))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: matching.first().map(|f| f.range),
        })
    }

    /// Close an open document and clear its diagnostics (unless it is a config
    /// file, whose on-disk diagnostics stay visible). Returns `true` when the
    /// closed URI was a config file so the caller can refresh on-disk findings.
    fn did_close(&self, params: DidCloseTextDocumentParams) -> bool {
        let uri = params.text_document.uri;
        let _publish_guard = self.reeval_publish.lock();
        let is_config = {
            let mut state = self.state.lock();
            state.open.remove(&uri);
            state.documents.remove(&uri);
            state.versions.remove(&uri);
            state.doc_seq.remove(&uri);
            state.doc_config.remove(&uri);
            let is_config = state.config_uris.contains(&uri);
            if !is_config {
                state.diagnostics.remove(&uri);
            }
            is_config
        };
        // Clear any diagnostics the editor is still showing - except on a
        // config file, whose anchored findings / load error stay visible
        // whether or not it is open.
        if !is_config {
            self.publish_diagnostics(uri, Vec::new(), None);
        }
        is_config
    }

    /// Full check: (re)build every relevant session and publish per-file
    /// diagnostics for every open document (plus each config file),
    /// clearing those that no longer have findings. Runs on open, save,
    /// watched-file changes, and workspace-folder changes.
    fn check_and_publish(&self, target_gen: u64) {
        let _check_guard = self.check_lock.lock();
        if self.is_stopping() || self.completed_check_gen.load(Ordering::SeqCst) >= target_gen {
            return;
        }
        let started_gen = self.check_gen.load(Ordering::SeqCst);
        let (folders, docs, trusted, has_prior_state) = {
            let state = self.state.lock();
            let docs: Vec<DocSnapshot> = state
                .open
                .iter()
                .map(|u| {
                    (
                        u.clone(),
                        state.documents.get(u).cloned(),
                        state.versions.get(u).copied(),
                        state.doc_seq.get(u).copied(),
                    )
                })
                .collect();
            let has_prior_state = !state.sessions.is_empty()
                || !state.broken.is_empty()
                || !state.doc_config.is_empty()
                || !state.config_uris.is_empty()
                || !state.diagnostics.is_empty()
                || !state.last_notes.is_empty()
                || !state.last_walk_warnings.is_empty();
            (state.folders.clone(), docs, state.trusted, has_prior_state)
        };

        let check = if folders.is_empty() {
            if !has_prior_state {
                self.completed_check_gen
                    .fetch_max(started_gen, Ordering::SeqCst);
                return;
            }
            // The last workspace folder was removed: apply an empty check so
            // stale sessions, config diagnostics, and open-document findings
            // are cleared cleanly.
            WorkspaceCheck::default()
        } else {
            let task_docs: Vec<(Url, Option<String>)> = docs
                .iter()
                .map(|(u, t, _, _)| (u.clone(), t.clone()))
                .collect();
            let settings = self.options.settings(trusted);
            let registry = self.options.registry();
            match catch_unwind(AssertUnwindSafe(|| {
                check_workspace(&folders, &task_docs, &settings, &registry)
            })) {
                Ok(check) => check,
                Err(payload) => {
                    tracing::error!(panic = %panic_summary(&*payload), "alint LSP full check panicked");
                    self.log_message(MessageType::ERROR, "alint: check panicked");
                    self.completed_check_gen
                        .fetch_max(started_gen, Ordering::SeqCst);
                    return;
                }
            }
        };

        if self.is_stopping() {
            return;
        }

        let (notes, reeval, recheck) = {
            let _publish_guard = self.reeval_publish.lock();
            if self.is_stopping() {
                return;
            }
            let applied = {
                let mut state = self.state.lock();
                apply_check(&mut state, &docs, check, &self.options)
            };
            // Enqueue logs and untrusted note ahead of `publish_all` onto the
            // non-blocking outbound channel so client logs arrive before the
            // matching diagnostics, while keeping blocking stderr writes
            // outside `reeval_publish`.
            for message in applied.logs {
                self.log_message(MessageType::WARNING, format!("alint: {message}"));
            }
            if applied.untrusted_note {
                self.show_message(MessageType::INFO, config::UNTRUSTED_NOTE);
            }
            self.publish_all(applied.publish);
            (applied.notes, applied.reeval, applied.recheck)
        };
        // Advance `completed_check_gen` BEFORE stderr writes or follow-up
        // re-evaluations so a broken stderr pipe or worker panic can never
        // leave the check thread stuck repeating the same generation.
        self.completed_check_gen
            .fetch_max(started_gen, Ordering::SeqCst);
        if recheck {
            self.check_gen.fetch_add(1, Ordering::SeqCst);
        }
        {
            use std::io::Write as _;
            let mut stderr = std::io::stderr();
            for block in notes {
                let _ = writeln!(stderr, "{block}");
            }
        }
        for (uri, text, version, seq) in reeval {
            self.reeval_file_seq(uri, text, version, seq);
        }
    }

    /// Single-file hot path: re-evaluate per-file rules against the
    /// editor's in-memory `text` and publish diagnostics for just this
    /// document. Per-file findings replace the previous per-file ones,
    /// but cached cross-file findings (from the last full run) are
    /// preserved so they don't flicker away while typing - they refresh
    /// on the next save. `version` ties the diagnostics to the edit.
    ///
    /// Only the latest version of a document is evaluated: a version that a
    /// newer edit superseded is skipped before it starts, and again before
    /// publishing if a newer edit arrived while it evaluated.
    #[cfg(test)]
    fn reeval_file(&self, uri: Url, text: String, version: i32) {
        self.reeval_file_seq(uri, text, version, None);
    }

    fn is_doc_current(state: &State, uri: &Url, version: i32, seq: Option<u64>) -> bool {
        state.open.contains(uri)
            && state.versions.get(uri) == Some(&version)
            && seq.is_none_or(|expected| state.doc_seq.get(uri) == Some(&expected))
    }

    fn is_session_current(state: &State, uri: &Url, session: &Arc<Session>) -> bool {
        state
            .doc_config
            .get(uri)
            .filter(|c| !state.broken.contains_key(*c))
            .and_then(|c| state.sessions.get(c))
            .is_some_and(|cur| Arc::ptr_eq(cur, session))
    }

    /// Keep editor diagnostics synchronized with `state.diagnostics` when a
    /// queued `reeval_file_seq` aborts early (e.g. `strip_prefix` failure or
    /// `eval_buffer` error/panic) after `apply_check` merged cross-file findings.
    fn publish_cached_if_current(
        &self,
        uri: &Url,
        version: i32,
        seq: Option<u64>,
        session: &Arc<Session>,
    ) {
        let _publish_guard = self.reeval_publish.lock();
        if self.is_stopping() {
            return;
        }
        let diagnostics = {
            let state = self.state.lock();
            if !Self::is_doc_current(&state, uri, version, seq)
                || !Self::is_session_current(&state, uri, session)
            {
                return;
            }
            state
                .diagnostics
                .get(uri)
                .map(|findings| {
                    findings_to_diagnostics(findings, state.related_information_supported)
                })
                .unwrap_or_default()
        };
        self.publish_diagnostics(uri.clone(), diagnostics, Some(version));
    }

    fn reeval_file_seq(&self, uri: Url, text: String, version: i32, seq: Option<u64>) {
        if self.is_stopping() {
            return;
        }
        let session = {
            let state = self.state.lock();
            if !Self::is_doc_current(&state, &uri, version, seq) {
                // Superseded (its successor re-evaluates), or closed/reopened.
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
            self.publish_cached_if_current(&uri, version, seq, &session);
            return;
        };

        #[cfg(test)]
        self.evaluations.fetch_add(1, Ordering::SeqCst);
        let outcome = catch_unwind(AssertUnwindSafe(|| eval_buffer(&session, &abs, &rel, text)));

        if self.is_stopping() {
            return;
        }

        let per_file = match outcome {
            Ok(Ok(per_file)) => per_file,
            // Excluded from the walk, or not walked yet: no per-file rule
            // applies, but a finding from the last full run still does. A
            // `git_tracked_only` rule reports a tracked file that an ignore
            // file hides (#503).
            Ok(Err(Error::FileNotInIndex { .. })) => Vec::new(),
            Ok(Err(err)) => {
                let current = {
                    let state = self.state.lock();
                    Self::is_doc_current(&state, &uri, version, seq)
                        && Self::is_session_current(&state, &uri, &session)
                };
                if current {
                    self.log_message(MessageType::WARNING, format!("alint: {err}"));
                    self.publish_cached_if_current(&uri, version, seq, &session);
                }
                return;
            }
            Err(payload) => {
                tracing::error!(panic = %panic_summary(&*payload), "alint LSP re-eval panicked");
                let current = {
                    let state = self.state.lock();
                    Self::is_doc_current(&state, &uri, version, seq)
                        && Self::is_session_current(&state, &uri, &session)
                };
                if current {
                    self.log_message(MessageType::ERROR, "alint: re-eval panicked");
                    self.publish_cached_if_current(&uri, version, seq, &session);
                }
                return;
            }
        };

        // Workers run `didChange` jobs concurrently, so a slow evaluation of an
        // older buffer or superseded session can finish after a newer one (or
        // after a config break / folder removal). Drop any result whose
        // version/sequence or governing session is no longer current.
        let _publish_guard = self.reeval_publish.lock();
        if self.is_stopping() {
            return;
        }
        let diagnostics = {
            let mut state = self.state.lock();
            if !Self::is_doc_current(&state, &uri, version, seq)
                || !Self::is_session_current(&state, &uri, &session)
            {
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
        self.publish_diagnostics(uri, diagnostics, Some(version));
    }

    #[cfg(test)]
    fn code_action(&self, params: CodeActionParams) -> Option<CodeActionResponse> {
        self.compute_code_action(params).ok().flatten()
    }

    fn compute_code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>, String> {
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
        let seq = inputs.seq;
        let (mut actions, unsafe_flags) =
            match catch_unwind(AssertUnwindSafe(|| fix_actions(&inputs, &abs, selection))) {
                Ok(Some(res)) => res,
                Ok(None) => return Ok(None),
                Err(payload) => {
                    let summary = panic_summary(&*payload);
                    tracing::error!(panic = %summary, "alint LSP code action panicked");
                    self.log_message(MessageType::ERROR, "alint: code action panicked");
                    return Err("alint: code action panicked".to_string());
                }
            };
        // The buffer changed, closed, or its governing session reloaded/broke
        // while edits were computed: they would apply to stale text or rules.
        {
            let state = self.state.lock();
            if !state.open.contains(&uri)
                || state.versions.get(&uri).copied() != version
                || state.doc_seq.get(&uri).copied() != seq
                || !Self::is_session_current(&state, &uri, &inputs.session)
            {
                return Ok(None);
            }
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

    /// The governing session, cached findings, buffer and buffer version
    /// for `uri`: everything `codeAction` needs, or `None` when any is
    /// missing.
    fn code_action_inputs(&self, uri: &Url) -> Option<CodeActionInputs> {
        let state = self.state.lock();
        if !state.open.contains(uri) {
            return None;
        }
        let session = state
            .doc_config
            .get(uri)
            .filter(|c| !state.broken.contains_key(*c))
            .and_then(|c| state.sessions.get(c).cloned())?;
        Some(CodeActionInputs {
            session,
            findings: state.diagnostics.get(uri).cloned()?,
            text: state.documents.get(uri).cloned()?,
            version: state.versions.get(uri).copied(),
            seq: state.doc_seq.get(uri).copied(),
            related_information_supported: state.related_information_supported,
        })
    }
}

/// What `codeAction` computes its fixes from, read under the state lock.
struct CodeActionInputs {
    session: Arc<Session>,
    findings: Vec<Finding>,
    text: String,
    /// The buffer's version, to drop edits computed for an older buffer.
    version: Option<i32>,
    /// The server-assigned sequence, guarding against close/reopen version reuse.
    seq: Option<u64>,
    related_information_supported: bool,
}

/// The quick-fixes for the findings of `abs` that overlap `selection`, and
/// whether each is Unsafe-tier; `None` when the file gets no fix at all.
/// Runs on a worker thread with an 8 MiB stack.
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
/// engine). Each config file gets its own anchored findings - published
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
    let mut seen_broken_logs: HashSet<String> = HashSet::new();
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
                let walk_warnings: Vec<String> = run
                    .session
                    .index
                    .walk_warnings()
                    .iter()
                    .map(|warning| run.session.redact(warning))
                    .collect();
                if state.last_walk_warnings.get(&key) != Some(&walk_warnings) {
                    for warning in &walk_warnings {
                        applied.logs.push(warning.clone());
                    }
                    state.last_walk_warnings.insert(key.clone(), walk_warnings);
                }
                withheld |= run.session.withheld;
                state.sessions.insert(key.clone(), run.session);
                by_config.insert(key, run.by_path);
            }
            Err(error) => {
                withheld |= error.untrusted;
                if seen_broken_logs.insert(error.message.clone()) {
                    applied.logs.push(error.message.clone());
                }
                state.broken.insert(key, error);
            }
        }
    }
    // Prune dedup caches for sessions that are broken or no longer active so
    // removed workspace folders do not leak entries and recovered configs
    // re-log still-active walk warnings and notes.
    state
        .last_notes
        .retain(|k, _| state.sessions.contains_key(k));
    state
        .last_walk_warnings
        .retain(|k, _| state.sessions.contains_key(k));
    (by_config, withheld)
}

#[allow(clippy::too_many_lines)]
fn publish_config_diagnostics(
    state: &mut State,
    docs: &[DocSnapshot],
    edited_uris: &HashSet<Url>,
    by_config: &HashMap<SessionKey, FindingsByPath>,
    published: &mut HashSet<Url>,
    applied: &mut Applied,
) {
    // Config files: anchored findings, or the load/build error. Multiple
    // workspace roots can share the same `--config` file, so merge per config
    // URI in deterministic key order rather than overwriting earlier roots.
    let mut keys: Vec<SessionKey> = state
        .sessions
        .keys()
        .chain(state.broken.keys())
        .cloned()
        .collect();
    keys.sort_by(|a, b| a.config.cmp(&b.config).then_with(|| a.root.cmp(&b.root)));

    let snapshotted_docs: HashSet<&Url> = docs.iter().map(|(u, ..)| u).collect();
    let reeval_uris: HashSet<Url> = applied.reeval.iter().map(|(u, ..)| u.clone()).collect();

    let mut config_findings_by_uri: HashMap<Url, Vec<Finding>> = HashMap::new();
    let mut config_broken_by_uri: HashMap<Url, Vec<Diagnostic>> = HashMap::new();
    let mut config_uris: HashSet<Url> = HashSet::new();

    for key in keys {
        let Ok(uri) = Url::from_file_path(&key.config) else {
            continue;
        };
        config_uris.insert(uri.clone());
        if let Some(error) = state.broken.get(&key) {
            let diagnostic = Diagnostic {
                range: error
                    .range
                    .unwrap_or_else(|| Range::new(Position::new(0, 0), Position::new(0, 1))),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("alint".to_string()),
                message: error.message.clone(),
                ..Diagnostic::default()
            };
            let entry = config_broken_by_uri.entry(uri).or_default();
            if !entry
                .iter()
                .any(|d| d.range == diagnostic.range && d.message == diagnostic.message)
            {
                entry.push(diagnostic);
            }
        } else {
            let findings = by_config
                .get(&key)
                .and_then(|m| m.get(&key.config).cloned())
                .unwrap_or_default();
            let entry = config_findings_by_uri.entry(uri).or_default();
            for finding in findings {
                // Deduplicate identical load-time pattern hints when multiple
                // roots share the same `--config` file while preserving distinct
                // per-root anchored findings (e.g. `file_exists`).
                if finding.is_pattern_hint
                    && entry.iter().any(|existing| {
                        existing.is_pattern_hint
                            && existing.range == finding.range
                            && existing.severity == finding.severity
                            && existing.rule_id == finding.rule_id
                            && existing.message == finding.message
                    })
                {
                    continue;
                }
                entry.push(finding);
            }
        }
    }

    let mut ordered_config_uris: Vec<Url> = config_uris.iter().cloned().collect();
    ordered_config_uris.sort();
    for uri in ordered_config_uris {
        let mut findings = config_findings_by_uri.remove(&uri).unwrap_or_default();
        let broken_diags = config_broken_by_uri.remove(&uri).unwrap_or_default();

        // If this config file was open (potentially with unsaved edits) when
        // the check started and closed before `apply_check` ran, strip overlaid
        // per-file findings and re-run from disk so discarded edits never stick.
        if snapshotted_docs.contains(&uri) && !state.open.contains(&uri) {
            findings.retain(|f| !f.per_file);
            applied.recheck = true;
        }

        if reeval_uris.contains(&uri) {
            // An open config file edited while this full check ran: preserve
            // the newer buffer's cached per-file findings and merge ONLY fresh
            // non-per-file findings (anchored rules, pattern hints, cross-file).
            // Never overwrite with stale `v1` per-file findings or stamp them
            // with `version: Some(2)`.
            let mut merged: Vec<Finding> = state
                .diagnostics
                .get(&uri)
                .map(|prev| prev.iter().filter(|f| f.per_file).cloned().collect())
                .unwrap_or_default();
            let non_per_file: Vec<Finding> = findings.into_iter().filter(|f| !f.per_file).collect();
            merged.extend(non_per_file.iter().cloned());
            if merged.is_empty() {
                state.diagnostics.remove(&uri);
            } else {
                state.diagnostics.insert(uri.clone(), merged);
            }
            if !broken_diags.is_empty() {
                let mut diagnostics = broken_diags;
                diagnostics.extend(findings_to_diagnostics(
                    &non_per_file,
                    state.related_information_supported,
                ));
                if published.insert(uri.clone()) {
                    applied.publish.push((uri, diagnostics, None));
                } else if let Some(entry) = applied.publish.iter_mut().find(|(u, _, _)| *u == uri) {
                    entry.1 = diagnostics;
                    entry.2 = None;
                }
            }
            continue;
        }

        let mut diagnostics = broken_diags;
        diagnostics.extend(findings_to_diagnostics(
            &findings,
            state.related_information_supported,
        ));
        if findings.is_empty() {
            state.diagnostics.remove(&uri);
        } else {
            state.diagnostics.insert(uri.clone(), findings);
        }
        // Never stamp on-disk config diagnostics (parse errors, pattern hints,
        // or anchored findings) with a newer unsaved buffer's version.
        let version = if edited_uris.contains(&uri) {
            None
        } else {
            state.versions.get(&uri).copied()
        };
        if published.insert(uri.clone()) {
            applied.publish.push((uri, diagnostics, version));
        } else if let Some(entry) = applied.publish.iter_mut().find(|(u, _, _)| *u == uri) {
            // An open config file: merged findings / load errors replace the
            // per-doc entry.
            entry.1 = diagnostics;
            entry.2 = version;
        }
    }
    // Config files we published to before that are no longer relevant and were
    // not already refreshed or queued for re-evaluation in this check.
    let mut no_longer_configs: Vec<Url> = state
        .config_uris
        .difference(&config_uris)
        .filter(|u| !published.contains(*u) && !reeval_uris.contains(*u))
        .cloned()
        .collect();
    no_longer_configs.sort();
    for uri in no_longer_configs {
        state.diagnostics.remove(&uri);
        let version = state.versions.get(&uri).copied();
        applied.publish.push((uri, Vec::new(), version));
    }
    state.config_uris = config_uris;
}

fn apply_check(
    state: &mut State,
    docs: &[DocSnapshot],
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
    let mut doc_config = check.doc_config;
    doc_config.retain(|uri, _| state.open.contains(uri));
    state.doc_config = doc_config;

    let mut published: HashSet<Url> = HashSet::new();
    let mut edited_uris: HashSet<Url> = HashSet::new();
    for (uri, _, snapshot_version, snapshot_seq) in docs {
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
        let current_version = state.versions.get(uri).copied();
        let current_seq = state.doc_seq.get(uri).copied();
        let edited_during_check =
            current_version != *snapshot_version || current_seq != *snapshot_seq;
        if edited_during_check {
            edited_uris.insert(uri.clone());
            // The buffer moved on while the full check ran: never overwrite
            // cached per-file findings with stale snapshot findings or publish
            // a backwards version. Keep the existing cached per-file findings
            // for the newer buffer and merge ONLY the fresh cross-file findings
            // (or clear immediately when no healthy session governs this file).
            let has_healthy_session = state
                .doc_config
                .get(uri)
                .is_some_and(|c| !state.broken.contains_key(c) && state.sessions.contains_key(c));
            if has_healthy_session {
                let mut merged: Vec<Finding> = state
                    .diagnostics
                    .get(uri)
                    .map(|prev| prev.iter().filter(|f| f.per_file).cloned().collect())
                    .unwrap_or_default();
                merged.extend(findings.into_iter().filter(|f| !f.per_file));
                if merged.is_empty() {
                    state.diagnostics.remove(uri);
                } else {
                    state.diagnostics.insert(uri.clone(), merged);
                }
                if let (Some(version), Some(text)) = (current_version, state.documents.get(uri)) {
                    applied
                        .reeval
                        .push((uri.clone(), text.clone(), version, current_seq));
                }
            } else {
                state.diagnostics.remove(uri);
                published.insert(uri.clone());
                applied
                    .publish
                    .push((uri.clone(), Vec::new(), current_version));
            }
        } else {
            let diagnostics =
                findings_to_diagnostics(&findings, state.related_information_supported);
            if findings.is_empty() {
                state.diagnostics.remove(uri);
            } else {
                state.diagnostics.insert(uri.clone(), findings);
            }
            published.insert(uri.clone());
            applied
                .publish
                .push((uri.clone(), diagnostics, *snapshot_version));
        }
    }

    publish_config_diagnostics(
        state,
        docs,
        &edited_uris,
        &by_config,
        &mut published,
        &mut applied,
    );
    applied
}
