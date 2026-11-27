//! In-memory protocol lifecycle and concurrency tests driving [`run_connection`]
//! over [`lsp_server::Connection::memory`] and `bounded(0)` rendezvous pairs.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use alint_core::{
    ContentAccess, Context, DispatchClass, FixContext, FixEdit, FixOutcome, Fixer,
    GitTrackedSupport, KindMeta, Level, PathsSupport, Rule, RuleSpec, ScopeFilterSupport,
    SinceSupport, TraceMeta, Violation,
};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId};
use lsp_types::notification::{
    Cancel, DidChangeTextDocument, DidChangeWorkspaceFolders, DidCloseTextDocument,
    DidOpenTextDocument, DidSaveTextDocument, Exit, Initialized, LogMessage, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{CodeActionRequest, HoverRequest, Initialize, Request as _, Shutdown};
use lsp_types::{
    CancelParams, CodeActionContext, CodeActionKind, CodeActionParams, CodeActionResponse,
    DidChangeTextDocumentParams, DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, Hover, HoverContents, HoverParams,
    InitializeParams, InitializeResult, InitializedParams, LogMessageParams, NumberOrString,
    PartialResultParams, Position, PublishDiagnosticsParams, Range, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams, WorkspaceFolder,
    WorkspaceFoldersChangeEvent,
};
use parking_lot::{Condvar, Mutex};

use super::{LspOptions, ServerCore, run_connection, run_connection_outcome};

const RECV_TIMEOUT: Duration = Duration::from_secs(10);

fn next_hook_id(prefix: &str) -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    format!("{prefix}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

struct CheckHookGuard {
    key: String,
    gate: Arc<Gate>,
}

impl CheckHookGuard {
    fn register(prefix: &str, hook: CheckHook) -> Self {
        let key = next_hook_id(prefix);
        let gate = Arc::clone(&hook.gate);
        check_hooks().lock().insert(key.clone(), hook);
        Self { key, gate }
    }
}

impl Drop for CheckHookGuard {
    fn drop(&mut self) {
        self.gate.release();
        check_hooks().lock().remove(&self.key);
    }
}

struct FixableHookGuard {
    key: String,
    gate: Arc<Gate>,
}

impl FixableHookGuard {
    fn register(prefix: &str, hook: FixableHook) -> Self {
        let key = next_hook_id(prefix);
        let gate = Arc::clone(&hook.gate);
        fixable_hooks().lock().insert(key.clone(), hook);
        Self { key, gate }
    }
}

impl Drop for FixableHookGuard {
    fn drop(&mut self) {
        self.gate.release();
        fixable_hooks().lock().remove(&self.key);
    }
}

fn send_request<R: lsp_types::request::Request>(conn: &Connection, id: i32, params: &R::Params) {
    conn.sender
        .send(Message::Request(Request::new(
            RequestId::from(id),
            R::METHOD.to_string(),
            serde_json::to_value(params).unwrap(),
        )))
        .unwrap();
}

fn send_notification<N: lsp_types::notification::Notification>(
    conn: &Connection,
    params: &N::Params,
) {
    conn.sender
        .send(Message::Notification(Notification::new(
            N::METHOD.to_string(),
            serde_json::to_value(params).unwrap(),
        )))
        .unwrap();
}

fn recv_response(conn: &Connection, id: i32) -> lsp_server::Response {
    let want = RequestId::from(id);
    loop {
        let msg = conn
            .receiver
            .recv_timeout(RECV_TIMEOUT)
            .expect("timed out waiting for response");
        if let Message::Response(resp) = msg
            && resp.id == want
        {
            return resp;
        }
    }
}

fn recv_diagnostics(conn: &Connection, uri: &Url) -> PublishDiagnosticsParams {
    loop {
        let msg = conn
            .receiver
            .recv_timeout(RECV_TIMEOUT)
            .expect("timed out waiting for publishDiagnostics");
        if let Message::Notification(notif) = msg
            && notif.method == PublishDiagnostics::METHOD
        {
            let params: PublishDiagnosticsParams = serde_json::from_value(notif.params).unwrap();
            if &params.uri == uri {
                return params;
            }
        }
    }
}

#[cfg(unix)]
fn collect_gitignore_warnings_until_publish(conn: &Connection, uri: &Url) -> Vec<String> {
    let mut warnings = Vec::new();
    loop {
        let msg = conn
            .receiver
            .recv_timeout(RECV_TIMEOUT)
            .expect("expected publishDiagnostics");
        if let Message::Notification(notif) = msg {
            if notif.method == LogMessage::METHOD {
                let params: LogMessageParams = serde_json::from_value(notif.params).unwrap();
                if params.message.contains(".gitignore") {
                    warnings.push(params.message);
                }
            } else if notif.method == PublishDiagnostics::METHOD {
                let params: PublishDiagnosticsParams =
                    serde_json::from_value(notif.params).unwrap();
                if &params.uri == uri {
                    return warnings;
                }
            }
        }
    }
}

fn initialize_folder(conn: &Connection, root: &Path) -> InitializeResult {
    send_request::<Initialize>(
        conn,
        1,
        &InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(root).unwrap(),
                name: "ws".to_string(),
            }]),
            ..InitializeParams::default()
        },
    );
    let init_resp = recv_response(conn, 1);
    let init: InitializeResult =
        serde_json::from_value(init_resp.response_result.unwrap()).unwrap();
    send_notification::<Initialized>(conn, &InitializedParams {});
    init
}

/// Pair of `Connection`s backed by `bounded(0)` rendezvous channels, matching
/// `Connection::stdio()`'s zero-buffer backpressure semantics.
fn rendezvous_connections() -> (Connection, Connection) {
    let (s_tx, c_rx) = crossbeam_channel::bounded::<Message>(0);
    let (c_tx, s_rx) = crossbeam_channel::bounded::<Message>(0);
    (
        Connection {
            sender: s_tx,
            receiver: s_rx,
        },
        Connection {
            sender: c_tx,
            receiver: c_rx,
        },
    )
}

