use super::*;
use alint_core::{PerFileRule, Scope};

static TEST_PER_FILE_META: KindMeta = KindMeta {
    dispatch: DispatchClass::PerFile { full_index: false },
    paths: PathsSupport::Optional,
    scope_filter: ScopeFilterSupport::Rejected {
        reason: Cow::Borrowed("test per-file rule"),
    },
    git_tracked_only: GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: Cow::Borrowed(&[]),
    since: SinceSupport::None,
    capabilities: Cow::Borrowed(&[]),
    content: ContentAccess::WholeFile,
    formats: Cow::Borrowed(&[]),
    dependencies: Cow::Borrowed(&[]),
    fix_ops: Cow::Borrowed(&[]),
    expect_matches: true,
    trace: TraceMeta::ASSERTION,
    nested_rules: false,
};

#[derive(Debug)]
struct GatedPerFileRule {
    id: String,
    level: Level,
    scope: Scope,
    calls: Arc<AtomicUsize>,
    block_on_call: usize,
    gate: Arc<Gate>,
    panic_on_file: Arc<AtomicBool>,
}

impl Rule for GatedPerFileRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn level(&self) -> Level {
        self.level
    }

    fn evaluate(&self, _ctx: &Context<'_>) -> alint_core::Result<Vec<Violation>> {
        Ok(Vec::new())
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for GatedPerFileRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        rel_path: &Path,
        _content: &[u8],
    ) -> alint_core::Result<Vec<Violation>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.block_on_call {
            self.gate.enter_and_wait();
        }
        assert!(
            !self.panic_on_file.load(Ordering::SeqCst),
            "injected per-file panic"
        );
        let mut v = Violation::new("per-file gated finding").with_path(rel_path);
        v.line = Some(1);
        Ok(vec![v])
    }
}

#[derive(Clone)]
struct PerFileHook {
    calls: Arc<AtomicUsize>,
    block_on_call: usize,
    gate: Arc<Gate>,
    panic_on_file: Arc<AtomicBool>,
}

fn per_file_hooks() -> &'static Mutex<HashMap<String, PerFileHook>> {
    static HOOKS: OnceLock<Mutex<HashMap<String, PerFileHook>>> = OnceLock::new();
    HOOKS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct PerFileHookGuard {
    key: String,
    gate: Arc<Gate>,
}

impl PerFileHookGuard {
    fn register(prefix: &str, hook: PerFileHook) -> Self {
        let key = next_hook_id(prefix);
        let gate = Arc::clone(&hook.gate);
        per_file_hooks().lock().insert(key.clone(), hook);
        Self { key, gate }
    }
}

impl Drop for PerFileHookGuard {
    fn drop(&mut self) {
        self.gate.release();
        per_file_hooks().lock().remove(&self.key);
    }
}

#[allow(clippy::unnecessary_wraps)]
fn build_gated_per_file(spec: &RuleSpec) -> alint_core::Result<Box<dyn Rule>> {
    let hook = per_file_hooks()
        .lock()
        .get(&spec.id)
        .cloned()
        .expect("missing per-file hook");
    let scope = Scope::from_spec(spec)?;
    Ok(Box::new(GatedPerFileRule {
        id: spec.id.clone(),
        level: spec.level,
        scope,
        calls: hook.calls,
        block_on_call: hook.block_on_call,
        gate: hook.gate,
        panic_on_file: hook.panic_on_file,
    }))
}

