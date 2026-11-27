//! LSP message loop, outbound relay, dedicated full-check thread, and
//! interactive worker pool driving [`ServerCore`].

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, bounded, select, select_biased, unbounded};
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

/// Result of [`run_connection_outcome`]: LSP process exit code plus whether the
/// outbound relay finished draining within [`EXIT_JOIN_TIMEOUT`].
#[derive(Debug, Clone, Copy)]
pub(super) struct ConnectionOutcome {
    pub(super) code: i32,
    pub(super) relay_drained: bool,
}

/// Interactive `codeAction` request queued onto the prioritized worker channel.
struct CodeActionJob {
    id: RequestId,
    token: u64,
    params: CodeActionParams,
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

/// Drop guard ensuring every accepted `codeAction` request clears its active
/// slot (and notifies any pending `shutdown` once the last one finishes) even
/// if a worker panics or returns early.
struct CodeActionGuard<'a> {
    core: &'a ServerCore,
    token: u64,
}

impl Drop for CodeActionGuard<'_> {
    fn drop(&mut self) {
        self.core.finish_code_action(self.token);
    }
}

/// Drop guard ensuring `shutting_down`, `exiting`, and `close_outbound` run on
/// every exit path of `run_connection_outcome` (including early thread-spawn
/// errors), so the outbound relay can never leak.
struct TeardownGuard<'a> {
    core: &'a ServerCore,
}

impl Drop for TeardownGuard<'_> {
    fn drop(&mut self) {
        self.core.shutting_down.store(true, Ordering::SeqCst);
        self.core.exiting.store(true, Ordering::SeqCst);
        self.core.close_outbound();
    }
}

/// Wait until `deadline` for `remaining` threads to reach zero, returning `true`
/// if all finished in time.
fn wait_for_threads_until(done: &(Mutex<usize>, Condvar), deadline: Instant) -> bool {
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

fn run_code_action_job(core: &ServerCore, job: CodeActionJob) {
    let CodeActionJob { id, token, params } = job;
    // Declared FIRST so its Drop runs AFTER `send_ok`/`send_err` queues this
    // response onto FIFO `outbound` -- guaranteeing `shutdown`'s `Ok(null)`
    // never overtakes an in-flight `codeAction` response.
    let _ca_guard = CodeActionGuard { core, token };
    if core.exiting.load(Ordering::SeqCst) {
        return;
    }
    if core.is_canceled(token) {
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
    if core.is_canceled(token) {
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

fn run_reeval_job(core: &ServerCore, uri: Url) {
    if core.is_stopping() {
        core.pending_reevals.lock().remove(&uri);
        return;
    }
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
        core.reeval_latest(uri);
    })) {
        tracing::error!(
            panic = %panic_summary(&*payload),
            "alint LSP reeval worker panicked"
        );
    }
}

fn spawn_outbound_relay(
    exiting: Arc<AtomicBool>,
    conn_tx: Sender<Message>,
    out_rx: Receiver<Message>,
    done: Arc<(Mutex<usize>, Condvar)>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("alint-lsp-outbound".to_string())
        .spawn(move || {
            let _guard = ThreadDoneGuard { done };
            while let Ok(msg) = out_rx.recv() {
                if exiting.load(Ordering::SeqCst) && !matches!(msg, Message::Response(_)) {
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
                        // Advance `completed_check_gen` and break so a
                        // deterministic panic cannot spin at 100% CPU.
                        core.completed_check_gen.fetch_max(target, Ordering::SeqCst);
                        break;
                    }
                }
            }
        })
}

type ThreadDone = Arc<(Mutex<usize>, Condvar)>;
type WorkerHandles = (Vec<std::thread::JoinHandle<()>>, ThreadDone);