/// Reusable deterministic latch for synchronizing worker-thread tests.
#[derive(Debug, Default)]
struct Gate {
    entered: (Mutex<bool>, Condvar),
    released: (Mutex<bool>, Condvar),
}

impl Gate {
    fn enter_and_wait(&self) {
        {
            let mut entered = self.entered.0.lock();
            *entered = true;
            self.entered.1.notify_all();
        }
        let mut released = self.released.0.lock();
        while !*released {
            self.released.1.wait(&mut released);
        }
    }

    fn wait_entered(&self) {
        let mut entered = self.entered.0.lock();
        while !*entered {
            self.entered.1.wait(&mut entered);
        }
    }

    fn release(&self) {
        let mut released = self.released.0.lock();
        *released = true;
        self.released.1.notify_all();
    }
}

static TEST_RULE_META: KindMeta = KindMeta {
    dispatch: DispatchClass::SingleShot,
    paths: PathsSupport::Forbidden,
    scope_filter: ScopeFilterSupport::Rejected {
        reason: Cow::Borrowed("test rule"),
    },
    git_tracked_only: GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: Cow::Borrowed(&[]),
    since: SinceSupport::None,
    capabilities: Cow::Borrowed(&[]),
    content: ContentAccess::PathOnly,
    formats: Cow::Borrowed(&[]),
    dependencies: Cow::Borrowed(&[]),
    fix_ops: Cow::Borrowed(&[]),
    expect_matches: false,
    trace: TraceMeta::ASSERTION,
    nested_rules: false,
};

static TEST_FIXABLE_META: KindMeta = KindMeta {
    dispatch: DispatchClass::SingleShot,
    paths: PathsSupport::Forbidden,
    scope_filter: ScopeFilterSupport::Rejected {
        reason: Cow::Borrowed("test rule"),
    },
    git_tracked_only: GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: Cow::Borrowed(&[]),
    since: SinceSupport::None,
    capabilities: Cow::Borrowed(&[]),
    content: ContentAccess::PathOnly,
    formats: Cow::Borrowed(&[]),
    dependencies: Cow::Borrowed(&[]),
    fix_ops: Cow::Borrowed(&[Cow::Borrowed("set_content")]),
    expect_matches: false,
    trace: TraceMeta::ASSERTION,
    nested_rules: false,
};

#[derive(Clone)]
struct CheckHook {
    calls: Arc<AtomicUsize>,
    block_on_call: usize,
    gate: Arc<Gate>,
}

#[derive(Clone)]
struct FixableHook {
    gate: Arc<Gate>,
    should_panic: bool,
    panic_on_eval: Arc<AtomicBool>,
}

fn check_hooks() -> &'static Mutex<HashMap<String, CheckHook>> {
    static HOOKS: OnceLock<Mutex<HashMap<String, CheckHook>>> = OnceLock::new();
    HOOKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fixable_hooks() -> &'static Mutex<HashMap<String, FixableHook>> {
    static HOOKS: OnceLock<Mutex<HashMap<String, FixableHook>>> = OnceLock::new();
    HOOKS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[allow(clippy::unnecessary_wraps)]
fn build_gated_check(spec: &RuleSpec) -> alint_core::Result<Box<dyn Rule>> {
    let hook = check_hooks()
        .lock()
        .get(&spec.id)
        .cloned()
        .expect("missing check hook");
    Ok(Box::new(GatedCheckRule {
        id: spec.id.clone(),
        level: spec.level,
        calls: hook.calls,
        block_on_call: hook.block_on_call,
        gate: hook.gate,
    }))
}

#[allow(clippy::unnecessary_wraps)]
fn build_gated_fixable(spec: &RuleSpec) -> alint_core::Result<Box<dyn Rule>> {
    let hook = fixable_hooks()
        .lock()
        .get(&spec.id)
        .cloned()
        .expect("missing fixable hook");
    Ok(Box::new(FixableFileRule {
        id: spec.id.clone(),
        level: spec.level,
        fixer: GatedFixer {
            gate: hook.gate,
            should_panic: hook.should_panic,
        },
        panic_on_eval: hook.panic_on_eval,
    }))
}

#[derive(Debug)]
struct GatedCheckRule {
    id: String,
    level: Level,
    calls: Arc<AtomicUsize>,
    block_on_call: usize,
    gate: Arc<Gate>,
}

impl Rule for GatedCheckRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn level(&self) -> Level {
        self.level
    }

    fn evaluate(&self, _ctx: &Context<'_>) -> alint_core::Result<Vec<Violation>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.block_on_call {
            self.gate.enter_and_wait();
        }
        Ok(Vec::new())
    }
}

#[derive(Debug)]
struct GatedFixer {
    gate: Arc<Gate>,
    should_panic: bool,
}

impl Fixer for GatedFixer {
    fn describe(&self) -> String {
        "gated test fix".to_string()
    }

    fn apply(&self, _v: &Violation, _ctx: &FixContext<'_>) -> alint_core::Result<FixOutcome> {
        Ok(FixOutcome::Applied("fixed".to_string()))
    }

    fn fix_edit(&self, violation: &Violation, _bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        self.gate.enter_and_wait();
        assert!(!self.should_panic, "injected fixer panic");
        Some(FixEdit::SetContent {
            path: violation.path.clone()?.to_path_buf(),
            content: b"FIXED\n".to_vec(),
        })
    }
}

#[derive(Debug)]
struct FixableFileRule {
    id: String,
    level: Level,
    fixer: GatedFixer,
    panic_on_eval: Arc<AtomicBool>,
}