#[test]
#[allow(clippy::too_many_lines)]
fn shutdown_stays_responsive_to_cancel_invalid_request_and_exit_while_code_action_blocked() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let doc_path = root.join("notes.txt");
    std::fs::write(&doc_path, "bad\n").unwrap();
    let doc_uri = Url::from_file_path(&doc_path).unwrap();

    // Part 1 (C2): `shutdown` with a blocked `codeAction` (string id) must NOT
    // block the main loop: post-shutdown requests immediately return
    // `InvalidRequest` and `$/cancelRequest` cancels the blocked `codeAction`.
    let gate = Arc::new(Gate::default());
    let guard = FixableHookGuard::register(
        "c2-cancel",
        FixableHook {
            gate: Arc::clone(&gate),
            should_panic: false,
            panic_on_eval: Arc::new(AtomicBool::new(false)),
        },
    );
    std::fs::write(
        root.join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: {}\n    kind: gated_fixable\n    level: error\n",
            guard.key
        ),
    )
    .unwrap();
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

    let ca_id = RequestId::from("ca-str-1".to_string());
    client_conn
        .sender
        .send(Message::Request(Request::new(
            ca_id.clone(),
            CodeActionRequest::METHOD.to_string(),
            serde_json::to_value(CodeActionParams {
                text_document: TextDocumentIdentifier {
                    uri: doc_uri.clone(),
                },
                range: diag_range,
                context: CodeActionContext::default(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .unwrap(),
        )))
        .unwrap();
    gate.wait_entered();

    send_request::<Shutdown>(&client_conn, 2, &());
    // While `codeAction` is STILL blocked, another request immediately gets InvalidRequest!
    send_request::<HoverRequest>(
        &client_conn,
        3,
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
    assert_eq!(
        recv_response(&client_conn, 3)
            .response_result
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest as i32
    );

    // Send `$/cancelRequest` AFTER `shutdown` and release the gate.
    send_notification::<Cancel>(
        &client_conn,
        &CancelParams {
            id: NumberOrString::String("ca-str-1".to_string()),
        },
    );
    // Use another InvalidRequest as a FIFO sync barrier on the main loop.
    send_request::<Shutdown>(&client_conn, 4, &());
    assert_eq!(
        recv_response(&client_conn, 4)
            .response_result
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest as i32
    );
    gate.release();

    let mut responses = Vec::new();
    while responses.len() < 2 {
        let msg = client_conn.receiver.recv_timeout(RECV_TIMEOUT).unwrap();
        if let Message::Response(resp) = msg {
            responses.push(resp);
        }
    }
    assert_eq!(responses[0].id, ca_id);
    assert_eq!(
        responses[0].response_result.as_ref().unwrap_err().code,
        ErrorCode::RequestCanceled as i32
    );
    assert_eq!(responses[1].id, RequestId::from(2));
    assert!(responses[1].response_result.is_ok());

    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);

    // Part 2 (C2): `exit` arrives while `codeAction` is still blocked after `shutdown`.
    let gate2 = Arc::new(Gate::default());
    let guard2 = FixableHookGuard::register(
        "c2-exit",
        FixableHook {
            gate: Arc::clone(&gate2),
            should_panic: false,
            panic_on_eval: Arc::new(AtomicBool::new(false)),
        },
    );
    std::fs::write(
        root.join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: {}\n    kind: gated_fixable\n    level: error\n",
            guard2.key
        ),
    )
    .unwrap();
    let mut registry2 = alint_rules::builtin_registry();
    registry2.register("gated_fixable", &TEST_FIXABLE_META, build_gated_fixable);
    let (server_conn, client_conn) = Connection::memory();
    let options2 = LspOptions::default().with_registry(Some(registry2));
    let server2 = std::thread::spawn(move || run_connection(&server_conn, options2));
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
            text_document: TextDocumentIdentifier { uri: doc_uri },
            range: diag_range,
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    gate2.wait_entered();
    send_request::<Shutdown>(&client_conn, 11, &());
    send_notification::<Exit>(&client_conn, &());
    // Release right after exit arrives so teardown joins fast without waiting 2s.
    gate2.release();
    assert_eq!(server2.join().unwrap().unwrap(), 0);
}

#[test]
#[allow(clippy::too_many_lines)]
fn stale_session_reeval_never_resurrects_after_folder_removal_and_per_file_panic_recovers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let doc_path = root.join("notes.txt");
    std::fs::write(&doc_path, "v1\n").unwrap();
    let doc_uri = Url::from_file_path(&doc_path).unwrap();
    let root_uri = Url::from_file_path(root).unwrap();

    let gate = Arc::new(Gate::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let panic_on_file = Arc::new(AtomicBool::new(false));
    let guard = PerFileHookGuard::register(
        "h1-pf",
        PerFileHook {
            calls,
            block_on_call: 2,
            gate: Arc::clone(&gate),
            panic_on_file: Arc::clone(&panic_on_file),
        },
    );
    std::fs::write(
        root.join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: {}\n    kind: gated_per_file\n    paths: \"*.txt\"\n    level: error\n",
            guard.key
        ),
    )
    .unwrap();
    let mut registry = alint_rules::builtin_registry();
    registry.register("gated_per_file", &TEST_PER_FILE_META, build_gated_per_file);

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
                text: "v1\n".to_string(),
            },
        },
    );
    assert_eq!(
        recv_diagnostics(&client_conn, &doc_uri).diagnostics.len(),
        1
    );

    // Edit to v2: worker clones OLD_SESSION and blocks inside `eval_buffer`.
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
                text: "v2\n".to_string(),
            }],
        },
    );
    gate.wait_entered();

    // Remove the workspace folder while OLD_SESSION's `reeval_file_seq` is in flight.
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
    assert_eq!(cleared.diagnostics, Vec::new());

    // Release the blocked OLD_SESSION worker: it MUST drop its stale finding (H1).
    gate.release();
    send_request::<Shutdown>(&client_conn, 2, &());
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
                        "stale session worker must never resurrect diagnostics: {:?}",
                        params.diagnostics
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
fn open_config_edited_during_check_never_publishes_stale_v1_as_v2_and_close_refreshes_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let cfg_path = root.join(".alint.yml");
    let cfg_uri = Url::from_file_path(&cfg_path).unwrap();

    // H3: open `.alint.yml` edited to clean v2 while a slow full check ran at dirty v1.
    let gate = Arc::new(Gate::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let guard = CheckHookGuard::register(
        "h3-cfg",
        CheckHook {
            calls,
            block_on_call: 2,
            gate: Arc::clone(&gate),
        },
    );
    let clean_cfg = format!(
        "version: 1\nrules:\n  - id: {}\n    kind: gated_check\n    level: error\n  \
         - id: no-ws\n    kind: file_content_forbidden\n    paths: \"*.yml\"\n    \
         pattern: \"BAD[T]OKEN\"\n    level: error\n",
        guard.key
    );
    let dirty_cfg = format!("{clean_cfg}# BADTOKEN\n");
    std::fs::write(&cfg_path, &dirty_cfg).unwrap();

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
                uri: cfg_uri.clone(),
                language_id: "yaml".to_string(),
                version: 1,
                text: dirty_cfg,
            },
        },
    );
    let init = recv_diagnostics(&client_conn, &cfg_uri);
    assert_eq!(init.diagnostics.len(), 1);

    // Trigger slow check at dirty v1, then edit `.alint.yml` to clean v2.
    send_notification::<DidSaveTextDocument>(
        &client_conn,
        &DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: cfg_uri.clone(),
            },
            text: None,
        },
    );
    gate.wait_entered();
    send_notification::<DidChangeTextDocument>(
        &client_conn,
        &DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: cfg_uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: clean_cfg.clone(),
            }],
        },
    );
    let v2_clean = recv_diagnostics(&client_conn, &cfg_uri);
    assert_eq!(v2_clean.version, Some(2));
    assert_eq!(v2_clean.diagnostics, Vec::new());

    gate.release();
    // Wait for `applied.reeval` to publish clean v2; no stale v1 finding may appear.
    let after_release = recv_diagnostics(&client_conn, &cfg_uri);
    assert_eq!(after_release.version, Some(2));
    assert!(
        after_release.diagnostics.is_empty(),
        "must not publish stale v1 per-file diagnostics stamped as v2: {:?}",
        after_release.diagnostics
    );

    send_request::<Shutdown>(&client_conn, 2, &());
    assert!(recv_response(&client_conn, 2).response_result.is_ok());
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);

    // M3: opening `.alint.yml` with unsaved dirty buffer and closing it BEFORE
    // the initial check completes must refresh cleanly from disk (clean_cfg).
    std::fs::write(&cfg_path, &clean_cfg).unwrap();
    let gate_m3 = Arc::new(Gate::default());
    let guard_m3 = CheckHookGuard::register(
        "m3-cfg",
        CheckHook {
            calls: Arc::new(AtomicUsize::new(0)),
            block_on_call: 1,
            gate: Arc::clone(&gate_m3),
        },
    );
    let clean_m3 = format!(
        "version: 1\nrules:\n  - id: {}\n    kind: gated_check\n    level: error\n  \
         - id: no-ws\n    kind: file_content_forbidden\n    paths: \"*.yml\"\n    \
         pattern: \"BAD[T]OKEN\"\n    level: error\n",
        guard_m3.key
    );
    std::fs::write(&cfg_path, &clean_m3).unwrap();
    let mut registry_m3 = alint_rules::builtin_registry();
    registry_m3.register("gated_check", &TEST_RULE_META, build_gated_check);
    let (server_conn, client_conn) = Connection::memory();
    let options_m3 = LspOptions::default().with_registry(Some(registry_m3));
    let server_m3 = std::thread::spawn(move || run_connection(&server_conn, options_m3));
    let _ = initialize_folder(&client_conn, root);
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: cfg_uri.clone(),
                language_id: "yaml".to_string(),
                version: 1,
                text: format!("{clean_m3}# BADTOKEN\n"),
            },
        },
    );
    gate_m3.wait_entered();
    send_notification::<DidCloseTextDocument>(
        &client_conn,
        &DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: cfg_uri.clone(),
            },
        },
    );
    // Barrier before releasing so `didClose` is processed while `state.config_uris` is empty.
    send_request::<HoverRequest>(
        &client_conn,
        5,
        &HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: cfg_uri.clone(),
                },
                position: Position::new(0, 0),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        },
    );
    assert!(recv_response(&client_conn, 5).response_result.is_ok());
    gate_m3.release();
    // Must publish clean `[]` for `.alint.yml` (never the discarded unsaved BADTOKEN).
    let diag = recv_diagnostics(&client_conn, &cfg_uri);
    assert!(
        diag.diagnostics.is_empty(),
        "closed config must not keep discarded unsaved per-file diagnostics: {:?}",
        diag.diagnostics
    );
    send_request::<Shutdown>(&client_conn, 6, &());
    assert!(recv_response(&client_conn, 6).response_result.is_ok());
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server_m3.join().unwrap().unwrap(), 0);
}

