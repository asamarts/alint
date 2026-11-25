//! LSP message loop, outbound relay, dedicated full-check thread, and
//! interactive worker pool driving [`ServerCore`].

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId};
use lsp_types::notification::{
    Cancel, DidChangeTextDocument, DidChangeWatchedFiles, DidChangeWorkspaceFolders,
    DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument, Exit, Initialized,
    Notification as LspNotification,
};
use lsp_types::request::{
    CodeActionRequest, HoverRequest, Initialize, Request as LspRequest, Shutdown,
};
use lsp_types::{
    CancelParams, CodeActionParams, DidChangeTextDocumentParams, DidChangeWorkspaceFoldersParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, HoverParams, InitializeParams,
    MessageType, NumberOrString, Url,
};
use parking_lot::{Condvar, Mutex};

use super::{
    EXIT_JOIN_TIMEOUT, LspOptions, ServerCore, WORKER_COUNT, WORKER_STACK_SIZE, exit_code,
    panic_summary,
};

/// Interactive work queued from the main LSP dispatch loop onto the 8 MiB
/// worker pool (`Reeval` and `CodeAction` only; full checks run on their own
/// dedicated thread).
enum Job {
    Reeval {
        uri: Url,
        text: String,
        version: i32,
        seq: u64,
    },
    CodeAction {
        id: RequestId,
        params: CodeActionParams,
    },
}

/// Convert an LSP `$/cancelRequest` `NumberOrString` id into `lsp_server::RequestId`.
fn cancel_id_to_request_id(id: NumberOrString) -> RequestId {
    match id {
        NumberOrString::Number(n) => RequestId::from(n),
        NumberOrString::String(s) => RequestId::from(s),
    }
}

/// Drop guard that decrements `active` and notifies `cvar` when a worker thread
/// finishes, so `run_connection` can join cleanly within a bounded grace window.
struct ThreadDoneGuard {
    done: Arc<(Mutex<usize>, Condvar)>,
}

impl Drop for ThreadDoneGuard {
    fn drop(&mut self) {
        let mut remaining = self.done.0.lock();
        *remaining = remaining.saturating_sub(1);
        if *remaining == 0 {
            self.done.1.notify_all();
        }
    }
}

/// Drop guard ensuring every accepted `codeAction` request clears its
/// `in_flight_code_actions` count and `pending_code_actions` entry even if a
/// worker panics or exits early.
struct CodeActionGuard<'a> {
    core: &'a ServerCore,
    id: RequestId,
}

impl Drop for CodeActionGuard<'_> {
    fn drop(&mut self) {
        self.core.finish_code_action(&self.id);
    }
}

/// Wait up to `timeout` for `remaining` threads to reach zero, returning `true`
/// if all finished in time.
fn wait_for_threads(done: &(Mutex<usize>, Condvar), timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let mut remaining = done.0.lock();
    while *remaining > 0 {
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let _ = done.1.wait_for(&mut remaining, deadline - now);
    }
    true
}

fn run_worker_job(core: &ServerCore, job: Job) {
    match job {
        Job::Reeval {
            uri,
            text,
            version,
            seq,
        } => {
            if core.is_stopping() {
                return;
            }
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                core.reeval_file_seq(uri, text, version, Some(seq));
            })) {
                tracing::error!(
                    panic = %panic_summary(&*payload),
                    "alint LSP reeval worker panicked"
                );
            }
        }
        Job::CodeAction { id, params } => {
            let _ca_guard = CodeActionGuard {
                core,
                id: id.clone(),
            };
            if core.exiting.load(Ordering::SeqCst) {
                return;
            }
            if core.is_canceled(&id) {
                core.send_err(
                    id,
                    ErrorCode::RequestCanceled,
                    "request canceled".to_string(),
                );
                return;
            }
            let res = catch_unwind(AssertUnwindSafe(|| core.compute_code_action(params)));
            if core.exiting.load(Ordering::SeqCst) {
                return;
            }
            if core.is_canceled(&id) {
                core.send_err(
                    id,
                    ErrorCode::RequestCanceled,
                    "request canceled".to_string(),
                );
                return;
            }
            match res {
                Ok(Ok(resp)) => {
                    let value = serde_json::to_value(resp).unwrap_or(serde_json::Value::Null);
                    core.send_ok(id, value);
                }
                Ok(Err(msg)) => core.send_err(id, ErrorCode::InternalError, msg),
                Err(payload) => {
                    tracing::error!(
                        panic = %panic_summary(&*payload),
                        "alint LSP codeAction worker panicked"
                    );
                    core.send_err(
                        id,
                        ErrorCode::InternalError,
                        "alint: code action panicked".to_string(),
                    );
                }
            }
        }
    }
}