impl Rule for FixableFileRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn level(&self) -> Level {
        self.level
    }

    fn evaluate(&self, _ctx: &Context<'_>) -> alint_core::Result<Vec<Violation>> {
        assert!(
            !self.panic_on_eval.load(Ordering::SeqCst),
            "injected rule panic"
        );
        let mut violation = Violation::new("needs gated fix").with_path(Path::new("notes.txt"));
        violation.line = Some(1);
        Ok(vec![violation])
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        Some(&self.fixer)
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn connection_lifecycle_open_hover_code_action_change_close_shutdown_exit() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n    \
         fix: { replace: { replacement: \"DONE\" } }\n",
    )
    .unwrap();
    let doc_path = root.join("notes.txt");
    let dirty = "x TODO\n";
    std::fs::write(&doc_path, dirty).unwrap();
    let doc_uri = Url::from_file_path(&doc_path).unwrap();

    let (server_conn, client_conn) = Connection::memory();
    let server = std::thread::spawn(move || run_connection(&server_conn, LspOptions::default()));

    let init = initialize_folder(&client_conn, root);
    assert_eq!(init.server_info.unwrap().name, "alint-lsp");

    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri.clone(),
                language_id: "plaintext".to_string(),
                version: 1,
                text: dirty.to_string(),
            },
        },
    );

    let published = recv_diagnostics(&client_conn, &doc_uri);
    assert_eq!(published.version, Some(1));
    assert_eq!(published.diagnostics.len(), 1);
    let diag_range = published.diagnostics[0].range;

    // Hover over diagnostic renders Markdown naming the rule id.
    send_request::<HoverRequest>(
        &client_conn,
        2,
        &HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: doc_uri.clone(),
                },
                position: diag_range.start,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        },
    );
    let hover_resp = recv_response(&client_conn, 2);
    let hover: Option<Hover> = serde_json::from_value(hover_resp.response_result.unwrap()).unwrap();
    let HoverContents::Markup(markup) = hover.expect("expected hover over diagnostic").contents
    else {
        panic!("expected Markdown hover contents");
    };
    assert!(
        markup.value.contains("no-todo"),
        "hover markdown should name rule id: {}",
        markup.value
    );

    // Hover outside the diagnostic range returns null.
    send_request::<HoverRequest>(
        &client_conn,
        20,
        &HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: doc_uri.clone(),
                },
                position: Position::new(5, 0),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        },
    );
    let miss_hover: Option<Hover> =
        serde_json::from_value(recv_response(&client_conn, 20).response_result.unwrap()).unwrap();
    assert!(miss_hover.is_none());

    // CodeAction filtered to a non-quickfix kind returns null.
    send_request::<CodeActionRequest>(
        &client_conn,
        30,
        &CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
            range: diag_range,
            context: CodeActionContext {
                only: Some(vec![CodeActionKind::SOURCE_ORGANIZE_IMPORTS]),
                ..CodeActionContext::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let filtered_actions: Option<CodeActionResponse> =
        serde_json::from_value(recv_response(&client_conn, 30).response_result.unwrap()).unwrap();
    assert!(filtered_actions.is_none());

    // CodeAction admitting quickfix returns the fix.
    send_request::<CodeActionRequest>(
        &client_conn,
        3,
        &CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
            range: diag_range,
            context: CodeActionContext {
                only: Some(vec![CodeActionKind::QUICKFIX]),
                ..CodeActionContext::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let action_resp = recv_response(&client_conn, 3);
    let actions: Option<CodeActionResponse> =
        serde_json::from_value(action_resp.response_result.unwrap()).unwrap();
    assert_eq!(actions.unwrap().len(), 1);

    // Incremental ranged didChange is ignored cleanly on a FULL-sync server.
    send_notification::<DidChangeTextDocument>(
        &client_conn,
        &DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: doc_uri.clone(),
                version: 99,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 0), Position::new(0, 1))),
                range_length: Some(1),
                text: "y".to_string(),
            }],
        },
    );

    send_notification::<DidChangeTextDocument>(
        &client_conn,
        &DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: doc_uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "x DONE\n".to_string(),
            }],
        },
    );
    let after_change = recv_diagnostics(&client_conn, &doc_uri);
    assert_eq!(after_change.version, Some(2));
    assert_eq!(after_change.diagnostics, Vec::new());

    send_notification::<DidCloseTextDocument>(
        &client_conn,
        &DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
        },
    );
    let after_close = recv_diagnostics(&client_conn, &doc_uri);
    assert_eq!(after_close.diagnostics, Vec::new());

    // didChange on an already-closed URI is ignored cleanly without crashing.
    send_notification::<DidChangeTextDocument>(
        &client_conn,
        &DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: doc_uri.clone(),
                version: 3,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "TODO\n".to_string(),
            }],
        },
    );

    send_request::<Shutdown>(&client_conn, 4, &());
    assert!(recv_response(&client_conn, 4).response_result.is_ok());
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[test]
fn connection_exit_without_shutdown_returns_one_and_eof_returns_zero() {
    // 1. Exit without shutdown -> exit code 1.
    let (server_conn, client_conn) = Connection::memory();
    let server = std::thread::spawn(move || run_connection(&server_conn, LspOptions::default()));

    send_request::<Initialize>(&client_conn, 1, &InitializeParams::default());
    let _ = recv_response(&client_conn, 1);
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 1);

    // 2. Clean EOF (client drops connection without exit) -> exit code 0.
    let (server_conn, client_conn) = Connection::memory();
    let server = std::thread::spawn(move || run_connection(&server_conn, LspOptions::default()));
    send_request::<Initialize>(&client_conn, 1, &InitializeParams::default());
    let _ = recv_response(&client_conn, 1);
    drop(client_conn);
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[test]
fn connection_enforces_lifecycle_and_invalid_params_error_codes() {
    let (server_conn, client_conn) = Connection::memory();
    let server = std::thread::spawn(move || run_connection(&server_conn, LspOptions::default()));

    // Request before initialize -> ServerNotInitialized.
    client_conn
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(1),
            "workspace/symbol".to_string(),
            serde_json::json!({}),
        )))
        .unwrap();
    assert_eq!(
        recv_response(&client_conn, 1)
            .response_result
            .unwrap_err()
            .code,
        ErrorCode::ServerNotInitialized as i32
    );

    // Shutdown before initialize -> ServerNotInitialized (LSP 3.17).
    send_request::<Shutdown>(&client_conn, 10, &());
    assert_eq!(
        recv_response(&client_conn, 10)
            .response_result
            .unwrap_err()
            .code,
        ErrorCode::ServerNotInitialized as i32
    );

    // Malformed initialize params -> InvalidParams.
    client_conn
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(11),
            Initialize::METHOD.to_string(),
            serde_json::json!("not-an-object"),
        )))
        .unwrap();
    assert_eq!(
        recv_response(&client_conn, 11)
            .response_result
            .unwrap_err()
            .code,
        ErrorCode::InvalidParams as i32
    );

    // Initialize cleanly.
    send_request::<Initialize>(&client_conn, 2, &InitializeParams::default());
    assert!(recv_response(&client_conn, 2).response_result.is_ok());

    // Duplicate initialize -> InvalidRequest.
    send_request::<Initialize>(&client_conn, 3, &InitializeParams::default());
    assert_eq!(
        recv_response(&client_conn, 3)
            .response_result
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest as i32
    );

    // Malformed hover and codeAction params -> InvalidParams.
    for (id, method) in [(12, HoverRequest::METHOD), (13, CodeActionRequest::METHOD)] {
        client_conn
            .sender
            .send(Message::Request(Request::new(
                RequestId::from(id),
                method.to_string(),
                serde_json::json!({ "wrong": true }),
            )))
            .unwrap();
        assert_eq!(
            recv_response(&client_conn, id)
                .response_result
                .unwrap_err()
                .code,
            ErrorCode::InvalidParams as i32
        );
    }

    // Unknown method -> MethodNotFound.
    client_conn
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(4),
            "alint/doesNotExist".to_string(),
            serde_json::json!({}),
        )))
        .unwrap();
    assert_eq!(
        recv_response(&client_conn, 4)
            .response_result
            .unwrap_err()
            .code,
        ErrorCode::MethodNotFound as i32
    );

    // Shutdown succeeds once, then rejects subsequent requests with InvalidRequest.
    send_request::<Shutdown>(&client_conn, 5, &());
    assert!(recv_response(&client_conn, 5).response_result.is_ok());

    send_request::<Shutdown>(&client_conn, 6, &());
    assert_eq!(
        recv_response(&client_conn, 6)
            .response_result
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest as i32
    );

    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[test]