fn spawn_workers(
    core: &Arc<ServerCore>,
    code_action_rx: &Receiver<CodeActionJob>,
    reeval_rx: &Receiver<Url>,
) -> std::io::Result<WorkerHandles> {
    let workers_done = Arc::new((Mutex::new(WORKER_COUNT), Condvar::new()));
    let mut workers = Vec::with_capacity(WORKER_COUNT);
    for idx in 0..WORKER_COUNT {
        let core = Arc::clone(core);
        let ca_rx = code_action_rx.clone();
        let re_rx = reeval_rx.clone();
        let done = Arc::clone(&workers_done);
        let handle = std::thread::Builder::new()
            .name(format!("alint-lsp-worker-{idx}"))
            .stack_size(WORKER_STACK_SIZE)
            .spawn(move || {
                let _guard = ThreadDoneGuard { done };
                let mut ca_open = true;
                let mut re_open = true;
                while ca_open || re_open {
                    if ca_open && re_open {
                        // Strictly prioritize interactive `codeAction` over
                        // coalesced background `didChange` re-evaluations so
                        // rapid typing bursts can never starve quick-fixes or
                        // `shutdown`.
                        select_biased! {
                            recv(ca_rx) -> msg => match msg {
                                Ok(job) => run_code_action_job(&core, job),
                                Err(_) => ca_open = false,
                            },
                            recv(re_rx) -> msg => match msg {
                                Ok(uri) => run_reeval_job(&core, uri),
                                Err(_) => re_open = false,
                            },
                        }
                    } else if ca_open {
                        match ca_rx.recv() {
                            Ok(job) => run_code_action_job(&core, job),
                            Err(_) => ca_open = false,
                        }
                    } else {
                        match re_rx.recv() {
                            Ok(uri) => run_reeval_job(&core, uri),
                            Err(_) => re_open = false,
                        }
                    }
                }
            })?;
        workers.push(handle);
    }
    Ok((workers, workers_done))
}

fn complete_shutdown(core: &ServerCore, shutdown_id: RequestId) {
    let publish_drain = core.reeval_publish.lock();
    drop(publish_drain);
    core.send_ok(shutdown_id, serde_json::Value::Null);
}

#[cfg(test)]
pub(super) fn run_connection(connection: &Connection, options: LspOptions) -> std::io::Result<i32> {
    run_connection_outcome(connection, options).map(|outcome| outcome.code)
}

/// Drive the LSP protocol loop over `connection` until `exit` or EOF, returning
/// the LSP-mandated process exit code (`0` after `shutdown` or EOF, `1` on
/// `exit` without a prior `shutdown`) and whether the outbound relay drained
/// within [`EXIT_JOIN_TIMEOUT`].
pub(super) fn run_connection_outcome(
    connection: &Connection,
    options: LspOptions,
) -> std::io::Result<ConnectionOutcome> {
    // Buffer outbound messages through an unbounded channel + dedicated relay
    // thread so `Connection::stdio()`'s `bounded(0)` writer channel can never
    // block workers holding `reeval_publish` or deadlock teardown.
    let (out_tx, out_rx) = unbounded::<Message>();
    let core = Arc::new(ServerCore::new(out_tx, options));
    let _teardown = TeardownGuard { core: &core };

    let relay_done = Arc::new((Mutex::new(1usize), Condvar::new()));
    let relay_handle = spawn_outbound_relay(
        Arc::clone(&core.exiting),
        connection.sender.clone(),
        out_rx,
        Arc::clone(&relay_done),
    )?;

    // Dedicated thread for coalesced full workspace checks so slow checks
    // never starve `didChange` re-evaluations or `codeAction` requests.
    let (check_tx, check_rx) = bounded::<()>(1);
    let check_done = Arc::new((Mutex::new(1usize), Condvar::new()));
    let check_handle = spawn_check_thread(Arc::clone(&core), check_rx, Arc::clone(&check_done))?;

    let (code_action_tx, code_action_rx) = unbounded::<CodeActionJob>();
    let (reeval_tx, reeval_rx) = unbounded::<Url>();
    let (workers, workers_done) = spawn_workers(&core, &code_action_rx, &reeval_rx)?;

    let mut initialized = false;
    let mut shutdown_received = false;
    let mut exit_received = false;
    let mut pending_shutdown: Option<(RequestId, Receiver<()>)> = None;

    loop {
        let msg = if let Some((shutdown_id, done_rx)) = pending_shutdown.take() {
            // Stay responsive to `$/cancelRequest`, `exit`, EOF, and
            // post-`shutdown` `InvalidRequest` while in-flight `codeAction`s
            // drain, preventing `shutdown` from deadlocking the dispatch loop.
            select! {
                recv(done_rx) -> _ => {
                    complete_shutdown(&core, shutdown_id);
                    continue;
                }
                recv(connection.receiver) -> incoming => {
                    pending_shutdown = Some((shutdown_id, done_rx));
                    match incoming {
                        Ok(msg) => msg,
                        Err(_) => break,
                    }
                }
            }
        } else {
            match connection.receiver.recv() {
                Ok(msg) => msg,
                Err(_) => break,
            }
        };

        match msg {
            Message::Request(req) => {
                handle_request(
                    &core,
                    &code_action_tx,
                    req,
                    &mut initialized,
                    &mut shutdown_received,
                    &mut pending_shutdown,
                );
            }
            Message::Notification(notif) => {
                if notif.method == Exit::METHOD {
                    exit_received = true;
                    break;
                }
                if !initialized {
                    continue;
                }
                if shutdown_received {
                    // Still accept `$/cancelRequest` while `shutdown` is
                    // waiting for in-flight `codeAction`s to drain.
                    if pending_shutdown.is_some() && notif.method == Cancel::METHOD {
                        handle_cancel(&core, notif.params);
                    }
                    continue;
                }
                handle_notification(&core, &check_tx, &reeval_tx, notif);
            }
            Message::Response(_) => {}
        }
    }

    core.shutting_down.store(true, Ordering::SeqCst);
    core.exiting.store(true, Ordering::SeqCst);
    drop(check_tx);
    drop(code_action_tx);
    drop(reeval_tx);
    core.close_outbound();

    // Share one bounded `EXIT_JOIN_TIMEOUT` deadline across all teardown joins.
    let deadline = Instant::now() + EXIT_JOIN_TIMEOUT;
    if wait_for_threads_until(&workers_done, deadline) {
        for worker in workers {
            let _ = worker.join();
        }
    }
    if wait_for_threads_until(&check_done, deadline) {
        let _ = check_handle.join();
    }
    let relay_drained = wait_for_threads_until(&relay_done, deadline);
    if relay_drained {
        let _ = relay_handle.join();
    }
    Ok(ConnectionOutcome {
        code: exit_code(exit_received, shutdown_received),
        relay_drained,
    })
}