fn spawn_outbound_relay(
    core: Arc<ServerCore>,
    conn_tx: Sender<Message>,
    out_rx: Receiver<Message>,
    done: Arc<(Mutex<usize>, Condvar)>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("alint-lsp-outbound".to_string())
        .spawn(move || {
            let _guard = ThreadDoneGuard { done };
            while let Ok(msg) = out_rx.recv() {
                if core.exiting.load(Ordering::SeqCst) && !matches!(msg, Message::Response(_)) {
                    continue;
                }
                if conn_tx.send(msg).is_err() {
                    break;
                }
            }
        })
}

fn spawn_check_thread(
    core: Arc<ServerCore>,
    check_rx: Receiver<()>,
    done: Arc<(Mutex<usize>, Condvar)>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("alint-lsp-check".to_string())
        .stack_size(WORKER_STACK_SIZE)
        .spawn(move || {
            let _guard = ThreadDoneGuard { done };
            while check_rx.recv().is_ok() {
                loop {
                    if core.is_stopping() {
                        return;
                    }
                    let target = core.check_gen.load(Ordering::SeqCst);
                    if core.completed_check_gen.load(Ordering::SeqCst) >= target {
                        break;
                    }
                    if let Err(payload) =
                        catch_unwind(AssertUnwindSafe(|| core.check_and_publish(target)))
                    {
                        tracing::error!(
                            panic = %panic_summary(&*payload),
                            "alint LSP check thread panicked"
                        );
                    }
                }
            }
        })
}

/// Drive the LSP protocol loop over `connection` until `exit` or EOF, returning
/// the LSP-mandated process exit code (`0` after `shutdown` or EOF, `1` on
/// `exit` without a prior `shutdown`).
pub(super) fn run_connection(connection: &Connection, options: LspOptions) -> std::io::Result<i32> {
    // Buffer outbound messages through an unbounded channel + dedicated relay
    // thread so `Connection::stdio()`'s `bounded(0)` writer channel can never
    // block workers holding `reeval_publish` or deadlock teardown.
    let (out_tx, out_rx) = unbounded::<Message>();
    let core = Arc::new(ServerCore::new(out_tx, options));

    let relay_done = Arc::new((Mutex::new(1usize), Condvar::new()));
    let relay_handle = spawn_outbound_relay(
        Arc::clone(&core),
        connection.sender.clone(),
        out_rx,
        Arc::clone(&relay_done),
    )?;

    // Dedicated thread for coalesced full workspace checks so slow checks
    // never starve `didChange` re-evaluations or `codeAction` requests.
    let (check_tx, check_rx) = bounded::<()>(1);
    let check_done = Arc::new((Mutex::new(1usize), Condvar::new()));
    let check_handle = spawn_check_thread(Arc::clone(&core), check_rx, Arc::clone(&check_done))?;

    let (job_tx, job_rx) = unbounded::<Job>();
    let workers_done = Arc::new((Mutex::new(WORKER_COUNT), Condvar::new()));
    let mut workers = Vec::with_capacity(WORKER_COUNT);
    for idx in 0..WORKER_COUNT {
        let core = Arc::clone(&core);
        let rx = job_rx.clone();
        let done = Arc::clone(&workers_done);
        let handle = std::thread::Builder::new()
            .name(format!("alint-lsp-worker-{idx}"))
            .stack_size(WORKER_STACK_SIZE)
            .spawn(move || {
                let _guard = ThreadDoneGuard { done };
                while let Ok(job) = rx.recv() {
                    run_worker_job(&core, job);
                }
            })?;
        workers.push(handle);
    }

    let mut initialized = false;
    let mut shutdown_received = false;
    let mut exit_received = false;

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                handle_request(
                    &core,
                    &job_tx,
                    req,
                    &mut initialized,
                    &mut shutdown_received,
                );
            }
            Message::Notification(notif) => {
                if notif.method == Exit::METHOD {
                    exit_received = true;
                    break;
                }
                if !initialized || shutdown_received {
                    continue;
                }
                handle_notification(&core, &check_tx, &job_tx, notif);
            }
            Message::Response(_) => {}
        }
    }

    core.shutting_down.store(true, Ordering::SeqCst);
    core.exiting.store(true, Ordering::SeqCst);
    drop(check_tx);
    drop(job_tx);
    core.close_outbound();

    if wait_for_threads(&workers_done, EXIT_JOIN_TIMEOUT) {
        for worker in workers {
            let _ = worker.join();
        }
    }
    if wait_for_threads(&check_done, EXIT_JOIN_TIMEOUT) {
        let _ = check_handle.join();
    }
    if wait_for_threads(&relay_done, EXIT_JOIN_TIMEOUT) {
        let _ = relay_handle.join();
    }
    Ok(exit_code(exit_received, shutdown_received))
}