fn rendezvous_bounded_zero_connection_does_not_deadlock_on_shutdown_or_exit() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n",
    )
    .unwrap();
    let doc_path = root.join("notes.txt");
    std::fs::write(&doc_path, "TODO\n").unwrap();
    let doc_uri = Url::from_file_path(&doc_path).unwrap();

    // Case 1: bounded(0) connection (matching Connection::stdio), client sends
    // open + shutdown, reads shutdown Ok, then stops reading and sends exit.
    let (server_conn, client_conn) = rendezvous_connections();
    let server = std::thread::spawn(move || run_connection(&server_conn, LspOptions::default()));

    let _ = initialize_folder(&client_conn, root);
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri.clone(),
                language_id: "plaintext".to_string(),
                version: 1,
                text: "TODO\n".to_string(),
            },
        },
    );
    let _ = recv_diagnostics(&client_conn, &doc_uri);
    send_request::<Shutdown>(&client_conn, 2, &());
    assert!(recv_response(&client_conn, 2).response_result.is_ok());
    // Stop reading completely and send exit while keeping client_conn alive.
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
    // No notifications may have been sent after shutdown Ok.
    assert!(client_conn.receiver.try_recv().is_err());

    // Case 2: bounded(0) connection where client drains the ready log, fires
    // an open, and immediately sends exit without shutdown.
    let (server_conn, client_conn) = rendezvous_connections();
    let server =
        std::thread::spawn(move || run_connection_outcome(&server_conn, LspOptions::default()));
    let _ = initialize_folder(&client_conn, root);
    let _ = client_conn.receiver.recv_timeout(RECV_TIMEOUT);
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri,
                language_id: "plaintext".to_string(),
                version: 1,
                text: "TODO\n".to_string(),
            },
        },
    );
    send_notification::<Exit>(&client_conn, &());
    drop(client_conn.receiver);
    let outcome = server.join().unwrap().unwrap();
    assert_eq!(outcome.code, 1);
    assert!(outcome.relay_drained);
    drop(client_conn.sender);
}

