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
         fix:\n      git_untrack: {{}}\n    level: error\n  \
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
    // `untrack` spawns only through its fix, so it lints without one.
    assert_eq!(built_rules(&session), ["readme", "untrack"]);
    assert!(session.withheld);
    let report = session.engine.run(&session.root, &session.index).unwrap();
    assert_eq!(report.results.len(), 2);
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

/// A rule whose fix alone spawns (`git_untrack`) keeps its check in an
/// untrusted session and loses only the fix: it still reports the file,
/// without a fix to offer.
#[test]
#[cfg(unix)]
fn an_untrusted_session_keeps_a_rule_whose_fix_alone_spawns() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: untrack\n    kind: file_absent\n    paths: \"*.log\"\n    \
         fix:\n      git_untrack: {}\n    level: error\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("build.log"), "x\n").unwrap();
    for args in [&["init", "-q"][..], &["add", "--", "build.log"]] {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    let fixable = |settings: &Settings| {
        let session = build_session_with(dir.path(), settings).unwrap().unwrap();
        let report = session.engine.run(&session.root, &session.index).unwrap();
        report
            .results
            .iter()
            .filter(|result| result.rule_id.as_ref() == "untrack" && !result.violations.is_empty())
            .map(|result| result.is_fixable)
            .collect::<Vec<_>>()
    };
    assert_eq!(fixable(&untrusted()), [false], "reports, without its fix");
    assert_eq!(
        fixable(&trusted()),
        [true],
        "positive control: trusted, fixable"
    );
}

/// Run `git` with the host's configuration left out, in `dir`.
#[cfg(unix)]
fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

/// A repository with `a.txt` committed whose `.git/config` names a command
/// (`core.fsmonitor`, which Git runs when it reads the index) that writes
/// the returned canary, and a config whose every rule but `readme` runs Git:
/// `git_tracked_only`, a `git_*` kind, `scope_filter.changed_since`, and one
/// nested in `require:`.
#[cfg(unix)]
fn git_canary_repository(dir: &Path) -> PathBuf {
    let canary = dir.join("CANARY");
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/b.txt"), "b\n").unwrap();
    git(dir, &["init", "-q"]);
    git(dir, &["add", "a.txt", "sub/b.txt"]);
    git(
        dir,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-qm",
            "init",
        ],
    );
    git(
        dir,
        &[
            "config",
            "core.fsmonitor",
            &format!("printf ran > '{}'; false", canary.display()),
        ],
    );
    std::fs::write(
        dir.join(".alint.yml"),
        "version: 1\nrules:\n  \
         - id: tracked\n    kind: file_exists\n    paths: a.txt\n    \
         git_tracked_only: true\n    level: error\n  \
         - id: denied\n    kind: git_no_denied_paths\n    denied: [\"*.secret\"]\n    \
         level: error\n  \
         - id: changed\n    kind: file_max_size\n    paths: \"*.txt\"\n    max_bytes: 100\n    \
         scope_filter:\n      changed_since: HEAD\n    level: error\n  \
         - id: per-dir\n    kind: for_each_dir\n    select: sub\n    require:\n      \
         - kind: file_max_size\n        paths: \"{path}/*.txt\"\n        max_bytes: 100\n        \
         scope_filter:\n          changed_since: HEAD\n    level: error\n  \
         - id: readme\n    kind: file_exists\n    paths: a.txt\n    level: error\n",
    )
    .unwrap();
    canary
}

/// Git runs the commands a repository's `.git/config` names, so an untrusted
/// session builds no rule that runs Git, and runs none.
#[test]
#[cfg(unix)]
fn an_untrusted_session_runs_no_git() {
    let dir = tempfile::tempdir().unwrap();
    let canary = git_canary_repository(dir.path());

    let session = build_session_with(dir.path(), &untrusted())
        .unwrap()
        .unwrap();
    assert_eq!(built_rules(&session), ["readme"]);
    assert!(session.withheld);
    session.engine.run(&session.root, &session.index).unwrap();
    assert!(!canary.exists(), "an untrusted session ran Git");

    // Trusted, the same config builds every rule and runs Git.
    let session = build_session(dir.path()).unwrap().unwrap();
    assert_eq!(
        built_rules(&session),
        ["changed", "denied", "per-dir", "readme", "tracked"]
    );
    session.engine.run(&session.root, &session.index).unwrap();
    assert!(
        canary.exists(),
        "positive control: a trusted session runs Git"
    );
}

