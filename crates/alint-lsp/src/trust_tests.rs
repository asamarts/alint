//! Workspace trust, config discovery, hover escaping, re-evaluation
//! coalescing, and config-error placement.

use super::config::{
    Settings, build_session, build_session_with, check_workspace, config_for_document, session_key,
};
use super::*;
use std::sync::atomic::Ordering;

fn untrusted() -> Settings {
    Settings::default()
}

fn trusted() -> Settings {
    Settings {
        trusted: true,
        ..Settings::default()
    }
}

/// The ids of the rules a session built, sorted.
#[cfg(unix)]
fn built_rules(session: &Session) -> Vec<String> {
    let mut ids: Vec<String> = session.kinds.keys().cloned().collect();
    ids.sort();
    ids
}

/// Every rule that spawns a process (by kind, fix operation, or nested
/// `require:` kind), a `custom:` fact that writes `sentinel`, and one plain
/// rule.
fn spawning_config(sentinel: &Path) -> String {
    // Single-quoted, so a Windows path's backslashes are not YAML escapes.
    let touch = format!("[\"sh\", \"-c\", 'touch {}']", sentinel.display());
    format!(
        "version: 1\nfacts:\n  - id: probe\n    custom:\n      argv: {touch}\nrules:\n  \
         - id: run\n    kind: command\n    paths: \"*.txt\"\n    command: {touch}\n    \
         level: error\n  \
         - id: untrack\n    kind: file_absent\n    paths: \"*.log\"\n    \
         git_tracked_only: true\n    fix:\n      git_untrack: {{}}\n    level: error\n  \
         - id: per-dir\n    kind: for_each_dir\n    select: \"*\"\n    require:\n      \
         - kind: command\n        paths: \"*.txt\"\n        command: {touch}\n    level: error\n  \
         - id: readme\n    kind: file_exists\n    paths: README.md\n    level: error\n"
    )
}

#[test]
#[cfg(unix)]
fn an_untrusted_session_builds_no_spawning_rule_or_fact() {
    let dir = tempfile::tempdir().unwrap();
    let sentinel = dir.path().join("SENTINEL");
    std::fs::write(dir.path().join(".alint.yml"), spawning_config(&sentinel)).unwrap();
    std::fs::write(dir.path().join("a.txt"), "hi\n").unwrap();

    let session = build_session_with(dir.path(), &untrusted())
        .unwrap()
        .unwrap();
    assert_eq!(built_rules(&session), ["readme"]);
    assert!(session.withheld);
    let report = session.engine.run(&session.root, &session.index).unwrap();
    assert_eq!(report.results.len(), 1);
    assert!(!sentinel.exists(), "an untrusted session ran a process");

    // Trusted, the same config builds every rule and runs the fact.
    let session = build_session(dir.path()).unwrap().unwrap();
    assert_eq!(
        built_rules(&session),
        ["per-dir", "readme", "run", "untrack"]
    );
    assert!(!session.withheld);
    session.engine.run(&session.root, &session.index).unwrap();
    assert!(sentinel.exists(), "positive control: the trusted fact runs");
}

#[test]
fn no_spawn_refuses_a_spawning_config_even_when_trusted() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        spawning_config(&dir.path().join("SENTINEL")),
    )
    .unwrap();
    let settings = Settings {
        no_spawn: true,
        ..trusted()
    };
    let error = build_session_with(dir.path(), &settings).unwrap_err();
    assert_eq!(
        error,
        "loading config: rule \"run\" (kind: command) spawns a process; --no-spawn refuses it"
    );
}

#[test]
fn an_untrusted_session_reads_no_environment() {
    // Cargo sets `CARGO_PKG_NAME` (`alint-lsp`) for the test process.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("alint-lsp"), "").unwrap();
    std::fs::write(dir.path().join("a.txt"), "TODO\n").unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: named\n    kind: file_exists\n    \
         paths: \"{{env.CARGO_PKG_NAME | default('fallback')}}\"\n    level: error\n  \
         - id: gated\n    kind: file_content_forbidden\n    paths: \"*.txt\"\n    \
         pattern: TODO\n    when: 'env.CARGO_PKG_NAME == \"alint-lsp\"'\n    level: error\n",
    )
    .unwrap();
    let fired = |settings: &Settings| {
        let session = build_session_with(dir.path(), settings).unwrap().unwrap();
        let report = session.engine.run(&session.root, &session.index).unwrap();
        let mut ids: Vec<String> = report
            .results
            .iter()
            .filter(|result| !result.violations.is_empty())
            .map(|result| result.rule_id.to_string())
            .collect();
        ids.sort();
        (ids, session.withheld)
    };
    // Untrusted: the default fills in, and `env.*` in `when:` reads nothing.
    assert_eq!(fired(&untrusted()), (vec!["named".to_string()], true));
    assert_eq!(fired(&trusted()), (vec!["gated".to_string()], false));
}