fn handle_cancel(core: &ServerCore, params: serde_json::Value) {
    match serde_json::from_value::<CancelParams>(params) {
        Ok(params) => {
            let id = cancel_id_to_request_id(params.id);
            core.cancel_request(&id);
        }
        Err(err) => {
            tracing::warn!(error = %err, "ignoring malformed $/cancelRequest notification");
        }
    }
}

fn handle_request(
    core: &ServerCore,
    code_action_tx: &Sender<CodeActionJob>,
    req: Request,
    initialized: &mut bool,
    shutdown_received: &mut bool,
    pending_shutdown: &mut Option<(RequestId, Receiver<()>)>,
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
        // Stop background notifications, wait asynchronously for any in-flight
        // `codeAction` responses to queue, and drain active publish batches so
        // the `shutdown` response is the final message sent before `exit`.
        core.shutting_down.store(true, Ordering::SeqCst);
        if let Some(done_rx) = core.arm_shutdown_wait() {
            *pending_shutdown = Some((req.id, done_rx));
        } else {
            complete_shutdown(core, req.id);
        }
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
                let token = core.register_code_action(&req.id);
                if code_action_tx
                    .send(CodeActionJob {
                        id: req.id.clone(),
                        token,
                        params,
                    })
                    .is_err()
                {
                    core.send_err(
                        req.id,
                        ErrorCode::InternalError,
                        "alint: worker pool unavailable".to_string(),
                    );
                    core.finish_code_action(token);
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

fn handle_did_change(core: &ServerCore, reeval_tx: &Sender<Url>, params: serde_json::Value) {
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
            {
                let mut state = core.state.lock();
                if !state.open.contains(&uri) {
                    return;
                }
                let seq = core.next_doc_seq.fetch_add(1, Ordering::SeqCst);
                state.documents.insert(uri.clone(), change.text);
                state.versions.insert(uri.clone(), version);
                state.doc_seq.insert(uri.clone(), seq);
            }
            core.enqueue_reeval(reeval_tx, &uri);
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
                .filter_map(|f| {
                    f.uri
                        .to_file_path()
                        .ok()
                        .map(|p| crate::diagnostics::normalize_path_key(&p))
                })
                .collect();
            state.folders.retain(|f| !removed.contains(f));
            for added in &params.event.added {
                if let Ok(raw_path) = added.uri.to_file_path() {
                    let path = crate::diagnostics::normalize_path_key(&raw_path);
                    if !state.folders.contains(&path) {
                        state.folders.push(path);
                    }
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
    reeval_tx: &Sender<Url>,
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
        Cancel::METHOD => handle_cancel(core, notif.params),
        DidOpenTextDocument::METHOD => {
            match serde_json::from_value::<DidOpenTextDocumentParams>(notif.params) {
                Ok(params) => {
                    let uri = params.text_document.uri;
                    let seq = core.next_doc_seq.fetch_add(1, Ordering::SeqCst);
                    {
                        let mut state = core.state.lock();
                        if let Ok(abs) = uri.to_file_path() {
                            state
                                .open_uri_by_path
                                .insert(crate::diagnostics::normalize_path_key(&abs), uri.clone());
                        }
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
        DidChangeTextDocument::METHOD => handle_did_change(core, reeval_tx, notif.params),
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