#[test]
#[allow(clippy::too_many_lines)]
fn slow_full_check_never_starves_did_change_or_code_action_and_never_publishes_backwards() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let gate = Arc::new(Gate::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_guard = CheckHookGuard::register(
        "gated",
        CheckHook {
            calls,
            block_on_call: 2,
            gate: Arc::clone(&gate),
        },
    );
    std::fs::write(
        root.join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: {}\n    kind: gated_check\n    level: error\n  \
             - id: no-todo\n    kind: file_content_forbidden\n    paths: \"*.txt\"\n    \
             pattern: \"TODO\"\n    level: error\n    fix: {{ replace: {{ replacement: \"DONE\" }} }}\n",
            hook_guard.key
        ),
    )
    .unwrap();
    let doc_path = root.join("notes.txt");
    std::fs::write(&doc_path, "TODO\n").unwrap();
    let doc_uri = Url::from_file_path(&doc_path).unwrap();

    let mut registry = alint_rules::builtin_registry();
    registry.register("gated_check", &TEST_RULE_META, build_gated_check);

    let (server_conn, client_conn) = Connection::memory();
    let options = LspOptions::default().with_registry(Some(registry));
    let server = std::thread::spawn(move || run_connection(&server_conn, options));

    let _ = initialize_folder(&client_conn, root);
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri.clone(),
                language_id: "plaintext".to_string(),
                version: 1,
                text: "TODO\n".to_string(),
            },
        },
    );
    let initial = recv_diagnostics(&client_conn, &doc_uri);
    assert_eq!(initial.version, Some(1));
    assert_eq!(initial.diagnostics.len(), 1);
    let diag_range = initial.diagnostics[0].range;

    // Trigger a burst of full checks: the 2nd check blocks inside `gate`.
    for _ in 0..6 {
        send_notification::<DidSaveTextDocument>(
            &client_conn,
            &DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier {
                    uri: doc_uri.clone(),
                },
                text: None,
            },
        );
    }
    gate.wait_entered();

    // While `alint-lsp-check` is blocked, `codeAction` and `didChange` MUST
    // still run immediately on the interactive worker pool (H1).
    send_request::<CodeActionRequest>(
        &client_conn,
        2,
        &CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
            range: diag_range,
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let actions: Option<CodeActionResponse> =
        serde_json::from_value(recv_response(&client_conn, 2).response_result.unwrap()).unwrap();
    assert_eq!(actions.unwrap().len(), 1);

    // Edit to clean v2 while the slow full check (snapshotted at v1) is in flight.
    send_notification::<DidChangeTextDocument>(
        &client_conn,
        &DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: doc_uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "DONE\n".to_string(),
            }],
        },
    );
    let v2_diag = recv_diagnostics(&client_conn, &doc_uri);
    assert_eq!(v2_diag.version, Some(2));
    assert_eq!(v2_diag.diagnostics, Vec::new());

    // Release the slow full check; it must NEVER publish backwards version 1
    // diagnostics over v2 (M2).
    gate.release();

    let after_release = recv_diagnostics(&client_conn, &doc_uri);
    assert_eq!(
        after_release.version,
        Some(2),
        "full check finishing after edit must not publish backwards version 1"
    );
    assert_eq!(after_release.diagnostics, Vec::new());

    send_request::<Shutdown>(&client_conn, 3, &());
    assert!(recv_response(&client_conn, 3).response_result.is_ok());
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[test]
fn removing_last_workspace_folder_clears_state_and_diagnostics() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let cfg_path = root.join(".alint.yml");
    std::fs::write(
        &cfg_path,
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n",
    )
    .unwrap();
    let doc_path = root.join("notes.txt");
    std::fs::write(&doc_path, "TODO\n").unwrap();
    let doc_uri = Url::from_file_path(&doc_path).unwrap();
    let root_uri = Url::from_file_path(root).unwrap();

    let (server_conn, client_conn) = Connection::memory();
    let server = std::thread::spawn(move || run_connection(&server_conn, LspOptions::default()));

    let _ = initialize_folder(&client_conn, root);
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri.clone(),
                language_id: "plaintext".to_string(),
                version: 1,
                text: "TODO\n".to_string(),
            },
        },
    );
    assert_eq!(
        recv_diagnostics(&client_conn, &doc_uri).diagnostics.len(),
        1
    );

    // Remove the ONLY workspace folder.
    send_notification::<DidChangeWorkspaceFolders>(
        &client_conn,
        &DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: Vec::new(),
                removed: vec![WorkspaceFolder {
                    uri: root_uri,
                    name: "ws".to_string(),
                }],
            },
        },
    );
    let cleared = recv_diagnostics(&client_conn, &doc_uri);
    assert!(
        cleared.diagnostics.is_empty(),
        "removing last workspace folder must clear open document diagnostics"
    );

    // Subsequent edits to the file must NOT re-lint using stale session state.
    send_notification::<DidChangeTextDocument>(
        &client_conn,
        &DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: doc_uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "TODO again\n".to_string(),
            }],
        },
    );
    send_request::<Shutdown>(&client_conn, 2, &());
    // Drain messages until shutdown response; no diagnostic with findings may appear.
    loop {
        let msg = client_conn.receiver.recv_timeout(RECV_TIMEOUT).unwrap();
        match msg {
            Message::Response(resp) if resp.id == RequestId::from(2) => {
                assert!(resp.response_result.is_ok());
                break;
            }
            Message::Notification(notif) if notif.method == PublishDiagnostics::METHOD => {
                let params: PublishDiagnosticsParams =
                    serde_json::from_value(notif.params).unwrap();
                if params.uri == doc_uri {
                    assert!(
                        params.diagnostics.is_empty(),
                        "removed folder must not re-lint on didChange"
                    );
                }
            }
            _ => {}
        }
    }
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[test]
#[allow(clippy::too_many_lines)]
fn shutdown_waits_for_in_flight_code_action_and_cancel_returns_request_canceled() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // Part 1: `$/cancelRequest` cancels an in-flight codeAction -> RequestCanceled (-32800).
    let cancel_gate = Arc::new(Gate::default());
    let cancel_guard = FixableHookGuard::register(
        "gated-fix",
        FixableHook {
            gate: Arc::clone(&cancel_gate),
            should_panic: false,
            panic_on_eval: Arc::new(AtomicBool::new(false)),
        },
    );
    std::fs::write(
        root.join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: {}\n    kind: gated_fixable\n    level: error\n",
            cancel_guard.key
        ),
    )
    .unwrap();
    let doc_path = root.join("notes.txt");
    std::fs::write(&doc_path, "bad\n").unwrap();
    let doc_uri = Url::from_file_path(&doc_path).unwrap();

    let mut registry = alint_rules::builtin_registry();
    registry.register("gated_fixable", &TEST_FIXABLE_META, build_gated_fixable);

    let (server_conn, client_conn) = Connection::memory();
    let options = LspOptions::default().with_registry(Some(registry));
    let server = std::thread::spawn(move || run_connection(&server_conn, options));

    let _ = initialize_folder(&client_conn, root);
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri.clone(),
                language_id: "plaintext".to_string(),
                version: 1,
                text: "bad\n".to_string(),
            },
        },
    );
    let diag_range = recv_diagnostics(&client_conn, &doc_uri).diagnostics[0].range;

    send_request::<CodeActionRequest>(
        &client_conn,
        10,
        &CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
            range: diag_range,
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    cancel_gate.wait_entered();
    // Cancel an unknown id (must be harmless) and then cancel id 10.
    send_notification::<Cancel>(
        &client_conn,
        &CancelParams {
            id: NumberOrString::Number(9999),
        },
    );
    send_notification::<Cancel>(
        &client_conn,
        &CancelParams {
            id: NumberOrString::Number(10),
        },
    );
    // Sync barrier: `hover` is handled synchronously on the main loop in FIFO
    // order after `$/cancelRequest`, guaranteeing the cancel flag is recorded
    // before the worker resumes from `cancel_gate`.
    send_request::<HoverRequest>(
        &client_conn,
        100,
        &HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: doc_uri.clone(),
                },
                position: diag_range.start,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        },
    );
    assert!(recv_response(&client_conn, 100).response_result.is_ok());
    cancel_gate.release();
    let canceled = recv_response(&client_conn, 10);
    assert_eq!(
        canceled.response_result.unwrap_err().code,
        ErrorCode::RequestCanceled as i32
    );

    send_request::<Shutdown>(&client_conn, 11, &());
    assert!(recv_response(&client_conn, 11).response_result.is_ok());
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);

    // Part 2: `shutdown` waits for an in-flight `codeAction` to reply first (H3).
    let shutdown_gate = Arc::new(Gate::default());
    let shutdown_guard = FixableHookGuard::register(
        "gated-fix",
        FixableHook {
            gate: Arc::clone(&shutdown_gate),
            should_panic: false,
            panic_on_eval: Arc::new(AtomicBool::new(false)),
        },
    );
    std::fs::write(
        root.join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: {}\n    kind: gated_fixable\n    level: error\n",
            shutdown_guard.key
        ),
    )
    .unwrap();
    let mut registry2 = alint_rules::builtin_registry();
    registry2.register("gated_fixable", &TEST_FIXABLE_META, build_gated_fixable);

    let (server_conn, client_conn) = Connection::memory();
    let options = LspOptions::default().with_registry(Some(registry2));
    let server = std::thread::spawn(move || run_connection(&server_conn, options));
    let _ = initialize_folder(&client_conn, root);
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri.clone(),
                language_id: "plaintext".to_string(),
                version: 1,
                text: "bad\n".to_string(),
            },
        },
    );
    let diag_range = recv_diagnostics(&client_conn, &doc_uri).diagnostics[0].range;

    send_request::<CodeActionRequest>(
        &client_conn,
        20,
        &CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
            range: diag_range,
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    shutdown_gate.wait_entered();
    send_request::<Shutdown>(&client_conn, 21, &());
    // Shutdown must NOT answer while codeAction 20 is still blocked (ignore
    // any config-URI notification that was already queued before shutdown).
    let wait_until = std::time::Instant::now() + Duration::from_millis(100);
    while let Some(rem) = wait_until.checked_duration_since(std::time::Instant::now()) {
        match client_conn.receiver.recv_timeout(rem) {
            Ok(Message::Response(resp)) => {
                panic!("unexpected response while codeAction 20 is blocked: {resp:?}");
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    shutdown_gate.release();
    // Both responses arrive in order: codeAction 20 first, then shutdown 21.
    let mut order = Vec::new();
    while order.len() < 2 {
        let msg = client_conn.receiver.recv_timeout(RECV_TIMEOUT).unwrap();
        if let Message::Response(resp) = msg {
            assert!(resp.response_result.is_ok());
            order.push(resp.id);
        }
    }
    assert_eq!(order, vec![RequestId::from(20), RequestId::from(21)]);
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[test]
fn panicking_rule_and_fixer_recover_and_return_internal_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let gate = Arc::new(Gate::default());
    gate.release(); // non-blocking
    let panic_on_eval = Arc::new(AtomicBool::new(false));
    let boom_guard = FixableHookGuard::register(
        "boom",
        FixableHook {
            gate,
            should_panic: true,
            panic_on_eval: Arc::clone(&panic_on_eval),
        },
    );
    std::fs::write(
        root.join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: {}\n    kind: boom_fixable\n    level: error\n",
            boom_guard.key
        ),
    )
    .unwrap();
    let doc_path = root.join("notes.txt");
    std::fs::write(&doc_path, "bad\n").unwrap();
    let doc_uri = Url::from_file_path(&doc_path).unwrap();

    let mut registry = alint_rules::builtin_registry();
    registry.register("boom_fixable", &TEST_FIXABLE_META, build_gated_fixable);

    let (server_conn, client_conn) = Connection::memory();
    let options = LspOptions::default().with_registry(Some(registry));
    let server = std::thread::spawn(move || run_connection(&server_conn, options));
    let _ = initialize_folder(&client_conn, root);
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri.clone(),
                language_id: "plaintext".to_string(),
                version: 1,
                text: "bad\n".to_string(),
            },
        },
    );
    let diag_range = recv_diagnostics(&client_conn, &doc_uri).diagnostics[0].range;

    // Panicking fixer replies InternalError (-32603) instead of hanging or killing a worker.
    send_request::<CodeActionRequest>(
        &client_conn,
        2,
        &CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
            range: diag_range,
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let resp = recv_response(&client_conn, 2);
    assert_eq!(
        resp.response_result.unwrap_err().code,
        ErrorCode::InternalError as i32
    );

    // Panicking full check logs an error and keeps the server responsive.
    panic_on_eval.store(true, Ordering::SeqCst);
    send_notification::<DidSaveTextDocument>(
        &client_conn,
        &DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
            text: None,
        },
    );
    loop {
        let msg = client_conn.receiver.recv_timeout(RECV_TIMEOUT).unwrap();
        if let Message::Notification(notif) = msg
            && notif.method == LogMessage::METHOD
        {
            let params: LogMessageParams = serde_json::from_value(notif.params).unwrap();
            if params.message.contains("panicked") {
                break;
            }
        }
    }

    // Server still handles hover and shutdown cleanly after both panics.
    send_request::<HoverRequest>(
        &client_conn,
        3,
        &HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: doc_uri },
                position: diag_range.start,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        },
    );
    assert!(recv_response(&client_conn, 3).response_result.is_ok());

    send_request::<Shutdown>(&client_conn, 4, &());
    assert!(recv_response(&client_conn, 4).response_result.is_ok());
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[test]
fn stale_doc_seq_drops_close_reopen_same_version_reeval() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n",
    )
    .unwrap();
    let doc_path = root.join("notes.txt");
    std::fs::write(&doc_path, "clean\n").unwrap();
    let uri = Url::from_file_path(&doc_path).unwrap();
    let session = super::config::build_session(root).unwrap().unwrap();

    let (tx, _rx) = crossbeam_channel::unbounded();
    let core = ServerCore::new(tx, LspOptions::default());
    {
        let mut st = core.state.lock();
        let key = super::config::SessionKey {
            config: session.config_path.clone(),
            root: session.root.clone(),
        };
        st.folders = vec![root.to_path_buf()];
        st.doc_config.insert(uri.clone(), key.clone());
        st.sessions.insert(key, Arc::new(session));
        st.open.insert(uri.clone());
        // Simulate reopen at the SAME client `version = 1` with a newer server `seq = 2`.
        st.versions.insert(uri.clone(), 1);
        st.doc_seq.insert(uri.clone(), 2);
        st.documents.insert(uri.clone(), "clean\n".to_string());
    }
    // Stale job from prior open (`version = 1`, `seq = 1`) must be dropped.
    core.reeval_file_seq(uri.clone(), "TODO\n".to_string(), 1, Some(1));
    assert_eq!(core.evaluations.load(Ordering::SeqCst), 0);
    assert!(!core.state.lock().diagnostics.contains_key(&uri));

    // Current job (`version = 1`, `seq = 2`) runs normally.
    core.reeval_file_seq(uri.clone(), "TODO\n".to_string(), 1, Some(2));
    assert_eq!(core.evaluations.load(Ordering::SeqCst), 1);
    assert_eq!(core.state.lock().diagnostics[&uri].len(), 1);
}