#[test]
fn an_untrusted_session_fetches_no_remote_extends_and_says_why() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        format!(
            "version: 1\nextends:\n  - \"https://127.0.0.1:{port}/base.yml#sha256-{}\"\n\
             rules: []\n",
            "0".repeat(64)
        ),
    )
    .unwrap();
    let check = check_workspace(
        &[dir.path().to_path_buf()],
        &[],
        &untrusted(),
        &alint_rules::builtin_registry(),
    );
    let [(_, Err(error))] = check.runs.as_slice() else {
        panic!("the config does not load: {:?}", check.runs);
    };
    assert!(error.untrusted, "{error:?}");
    assert!(
        error.message.contains("remote `extends` is disabled"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains(config::UNTRUSTED_NOTE),
        "{}",
        error.message
    );
    assert!(
        listener
            .accept()
            .is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock),
        "an untrusted session connected to the remote"
    );
}

#[test]
fn the_workspace_folders_config_governs_a_vendored_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    let vendored = root.join("node_modules/evil-pkg");
    std::fs::create_dir_all(&vendored).unwrap();
    std::fs::write(vendored.join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    let doc = vendored.join("index.js");
    let folders = [root.to_path_buf()];
    assert_eq!(
        config_for_document(&doc, &folders),
        Some(root.join(".alint.yml"))
    );

    // A folder with no config at or above it lints nothing, as `alint check`
    // run there would find none: a config below it governs nothing alone.
    let bare = tempfile::tempdir().unwrap();
    let pkg = bare.path().join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    assert_eq!(
        config_for_document(&pkg.join("a.txt"), &[bare.path().to_path_buf()]),
        None
    );

    // `--config` governs every folder, rooted at the folder.
    let fixed = bare.path().join("mine.yml");
    let key = session_key(&doc, &folders, Some(&fixed)).unwrap();
    assert_eq!((key.config, key.root), (fixed.clone(), root.to_path_buf()));
}

#[test]
fn hover_shows_repository_markdown_literally() {
    let mut finding = Finding {
        range: Range::new(Position::new(0, 0), Position::new(0, 1)),
        severity: DiagnosticSeverity::ERROR,
        rule_id: "id`with`ticks".to_string(),
        message: "found ![x](http://127.0.0.1:9/a.png) and <img src=x> [y](http://e.test)"
            .to_string(),
        line: None,
        column: None,
        policy_url: Some("javascript:alert(1)".to_string()),
        fixable: false,
        sensitive: false,
        per_file: true,
        description: None,
        docs_url: None,
        related_information: Vec::new(),
    };
    let value = render::render_finding(&finding);
    assert!(!value.contains("](http"), "{value}");
    assert!(value.contains(r"and \<img src\=x\>"), "{value}");
    assert!(!value.contains("[Policy"), "{value}");
    // Every escape is a backslash before ASCII punctuation, so the text
    // reads back as written.
    let message_line = value.lines().nth(2).unwrap();
    let mut unescaped = String::new();
    let mut chars = message_line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some_and(char::is_ascii_punctuation) {
            continue;
        }
        unescaped.push(c);
    }
    assert_eq!(unescaped, finding.message);
    assert!(
        value.starts_with("**alint** · ``id`with`ticks`` "),
        "{value}"
    );

    finding.policy_url = Some("https://example.com/policy (draft)".to_string());
    let value = render::render_finding(&finding);
    assert!(
        value.contains("[Policy →](<https://example.com/policy%20(draft)>)"),
        "{value}"
    );
}