#[test]
#[allow(clippy::too_many_lines)]
fn multi_root_shared_config_dedups_pattern_hints_and_broken_logs_and_m1_clears_open_broken_cfg() {
    // M5a: pattern_hints deduplicated across two roots sharing `--config`.
    let tmp = tempfile::tempdir().unwrap();
    let shared_cfg = tmp.path().join("shared.alint.yml");
    std::fs::write(
        &shared_cfg,
        "version: 1\nrules:\n  - id: anchored\n    kind: file_content_forbidden\n    \
         paths: '*.txt'\n    pattern: '^test'\n    level: warning\n",
    )
    .unwrap();
    let root_a = tmp.path().join("a");
    let root_b = tmp.path().join("b");
    std::fs::create_dir(&root_a).unwrap();
    std::fs::create_dir(&root_b).unwrap();
    std::fs::write(root_a.join("a.txt"), "ok\n").unwrap();
    std::fs::write(root_b.join("b.txt"), "ok\n").unwrap();
    let cfg_uri = Url::from_file_path(&shared_cfg).unwrap();
    let a_uri = Url::from_file_path(root_a.join("a.txt")).unwrap();

    let (server_conn, client_conn) = Connection::memory();
    let options = LspOptions::default().with_config(Some(shared_cfg.clone()));
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
                uri: a_uri.clone(),
                language_id: "plaintext".to_string(),
                version: 1,
                text: "ok\n".to_string(),
            },
        },
    );
    let cfg_diags = recv_diagnostics(&client_conn, &cfg_uri);
    assert_eq!(
        cfg_diags.diagnostics.len(),
        1,
        "identical pattern_hints must deduplicate across roots sharing --config: {:?}",
        cfg_diags.diagnostics
    );

    // M5b: broken shared `--config` emits only ONE `window/logMessage` across both roots.
    std::fs::write(&shared_cfg, "version: 1\nrules: [unclosed\n").unwrap();
    send_notification::<DidSaveTextDocument>(
        &client_conn,
        &DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: a_uri },
            text: None,
        },
    );
    let mut err_logs = 0usize;
    loop {
        let msg = client_conn.receiver.recv_timeout(RECV_TIMEOUT).unwrap();
        if let Message::Notification(notif) = msg {
            if notif.method == LogMessage::METHOD {
                let p: LogMessageParams = serde_json::from_value(notif.params).unwrap();
                if p.typ == lsp_types::MessageType::WARNING {
                    err_logs += 1;
                }
            } else if notif.method == PublishDiagnostics::METHOD {
                let p: PublishDiagnosticsParams = serde_json::from_value(notif.params).unwrap();
                if p.uri == cfg_uri && !p.diagnostics.is_empty() {
                    break;
                }
            }
        }
    }
    assert_eq!(err_logs, 1, "broken shared --config should log once");

    // M1: opening a broken config mid-check while removing all folders clears its ConfigError.
    send_notification::<DidOpenTextDocument>(
        &client_conn,
        &DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: cfg_uri.clone(),
                language_id: "yaml".to_string(),
                version: 1,
                text: "version: 1\nrules: [unclosed\n".to_string(),
            },
        },
    );
    let _ = recv_diagnostics(&client_conn, &cfg_uri);
    send_notification::<DidChangeWorkspaceFolders>(
        &client_conn,
        &DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: Vec::new(),
                removed: vec![
                    WorkspaceFolder {
                        uri: Url::from_file_path(&root_a).unwrap(),
                        name: "a".to_string(),
                    },
                    WorkspaceFolder {
                        uri: Url::from_file_path(&root_b).unwrap(),
                        name: "b".to_string(),
                    },
                ],
            },
        },
    );
    let cleared = recv_diagnostics(&client_conn, &cfg_uri);
    assert!(
        cleared.diagnostics.is_empty(),
        "removing all folders must clear open broken config diagnostics: {:?}",
        cleared.diagnostics
    );

    send_request::<Shutdown>(&client_conn, 2, &());
    assert!(recv_response(&client_conn, 2).response_result.is_ok());
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}