fn handle_request(
    core: &ServerCore,
    job_tx: &Sender<Job>,
    req: Request,
    initialized: &mut bool,
    shutdown_received: &mut bool,
) {
    if *shutdown_received {
        core.send_err(
            req.id,
            ErrorCode::InvalidRequest,
            "server is shutting down".to_string(),
        );
        return;
    }
    if !*initialized {
        if req.method == Initialize::METHOD {
            match serde_json::from_value::<InitializeParams>(req.params) {
                Ok(params) => {
                    let result = core.initialize(&params);
                    *initialized = true;
                    let value = serde_json::to_value(result).unwrap_or(serde_json::Value::Null);
                    core.send_ok(req.id, value);
                }
                Err(err) => {
                    core.send_err(req.id, ErrorCode::InvalidParams, err.to_string());
                }
            }
        } else {
            core.send_err(
                req.id,
                ErrorCode::ServerNotInitialized,
                format!("server not initialized before {}", req.method),
            );
        }
        return;
    }
    if req.method == Shutdown::METHOD {
        *shutdown_received = true;
        // Stop background notifications, wait for in-flight `codeAction`
        // responses to queue, and wait for any active publish batch to finish
        // so the `shutdown` response is the final message sent before `exit`.
        core.shutting_down.store(true, Ordering::SeqCst);
        core.wait_for_code_actions();
        let publish_drain = core.reeval_publish.lock();
        drop(publish_drain);
        core.send_ok(req.id, serde_json::Value::Null);
        return;
    }

    match req.method.as_str() {
        Initialize::METHOD => {
            core.send_err(
                req.id,
                ErrorCode::InvalidRequest,
                "server already initialized".to_string(),
            );
        }
        HoverRequest::METHOD => match serde_json::from_value::<HoverParams>(req.params) {
            Ok(params) => {
                let hover = core.hover(params);
                let value = serde_json::to_value(hover).unwrap_or(serde_json::Value::Null);
                core.send_ok(req.id, value);
            }
            Err(err) => core.send_err(req.id, ErrorCode::InvalidParams, err.to_string()),
        },
        CodeActionRequest::METHOD => match serde_json::from_value::<CodeActionParams>(req.params) {
            Ok(params) => {
                core.register_code_action(&req.id);
                if job_tx
                    .send(Job::CodeAction {
                        id: req.id.clone(),
                        params,
                    })
                    .is_err()
                {
                    core.finish_code_action(&req.id);
                }
            }
            Err(err) => core.send_err(req.id, ErrorCode::InvalidParams, err.to_string()),
        },
        _ => {
            core.send_err(
                req.id,
                ErrorCode::MethodNotFound,
                format!("method not found: {}", req.method),
            );
        }
    }
}