#[test]
fn multi_root_shared_config_merges_anchored_diagnostics() {
    let tmp = tempfile::tempdir().unwrap();
    let shared_cfg = tmp.path().join("shared.alint.yml");
    std::fs::write(
        &shared_cfg,
        "version: 1\nrules:\n  - id: need-readme\n    kind: file_exists\n    \
         paths: README.md\n    level: error\n",
    )
    .unwrap();
    let root_a = tmp.path().join("a");
    let root_b = tmp.path().join("b");
    std::fs::create_dir(&root_a).unwrap();
    std::fs::create_dir(&root_b).unwrap();
    std::fs::write(root_a.join("a.txt"), "hi\n").unwrap();
    std::fs::write(root_b.join("b.txt"), "hi\n").unwrap();

    let cfg_uri = Url::from_file_path(&shared_cfg).unwrap();
    let a_uri = Url::from_file_path(root_a.join("a.txt")).unwrap();

    let (server_conn, client_conn) = Connection::memory();
    let options = LspOptions::default().with_config(Some(shared_cfg));
    let server = std::thread::spawn(move || run_connection(&server_conn, options));

    send_request::<Initialize>(
        &client_conn,
        1,
        &InitializeParams {
            workspace_folders: Some(vec![
                WorkspaceFolder {
                    uri: Url::from_file_path(&root_a).unwrap(),
                    name: "a".to_string(),
                },
                WorkspaceFolder {
                    uri: Url::from_file_path(&root_b).unwrap(),
                    name: "b".to_string(),
                },
            ]),
            ..InitializeParams::default()
        },
    );
    assert!(recv_response(&client_conn, 1).response_result.is_ok());
    send_notification::<Initialized>(&client_conn, &InitializedParams {});
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: a_uri,
                language_id: "plaintext".to_string(),
                version: 1,
                text: "hi\n".to_string(),
            },
        },
    );

    let cfg_diags = recv_diagnostics(&client_conn, &cfg_uri);
    assert_eq!(
        cfg_diags.diagnostics.len(),
        2,
        "both roots' config-anchored missing-README findings must merge on shared --config: {:?}",
        cfg_diags.diagnostics
    );

    send_request::<Shutdown>(&client_conn, 2, &());
    assert!(recv_response(&client_conn, 2).response_result.is_ok());
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[cfg(unix)]
#[test]
fn connection_logs_parent_ignore_walk_warnings_dedups_and_relogs_after_config_recovery() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = tmp.path();
    // Place a git repository at `outer` with a non-regular `.gitignore`
    // (symlink to /dev/zero) so walking the subdirectory `repo` hits the
    // parent-ignore warning (#568).
    std::fs::create_dir(outer.join(".git")).unwrap();
    std::fs::write(outer.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::os::unix::fs::symlink("/dev/zero", outer.join(".gitignore")).unwrap();
    let repo = outer.join("repo");
    std::fs::create_dir(&repo).unwrap();
    let valid_cfg = "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
                     paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n";
    let cfg_path = repo.join(".alint.yml");
    std::fs::write(&cfg_path, valid_cfg).unwrap();
    let doc_path = repo.join("a.txt");
    std::fs::write(&doc_path, "clean\n").unwrap();
    let doc_uri = Url::from_file_path(&doc_path).unwrap();

    let (server_conn, client_conn) = Connection::memory();
    let server = std::thread::spawn(move || run_connection(&server_conn, LspOptions::default()));

    let _ = initialize_folder(&client_conn, &repo);
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri.clone(),
                language_id: "plaintext".to_string(),
                version: 1,
                text: "clean\n".to_string(),
            },
        },
    );

    let first = collect_gitignore_warnings_until_publish(&client_conn, &doc_uri);
    assert_eq!(
        first.len(),
        1,
        "parent-ignore walk warning should be logged once: {first:?}"
    );

    // Repeat full check on save must not duplicate the unchanged warning.
    send_notification::<DidSaveTextDocument>(
        &client_conn,
        &DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
            text: None,
        },
    );
    let second = collect_gitignore_warnings_until_publish(&client_conn, &doc_uri);
    assert_eq!(
        second,
        Vec::<String>::new(),
        "unchanged walk warning must not repeat on save"
    );

    // Break the config (prunes `last_walk_warnings`), then restore it: warning re-logs!
    std::fs::write(&cfg_path, "version: 1\nrules: [unclosed\n").unwrap();
    send_notification::<DidSaveTextDocument>(
        &client_conn,
        &DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
            text: None,
        },
    );
    let _ = recv_diagnostics(&client_conn, &doc_uri);

    std::fs::write(&cfg_path, valid_cfg).unwrap();
    send_notification::<DidSaveTextDocument>(
        &client_conn,
        &DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: doc_uri.clone(),
            },
            text: None,
        },
    );
    let after_recovery = collect_gitignore_warnings_until_publish(&client_conn, &doc_uri);
    assert_eq!(
        after_recovery.len(),
        1,
        "walk warning should re-log after config recovers from broken state: {after_recovery:?}"
    );

    send_request::<Shutdown>(&client_conn, 2, &());
    let _ = recv_response(&client_conn, 2);
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[test]
fn initialize_advertises_text_document_sync_options_with_save() {
    use lsp_types::{
        TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncSaveOptions,
    };

    let (server_conn, client_conn) = Connection::memory();
    let server = std::thread::spawn(move || run_connection(&server_conn, LspOptions::default()));
    send_request::<Initialize>(&client_conn, 1, &InitializeParams::default());
    let resp = recv_response(&client_conn, 1);
    let init: InitializeResult = serde_json::from_value(resp.response_result.unwrap()).unwrap();
    let Some(TextDocumentSyncCapability::Options(opts)) = init.capabilities.text_document_sync
    else {
        panic!(
            "expected TextDocumentSyncCapability::Options, got: {:?}",
            init.capabilities.text_document_sync
        );
    };
    assert_eq!(opts.open_close, Some(true));
    assert_eq!(opts.change, Some(TextDocumentSyncKind::FULL));
    assert_eq!(
        opts.save,
        Some(TextDocumentSyncSaveOptions::Supported(true))
    );

    send_request::<Shutdown>(&client_conn, 2, &());
    let _ = recv_response(&client_conn, 2);
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[cfg(unix)]
#[test]
#[allow(clippy::too_many_lines)]
fn percent_encoded_client_uri_preserves_config_diagnostics_hover_did_close_and_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("dir:1");
    std::fs::create_dir(&repo).unwrap();
    let cfg_path = repo.join(".alint.yml");
    let cfg_text = "version: 1\nrules:\n  - id: need-readme\n    kind: file_exists\n    \
         paths: README.md\n    level: error\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: '*.txt'\n    pattern: 'TODO'\n    level: error\n    \
         fix: { replace: { replacement: 'DONE' } }\n";
    std::fs::write(&cfg_path, cfg_text).unwrap();
    let txt_path = repo.join("a:b.txt");
    std::fs::write(&txt_path, "x TODO\n").unwrap();

    // Construct non-canonical percent-encoded URIs (`:` -> `%3A`), modeling
    // VS Code on Windows (`file:///c%3A/...`) and clients that percent-encode `:`.
    let canonical_cfg_uri = Url::from_file_path(&cfg_path).unwrap();
    let encoded_cfg_uri =
        Url::parse(&canonical_cfg_uri.as_str().replace("dir:1", "dir%3A1")).unwrap();
    assert_ne!(canonical_cfg_uri, encoded_cfg_uri);
    let canonical_txt_uri = Url::from_file_path(&txt_path).unwrap();
    let encoded_txt_uri = Url::parse(
        &canonical_txt_uri
            .as_str()
            .replace("dir:1", "dir%3A1")
            .replace("a:b.txt", "a%3Ab.txt"),
    )
    .unwrap();
    assert_ne!(canonical_txt_uri, encoded_txt_uri);

    let (server_conn, client_conn) = Connection::memory();
    let server = std::thread::spawn(move || run_connection(&server_conn, LspOptions::default()));
    send_request::<Initialize>(
        &client_conn,
        1,
        &InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(&repo).unwrap(),
                name: "repo".to_string(),
            }]),
            ..InitializeParams::default()
        },
    );
    assert!(recv_response(&client_conn, 1).response_result.is_ok());
    send_notification::<Initialized>(&client_conn, &InitializedParams {});

    // Open `.alint.yml` under its percent-encoded client URI.
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: encoded_cfg_uri.clone(),
                language_id: "yaml".to_string(),
                version: 1,
                text: cfg_text.to_string(),
            },
        },
    );
    let cfg_diags = recv_diagnostics(&client_conn, &encoded_cfg_uri);
    assert_eq!(cfg_diags.diagnostics.len(), 1, "{cfg_diags:?}");

    // Hover on the config-anchored finding works under the client's exact URI.
    send_request::<HoverRequest>(
        &client_conn,
        2,
        &HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: encoded_cfg_uri.clone(),
                },
                position: Position::new(0, 0),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        },
    );
    let hover_resp = recv_response(&client_conn, 2);
    let hover: Option<Hover> = serde_json::from_value(hover_resp.response_result.unwrap()).unwrap();
    assert!(
        hover.is_some(),
        "hover must resolve under percent-encoded config URI"
    );

    // Open `a:b.txt` under its percent-encoded URI and check WorkspaceEdit key.
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: encoded_txt_uri.clone(),
                language_id: "plaintext".to_string(),
                version: 1,
                text: "x TODO\n".to_string(),
            },
        },
    );
    let txt_diags = recv_diagnostics(&client_conn, &encoded_txt_uri);
    assert_eq!(txt_diags.diagnostics.len(), 1, "{txt_diags:?}");
    send_request::<CodeActionRequest>(
        &client_conn,
        3,
        &CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: encoded_txt_uri.clone(),
            },
            range: txt_diags.diagnostics[0].range,
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let ca_resp = recv_response(&client_conn, 3);
    let actions: Option<CodeActionResponse> =
        serde_json::from_value(ca_resp.response_result.unwrap()).unwrap();
    let lsp_types::CodeActionOrCommand::CodeAction(action) = &actions.unwrap()[0] else {
        panic!("expected CodeAction");
    };
    let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
    assert!(
        changes.contains_key(&encoded_txt_uri),
        "WorkspaceEdit.changes must be keyed by client's exact URI {encoded_txt_uri}, got: {:?}",
        changes.keys().collect::<Vec<_>>()
    );

    // Closing `.alint.yml` under the percent-encoded URI must preserve config
    // diagnostics (republishing from disk to canonical URI, clearing old open URI).
    send_notification::<DidCloseTextDocument>(
        &client_conn,
        &DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: encoded_cfg_uri.clone(),
            },
        },
    );
    let after_close = recv_diagnostics(&client_conn, &canonical_cfg_uri);
    assert_eq!(
        after_close.diagnostics.len(),
        1,
        "closing percent-encoded config URI must preserve on-disk config diagnostics"
    );

    send_request::<Shutdown>(&client_conn, 4, &());
    let _ = recv_response(&client_conn, 4);
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[path = "protocol_tests_audit.rs"]
mod audit;