#[test]
#[allow(clippy::too_many_lines)]
fn per_file_reeval_panic_recovers_and_rapid_did_change_burst_coalesces_without_starving_code_action()
 {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let doc_path = root.join("notes.txt");
    std::fs::write(&doc_path, "TODO\n").unwrap();
    let doc_uri = Url::from_file_path(&doc_path).unwrap();

    // Part 1: panicking per-file rule on `didChange` logs error and keeps server alive.
    let gate = Arc::new(Gate::default());
    gate.release();
    let panic_on_file = Arc::new(AtomicBool::new(false));
    let guard = PerFileHookGuard::register(
        "pf-panic",
        PerFileHook {
            calls: Arc::new(AtomicUsize::new(0)),
            block_on_call: usize::MAX,
            gate,
            panic_on_file: Arc::clone(&panic_on_file),
        },
    );
    std::fs::write(
        root.join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: {}\n    kind: gated_per_file\n    paths: \"*.txt\"\n    level: error\n  \
             - id: no-todo\n    kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: \"TODO\"\n    \
             level: error\n    fix: {{ replace: {{ replacement: \"DONE\" }} }}\n",
            guard.key
        ),
    )
    .unwrap();
    let mut registry = alint_rules::builtin_registry();
    registry.register("gated_per_file", &TEST_PER_FILE_META, build_gated_per_file);

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
    let init = recv_diagnostics(&client_conn, &doc_uri);
    assert_eq!(init.diagnostics.len(), 2);
    let diag_range = init.diagnostics[0].range;

    panic_on_file.store(true, Ordering::SeqCst);
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
                text: "TODO panic\n".to_string(),
            }],
        },
    );
    loop {
        let msg = client_conn.receiver.recv_timeout(RECV_TIMEOUT).unwrap();
        if let Message::Notification(notif) = msg
            && notif.method == LogMessage::METHOD
        {
            let p: LogMessageParams = serde_json::from_value(notif.params).unwrap();
            if p.message.contains("re-eval panicked") {
                break;
            }
        }
    }

    // Part 2 (M8): burst of 20 `didChange` notifications followed by `codeAction`
    // coalesces cleanly and replies to `codeAction` promptly.
    panic_on_file.store(false, Ordering::SeqCst);
    for v in 3..=22 {
        send_notification::<DidChangeTextDocument>(
            &client_conn,
            &DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: doc_uri.clone(),
                    version: v,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: format!("TODO {v}\n"),
                }],
            },
        );
    }
    send_request::<CodeActionRequest>(
        &client_conn,
        99,
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
    let mut got_code_action = false;
    let mut got_v22 = false;
    while !(got_code_action && got_v22) {
        let msg = client_conn.receiver.recv_timeout(RECV_TIMEOUT).unwrap();
        match msg {
            Message::Response(resp) if resp.id == RequestId::from(99) => {
                assert!(resp.response_result.is_ok());
                got_code_action = true;
            }
            Message::Notification(notif) if notif.method == PublishDiagnostics::METHOD => {
                let diag: PublishDiagnosticsParams = serde_json::from_value(notif.params).unwrap();
                if diag.uri == doc_uri && diag.version == Some(22) {
                    assert_eq!(diag.diagnostics.len(), 2);
                    got_v22 = true;
                }
            }
            _ => {}
        }
    }

    send_request::<Shutdown>(&client_conn, 100, &());
    assert!(recv_response(&client_conn, 100).response_result.is_ok());
    send_notification::<Exit>(&client_conn, &());
    assert_eq!(server.join().unwrap().unwrap(), 0);
}