#[tokio::test]
async fn a_superseded_buffer_is_not_evaluated() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: \"*.txt\"\n    pattern: TODO\n    level: error\n",
    )
    .unwrap();
    std::fs::write(root.join("a.txt"), "clean\n").unwrap();
    let session = build_session(&root).unwrap().unwrap();
    let (service, _socket) = LspService::new(|client| Backend::new(client, LspOptions::default()));
    let backend = service.inner();
    let uri = Url::from_file_path(root.join("a.txt")).unwrap();
    {
        let mut st = backend.state.lock();
        st.folders = vec![root.clone()];
        let key = SessionKey {
            config: session.config_path.clone(),
            root: session.root.clone(),
        };
        st.doc_config.insert(uri.clone(), key.clone());
        st.sessions.insert(key, Arc::new(session));
        st.open.insert(uri.clone());
        st.versions.insert(uri.clone(), 3);
    }
    // Versions 1 and 2 were superseded by 3 before they started.
    for version in [1, 2] {
        backend
            .reeval_file(uri.clone(), "TODO\n".into(), version)
            .await;
    }
    assert_eq!(backend.evaluations.load(Ordering::SeqCst), 0);
    assert!(!backend.state.lock().diagnostics.contains_key(&uri));
    backend.reeval_file(uri.clone(), "TODO\n".into(), 3).await;
    assert_eq!(backend.evaluations.load(Ordering::SeqCst), 1);
    assert_eq!(backend.state.lock().diagnostics[&uri].len(), 1);
}

#[test]
fn a_config_syntax_error_is_placed_where_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".alint.yml");
    std::fs::write(&config, "version: 1\nrules:\n  - id: [unclosed\n").unwrap();
    let check = check_workspace(
        &[dir.path().to_path_buf()],
        &[],
        &untrusted(),
        &alint_rules::builtin_registry(),
    );
    let mut state = State::default();
    let applied = apply_check(&mut state, &[], check, &LspOptions::default());
    let uri = Url::from_file_path(&config).unwrap();
    let (_, diagnostics, _) = applied
        .publish
        .iter()
        .find(|(published, _, _)| *published == uri)
        .expect("the config error is published");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_ne!(
        diagnostics[0].range.start.line, 0,
        "placed where the YAML error is: {diagnostics:?}"
    );
    assert!(
        !applied.untrusted_note,
        "a syntax error is not a trust limit"
    );

    // A typed error has no place of its own in the file: line 0.
    std::fs::write(&config, "version: 1\nrulez: []\n").unwrap();
    let check = check_workspace(
        &[dir.path().to_path_buf()],
        &[],
        &untrusted(),
        &alint_rules::builtin_registry(),
    );
    let applied = apply_check(&mut state, &[], check, &LspOptions::default());
    let (_, diagnostics, _) = applied
        .publish
        .iter()
        .find(|(published, _, _)| *published == uri)
        .unwrap();
    assert_eq!(diagnostics[0].range.start, Position::new(0, 0));
}

#[test]
fn the_untrusted_note_is_shown_once_per_server() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: run\n    kind: command\n    paths: \"*.txt\"\n    \
         command: [\"true\"]\n    level: error\n",
    )
    .unwrap();
    let folders = [dir.path().to_path_buf()];
    let registry = alint_rules::builtin_registry();
    let mut state = State::default();
    let options = LspOptions::default();
    let shown: Vec<bool> = (0..2)
        .map(|_| {
            let check = check_workspace(&folders, &[], &untrusted(), &registry);
            apply_check(&mut state, &[], check, &options).untrusted_note
        })
        .collect();
    assert_eq!(shown, [true, false]);

    // A trusted workspace, or a config that uses nothing trust turns on,
    // gets no note.
    let mut state = State::default();
    let check = check_workspace(&folders, &[], &trusted(), &registry);
    assert!(!apply_check(&mut state, &[], check, &options).untrusted_note);
    std::fs::write(dir.path().join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
    let check = check_workspace(&folders, &[], &untrusted(), &registry);
    assert!(!apply_check(&mut state, &[], check, &options).untrusted_note);
}

#[tokio::test]
async fn initialize_reads_only_a_boolean_trusted_option() {
    for (options, trusted) in [
        (None, false),
        (Some(serde_json::json!({})), false),
        (Some(serde_json::json!({ "trusted": "true" })), false),
        (Some(serde_json::json!({ "trusted": false })), false),
        (Some(serde_json::json!({ "trusted": true })), true),
    ] {
        let (service, _socket) =
            LspService::new(|client| Backend::new(client, LspOptions::default()));
        let backend = service.inner();
        let params = InitializeParams {
            initialization_options: options,
            ..InitializeParams::default()
        };
        backend.initialize(params).await.unwrap();
        assert_eq!(backend.state.lock().trusted, trusted);
    }
}

#[test]
fn a_fixed_config_is_trusted_whatever_the_client_says() {
    let options = LspOptions {
        config: Some(PathBuf::from("/policies/alint.yml")),
        ..LspOptions::default()
    };
    assert!(options.settings(false).trusted);
    assert!(!LspOptions::default().settings(false).trusted);
    assert!(LspOptions::default().settings(true).trusted);
}
