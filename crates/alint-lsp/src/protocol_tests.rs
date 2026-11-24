//! In-memory protocol lifecycle tests driving [`run_connection`] over
//! [`lsp_server::Connection::memory`].

use std::time::Duration;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Initialized,
    Notification as _, PublishDiagnostics,
};
#[cfg(unix)]
use lsp_types::notification::{DidSaveTextDocument, LogMessage};
use lsp_types::request::{CodeActionRequest, HoverRequest, Initialize, Shutdown};
use lsp_types::{
    CodeActionContext, CodeActionParams, CodeActionResponse, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, Hover, HoverParams, InitializeParams,
    InitializeResult, InitializedParams, PartialResultParams, PublishDiagnosticsParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Url, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    WorkspaceFolder,
};
#[cfg(unix)]
use lsp_types::{DidSaveTextDocumentParams, LogMessageParams};

use super::{LspOptions, run_connection};

const RECV_TIMEOUT: Duration = Duration::from_secs(10);

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

fn initialize_folder(conn: &Connection, root: &std::path::Path) -> InitializeResult {
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

#[test]
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
    assert!(hover.is_some(), "expected hover over diagnostic");

    send_request::<CodeActionRequest>(
        &client_conn,
        3,
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
    let action_resp = recv_response(&client_conn, 3);
    let actions: Option<CodeActionResponse> =
        serde_json::from_value(action_resp.response_result.unwrap()).unwrap();
    assert_eq!(actions.unwrap().len(), 1);

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

    send_request::<Shutdown>(&client_conn, 4, &());
    assert!(recv_response(&client_conn, 4).response_result.is_ok());
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[test]
fn connection_exit_without_shutdown_returns_one() {
    let (server_conn, client_conn) = Connection::memory();
    let server = std::thread::spawn(move || run_connection(&server_conn, LspOptions::default()));

    send_request::<Initialize>(&client_conn, 1, &InitializeParams::default());
    let _ = recv_response(&client_conn, 1);
    send_notification::<Exit>(&client_conn, &());

    let code = server.join().unwrap().unwrap();
    assert_eq!(code, 1);
}

#[test]
fn connection_enforces_lifecycle_error_codes() {
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
    let before_init = recv_response(&client_conn, 1);
    assert_eq!(
        before_init.response_result.unwrap_err().code,
        ErrorCode::ServerNotInitialized as i32
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
    let code = server.join().unwrap().unwrap();
    assert_eq!(code, 0);
}

#[cfg(unix)]
#[test]
fn connection_logs_parent_ignore_walk_warnings_and_dedups_repeat_checks() {
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
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n",
    )
    .unwrap();
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

    send_request::<Shutdown>(&client_conn, 2, &());
    let _ = recv_response(&client_conn, 2);
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}