fn handle_did_change(core: &ServerCore, job_tx: &Sender<Job>, params: serde_json::Value) {
    match serde_json::from_value::<DidChangeTextDocumentParams>(params) {
        Ok(mut params) => {
            // FULL document sync -> the last content change carries the whole
            // new text. Ignore incremental ranged changes and changes for
            // documents that are not currently open.
            let Some(change) = params.content_changes.pop() else {
                return;
            };
            if change.range.is_some() {
                tracing::warn!("ignoring incremental didChange on FULL-sync LSP server");
                return;
            }
            let uri = params.text_document.uri;
            let version = params.text_document.version;
            let seq = {
                let mut state = core.state.lock();
                if !state.open.contains(&uri) {
                    return;
                }
                let seq = core.next_doc_seq.fetch_add(1, Ordering::SeqCst);
                state.documents.insert(uri.clone(), change.text.clone());
                state.versions.insert(uri.clone(), version);
                state.doc_seq.insert(uri.clone(), seq);
                seq
            };
            let _ = job_tx.send(Job::Reeval {
                uri,
                text: change.text,
                version,
                seq,
            });
        }
        Err(err) => {
            tracing::warn!(error = %err, "ignoring malformed didChange notification");
        }
    }
}

fn handle_workspace_folders(core: &ServerCore, params: serde_json::Value) -> bool {
    match serde_json::from_value::<DidChangeWorkspaceFoldersParams>(params) {
        Ok(params) => {
            let mut state = core.state.lock();
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
            true
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "ignoring malformed didChangeWorkspaceFolders notification"
            );
            false
        }
    }
}

fn handle_notification(
    core: &ServerCore,
    check_tx: &Sender<()>,
    job_tx: &Sender<Job>,
    notif: Notification,
) {
    let trigger_full_check = || {
        core.check_gen.fetch_add(1, Ordering::SeqCst);
        let _ = check_tx.try_send(());
    };

    match notif.method.as_str() {
        Initialized::METHOD => {
            core.log_message(MessageType::INFO, "alint language server ready");
        }
        Cancel::METHOD => match serde_json::from_value::<CancelParams>(notif.params) {
            Ok(params) => {
                let id = cancel_id_to_request_id(params.id);
                core.cancel_request(&id);
            }
            Err(err) => {
                tracing::warn!(error = %err, "ignoring malformed $/cancelRequest notification");
            }
        },
        DidOpenTextDocument::METHOD => {
            match serde_json::from_value::<DidOpenTextDocumentParams>(notif.params) {
                Ok(params) => {
                    let uri = params.text_document.uri;
                    let seq = core.next_doc_seq.fetch_add(1, Ordering::SeqCst);
                    {
                        let mut state = core.state.lock();
                        state.open.insert(uri.clone());
                        state
                            .versions
                            .insert(uri.clone(), params.text_document.version);
                        state.doc_seq.insert(uri.clone(), seq);
                        state.documents.insert(uri, params.text_document.text);
                    }
                    trigger_full_check();
                }
                Err(err) => {
                    tracing::warn!(error = %err, "ignoring malformed didOpen notification");
                }
            }
        }
        DidChangeTextDocument::METHOD => handle_did_change(core, job_tx, notif.params),
        DidSaveTextDocument::METHOD | DidChangeWatchedFiles::METHOD => trigger_full_check(),
        DidChangeWorkspaceFolders::METHOD => {
            if handle_workspace_folders(core, notif.params) {
                trigger_full_check();
            }
        }
        DidCloseTextDocument::METHOD => {
            match serde_json::from_value::<DidCloseTextDocumentParams>(notif.params) {
                Ok(params) => {
                    if core.did_close(params) {
                        // A closed config file may have had unsaved buffer
                        // diagnostics; refresh from disk.
                        trigger_full_check();
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "ignoring malformed didClose notification");
                }
            }
        }
        other => {
            tracing::debug!(method = other, "ignoring unhandled LSP notification");
        }
    }
}