#[test]
#[cfg(unix)]
fn no_spawn_refuses_a_git_rule_even_when_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let canary = git_canary_repository(dir.path());
    let settings = Settings {
        no_spawn: true,
        ..trusted()
    };
    let error = build_session_with(dir.path(), &settings).unwrap_err();
    assert_eq!(
        error,
        "loading config: rule \"tracked\" (git_tracked_only) runs Git, which runs the commands \
         a repository's Git configuration names; --no-spawn refuses it"
    );
    assert!(!canary.exists());
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

/// A folder with no config of its own lints a document with the config at
/// the top of the nearest repository above it, inside the folder: the
/// nearest directory holding a `.git` directory or file. A directory without
/// `.git` (a vendored package) is never one, and a nearer repository without a
/// config of its own lints nothing.
#[test]
fn a_folder_without_a_config_lints_by_each_repositorys_own() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("workspace");
    let folders = [folder.clone()];
    if alint_dsl::discover(&folder.join("x")).is_some() {
        eprintln!("skipped: a config above the temporary directory");
        return;
    }
    let config = |dir: &Path| {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(".alint.yml"), "version: 1\nrules: []\n").unwrap();
        dir.join(".alint.yml")
    };
    let a = folder.join("a");
    let a_config = config(&a);
    std::fs::create_dir_all(a.join(".git")).unwrap();
    let b = folder.join("b");
    let b_config = config(&b);
    std::fs::write(b.join(".git"), "gitdir: ../b.git\n").unwrap();
    config(&a.join("node_modules/pkg"));
    config(&folder.join("vendor/pkg"));
    std::fs::create_dir_all(a.join("sub/.git")).unwrap();

    let key = session_key(&a.join("src/x.txt"), &folders, None).unwrap();
    assert_eq!((key.config, key.root), (a_config.clone(), a.clone()));
    assert_eq!(
        config_for_document(&a.join("node_modules/pkg/index.js"), &folders),
        Some(a_config)
    );
    assert_eq!(
        config_for_document(&b.join("deep/y.txt"), &folders),
        Some(b_config)
    );
    for orphan in ["vendor/pkg/index.js", "loose.txt", "a/sub/y.txt"] {
        assert_eq!(
            config_for_document(&folder.join(orphan), &folders),
            None,
            "{orphan}"
        );
    }
    // A repository above the folder is not the folder's to pick.
    let inner = [a.join("src")];
    std::fs::remove_file(a.join(".alint.yml")).unwrap();
    assert_eq!(config_for_document(&a.join("src/x.txt"), &inner), None);
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

/// The peak resident set of this process, in KiB (Linux's `VmHWM`).
#[cfg(target_os = "linux")]
fn peak_resident_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap()
}

/// A config the loader refuses for its size, or for not being a regular file,
/// is not read whole to place the error either: a sparse config past the cap
/// used to be read into memory, and a FIFO blocked the server.
#[cfg(target_os = "linux")]
#[test]
fn a_config_refused_for_its_size_or_kind_is_not_read_to_place_the_error() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".alint.yml");
    // Past the 32 MiB cap, and far past what this test process otherwise
    // holds; sparse, so it takes no space on disk.
    std::fs::File::create(&config)
        .unwrap()
        .set_len(2 << 30)
        .unwrap();
    let before = peak_resident_kib();
    let error = build_session_with(dir.path(), &trusted())
        .err()
        .unwrap_or_default();
    let grown = peak_resident_kib().saturating_sub(before);
    assert!(error.contains("maximum supported size"), "{error}");
    assert!(grown < 512 * 1024, "the peak grew by {grown} KiB");

    let fifo = dir.path().join("fifo.yml");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|status| status.success());
    if !made {
        eprintln!("mkfifo unavailable; skipping the FIFO case");
        return;
    }
    let settings = Settings {
        config: Some(fifo),
        ..trusted()
    };
    let root = dir.path().to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(build_session_with(&root, &settings).err());
    });
    let error = receiver
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the session build returns")
        .unwrap_or_default();
    assert!(error.contains("not a regular file"), "{error}");
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
    // `--untrusted` overrides both the client and `--config`.
    let untrusted = LspOptions {
        untrusted: true,
        ..options
    };
    assert!(!untrusted.settings(true).trusted);
    assert!(!untrusted.settings(false).trusted);
}
