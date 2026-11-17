//! `alint lsp` workspace trust, driving the real binary over stdio:
//!   1. an untrusted workspace runs no command rule, custom fact or Git, reads
//!      no environment value and fetches no remote `extends:`, lints everything
//!      else, and says so once (`window/showMessage`); a trusted one runs
//!      them (the positive controls);
//!   2. a nested or vendored `.alint.yml` never governs a file as a top-level
//!      config, and the server's verdict matches `alint check`'s;
//!   3. deciding which config governs a file resolves no nested config's
//!      `extends:`;
//!   4. `--config`, `--no-gitignore` and `--no-spawn` apply to the server.
//!
//! Each canary is a `touch` of a sentinel file, and each network canary a
//! local listener that must see no connection. Unix-only for the shared
//! harness's `file://` URIs and the canaries' `/bin/sh`.
#![cfg(unix)]

use std::collections::HashMap;
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::lsp_common::{Server, recv_response, spawn_server_with};

fn uri_of(path: &Path) -> String {
    format!("file://{}", path.to_str().unwrap())
}

/// A server for `root`, started with `args` and `env`, initialized with
/// `options` as its `initializationOptions`.
fn start(root: &Path, args: &[&str], env: &[(&str, &str)], options: Option<Value>) -> Server {
    let mut server = spawn_server_with(root, args, env);
    let mut params = json!({ "rootUri": uri_of(root), "capabilities": {} });
    if let Some(options) = options {
        params["initializationOptions"] = options;
    }
    server.send(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": params }));
    recv_response(&server.rx, 1);
    server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    server
}

fn trusted() -> Value {
    json!({ "trusted": true })
}

fn open(server: &mut Server, path: &Path) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": uri_of(path), "languageId": "plaintext", "version": 1, "text": text
        }}
    }));
}

/// What the server sent until every URI in `expect` was published and then
/// nothing arrived for a quiet spell: the last diagnostics per URI, and every
/// `window/showMessage` text.
struct Seen {
    diagnostics: HashMap<String, Vec<Value>>,
    messages: Vec<String>,
}

impl Seen {
    fn codes(&self, uri: &str) -> Vec<String> {
        let mut codes: Vec<String> = self.diagnostics[uri]
            .iter()
            .map(|d| d["code"].as_str().unwrap_or_default().to_string())
            .collect();
        codes.sort();
        codes
    }

    fn text(&self, uri: &str) -> String {
        serde_json::to_string(&self.diagnostics[uri]).unwrap()
    }

    fn trust_notes(&self) -> usize {
        self.messages
            .iter()
            .filter(|message| message.contains("workspace is not trusted"))
            .count()
    }
}

fn settle(server: &Server, expect: &[&str]) -> Seen {
    let quiet = Duration::from_millis(1500);
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut seen = Seen {
        diagnostics: HashMap::new(),
        messages: Vec::new(),
    };
    loop {
        let waiting = expect
            .iter()
            .any(|uri| !seen.diagnostics.contains_key(*uri));
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = if waiting {
            remaining
        } else {
            quiet.min(remaining)
        };
        let Ok(msg) = server.rx.recv_timeout(timeout) else {
            break;
        };
        match msg["method"].as_str() {
            Some("textDocument/publishDiagnostics") => {
                let uri = msg["params"]["uri"].as_str().unwrap_or_default();
                let diagnostics = msg["params"]["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                seen.diagnostics.insert(uri.to_string(), diagnostics);
            }
            Some("window/showMessage") => {
                let text = msg["params"]["message"].as_str().unwrap_or_default();
                seen.messages.push(text.to_string());
            }
            _ => {}
        }
    }
    for uri in expect {
        assert!(seen.diagnostics.contains_key(*uri), "no publish for {uri}");
    }
    seen
}

/// Whether `listener` (non-blocking) accepted a connection within `wait`.
fn connected(listener: &TcpListener, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    loop {
        match listener.accept() {
            Ok(_) => return true,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("listener failed: {error}"),
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

fn touch(sentinel: &Path) -> String {
    format!("[\"sh\", \"-c\", \"touch {}\"]", sentinel.display())
}

const NO_TODO: &str = "  - id: no-todo\n    kind: file_content_forbidden\n    \
                       paths: \"**/*.txt\"\n    pattern: TODO\n    level: error\n";

/// (1) The repository's own command rule and custom fact do not run until
/// the client says the workspace is trusted; the other rule lints either way.
#[test]
fn an_untrusted_workspace_runs_no_command_rule_or_custom_fact() {
    for trusted_workspace in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let rule_canary = root.join("RULE_RAN");
        let fact_canary = root.join("FACT_RAN");
        std::fs::write(
            root.join(".alint.yml"),
            format!(
                "version: 1\nfacts:\n  - id: probe\n    custom:\n      argv: {}\nrules:\n  \
                 - id: run\n    kind: command\n    paths: \"*.txt\"\n    command: {}\n    \
                 level: error\n{NO_TODO}",
                touch(&fact_canary),
                touch(&rule_canary)
            ),
        )
        .unwrap();
        let file = root.join("a.txt");
        std::fs::write(&file, "TODO\n").unwrap();

        let options = trusted_workspace.then(trusted);
        let mut server = start(root, &[], &[], options);
        open(&mut server, &file);
        let seen = settle(&server, &[&uri_of(&file)]);
        assert!(
            seen.codes(&uri_of(&file)).contains(&"no-todo".to_string()),
            "{:?}",
            seen.diagnostics
        );
        assert_eq!(rule_canary.exists(), trusted_workspace, "the command rule");
        assert_eq!(fact_canary.exists(), trusted_workspace, "the custom fact");
        assert_eq!(
            seen.trust_notes(),
            usize::from(!trusted_workspace),
            "{:?}",
            seen.messages
        );
        server.shutdown();
    }
}

/// Run `git` with the host's configuration left out, in `dir`.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(output.status.success(), "{output:?}");
}

/// (1) Git runs the commands a repository's `.git/config` names
/// (`core.fsmonitor`, when it reads the index), so a rule that runs Git does
/// not run until the workspace is trusted, nor under `--no-spawn` at all.
#[test]
fn an_untrusted_workspace_runs_no_git() {
    for (trusted_workspace, args) in [
        (false, &[][..]),
        (true, &["--no-spawn"][..]),
        // `--untrusted` makes even a workspace the client trusts untrusted.
        (true, &["--untrusted"][..]),
        (true, &[][..]),
    ] {
        let refused = args.contains(&"--no-spawn");
        let untrusted = !trusted_workspace || args.contains(&"--untrusted");
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let canary = root.join("GIT_RAN");
        let file = root.join("a.txt");
        std::fs::write(&file, "TODO\n").unwrap();
        git(root, &["init", "-q"]);
        git(root, &["add", "a.txt"]);
        git(
            root,
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
            root,
            &[
                "config",
                "core.fsmonitor",
                &format!("printf ran > '{}'; false", canary.display()),
            ],
        );
        let config = root.join(".alint.yml");
        std::fs::write(
            &config,
            format!(
                "version: 1\nrules:\n  - id: tracked\n    kind: file_exists\n    \
                 paths: a.txt\n    git_tracked_only: true\n    level: error\n  \
                 - id: denied\n    kind: git_no_denied_paths\n    denied: [\"*.secret\"]\n    \
                 level: error\n{NO_TODO}"
            ),
        )
        .unwrap();

        let options = trusted_workspace.then(trusted);
        let mut server = start(root, args, &[], options);
        open(&mut server, &file);
        if refused {
            let seen = settle(&server, &[&uri_of(&config)]);
            let text = seen.text(&uri_of(&config));
            assert!(
                text.contains(r#"rule \"tracked\" (git_tracked_only) runs Git"#),
                "{text}"
            );
        } else {
            let seen = settle(&server, &[&uri_of(&file)]);
            assert!(
                seen.codes(&uri_of(&file)).contains(&"no-todo".to_string()),
                "{:?}",
                seen.diagnostics
            );
            assert_eq!(seen.trust_notes(), usize::from(untrusted), "{args:?}");
        }
        let runs_git = !untrusted && !refused;
        assert_eq!(canary.exists(), runs_git, "{trusted_workspace} {args:?}");
        server.shutdown();
    }
}

/// (1) Untrusted, `{{env.*}}` resolves to its default and `env.*` in a
/// `when:` reads nothing; trusted, both read the server's environment.
#[test]
fn an_untrusted_workspace_reads_no_environment_value() {
    for trusted_workspace in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let config = root.join(".alint.yml");
        std::fs::write(
            &config,
            "version: 1\nrules:\n  - id: need-file\n    kind: file_exists\n    \
             paths: \"{{env.ALINT_TRUST_PROBE | default('missing.txt')}}\"\n    \
             level: error\n  - id: gated\n    kind: file_content_forbidden\n    \
             paths: \"*.txt\"\n    pattern: TODO\n    \
             when: 'env.ALINT_TRUST_PROBE == \"present.txt\"'\n    level: error\n",
        )
        .unwrap();
        std::fs::write(root.join("present.txt"), "TODO\n").unwrap();
        let file = root.join("present.txt");

        let options = trusted_workspace.then(trusted);
        let mut server = start(root, &[], &[("ALINT_TRUST_PROBE", "present.txt")], options);
        open(&mut server, &file);
        let seen = settle(&server, &[&uri_of(&file), &uri_of(&config)]);
        if trusted_workspace {
            assert_eq!(seen.codes(&uri_of(&file)), ["gated"]);
            assert_eq!(seen.codes(&uri_of(&config)), Vec::<String>::new());
        } else {
            assert_eq!(seen.codes(&uri_of(&file)), Vec::<String>::new());
            assert_eq!(seen.codes(&uri_of(&config)), ["need-file"]);
            assert_eq!(seen.trust_notes(), 1, "{:?}", seen.messages);
        }
        server.shutdown();
    }
}

/// (1) An untrusted workspace's remote `extends:` is refused before any
/// connection, with the reason on the config; trusted, the server connects
/// (the positive control for the listener).
#[test]
fn an_untrusted_workspace_fetches_no_remote_extends() {
    for trusted_workspace in [false, true] {
        let (listener, port) = listener();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let config = root.join(".alint.yml");
        std::fs::write(
            &config,
            format!(
                "version: 1\nextends:\n  - \"https://127.0.0.1:{port}/base.yml#sha256-{}\"\n\
                 rules:\n{NO_TODO}",
                "0".repeat(64)
            ),
        )
        .unwrap();
        let file = root.join("a.txt");
        std::fs::write(&file, "TODO\n").unwrap();
        let options = trusted_workspace.then(trusted);
        let mut server = start(root, &[], &[], options);
        open(&mut server, &file);
        if trusted_workspace {
            assert!(
                connected(&listener, Duration::from_secs(20)),
                "positive control: a trusted workspace fetches its remote extends"
            );
            // Closing the listener fails the TLS handshake the server waits on.
            drop(listener);
            settle(&server, &[&uri_of(&config)]);
        } else {
            let seen = settle(&server, &[&uri_of(&config)]);
            let text = seen.text(&uri_of(&config));
            assert!(text.contains("remote `extends` is disabled"), "{text}");
            assert!(text.contains("workspace is not trusted"), "{text}");
            assert!(!connected(&listener, Duration::ZERO), "an untrusted fetch");
        }
        server.shutdown();
    }
}

/// (2) A vendored package's `.alint.yml`, in a directory the workspace's
/// `.gitignore` excludes, used to govern its files as a top-level config:
/// opening `index.js` there ran its command rule. Now the workspace's own
/// config governs the file, as `alint check` does from the workspace root.
#[test]
fn a_vendored_config_never_governs_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let canary = root.join("CANARY");
    std::fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: readme\n    kind: file_exists\n    paths: README.md\n    \
         level: error\n",
    )
    .unwrap();
    std::fs::write(root.join("README.md"), "hi\n").unwrap();
    let package = root.join("node_modules/evil-pkg");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: pwn\n    kind: command\n    paths: \"*.js\"\n    \
             command: {}\n    level: error\n",
            touch(&canary)
        ),
    )
    .unwrap();
    let file = package.join("index.js");
    std::fs::write(&file, "module.exports = 1;\n").unwrap();

    let cli = Command::new(env!("CARGO_BIN_EXE_alint"))
        .arg("check")
        .arg(root)
        .output()
        .unwrap();
    let cli_out = String::from_utf8_lossy(&cli.stdout);
    assert!(cli.status.success(), "{cli_out}");
    assert!(cli_out.contains("All 1 rule(s) passed"), "{cli_out}");
    assert!(!canary.exists());

    // Trusted: the workspace's own command rules would run, so this is the
    // strongest case.
    let mut server = start(root, &[], &[], Some(trusted()));
    open(&mut server, &file);
    let seen = settle(&server, &[&uri_of(&file)]);
    assert_eq!(
        seen.codes(&uri_of(&file)),
        Vec::<String>::new(),
        "the CLI passes the file"
    );
    assert!(!canary.exists(), "the vendored config's command rule ran");
    server.shutdown();
}

/// (2) A workspace folder with no config of its own, holding two
/// repositories with a config each: each file is linted by its own
/// repository's config, as `alint check` run in that repository does.
#[test]
fn each_repository_in_a_folder_lints_by_its_own_config() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("workspace");
    let rule = |id: &str, word: &str| {
        format!(
            "version: 1\nrules:\n  - id: {id}\n    kind: file_content_forbidden\n    \
             paths: \"**/*.txt\"\n    pattern: {word}\n    level: error\n"
        )
    };
    let mut files = Vec::new();
    for (repo, id, word) in [("a", "no-alpha", "ALPHA"), ("b", "no-beta", "BETA")] {
        let top = folder.join(repo);
        std::fs::create_dir_all(top.join(".git")).unwrap();
        std::fs::create_dir_all(top.join("src")).unwrap();
        std::fs::write(top.join(".alint.yml"), rule(id, word)).unwrap();
        let file = top.join("src/notes.txt");
        std::fs::write(&file, "ALPHA BETA\n").unwrap();
        files.push(file);
    }
    // A worktree or submodule's `.git` is a file; it is a repository top too.
    std::fs::remove_dir(folder.join("b/.git")).unwrap();
    std::fs::write(folder.join("b/.git"), "gitdir: ../b.git\n").unwrap();

    let mut server = start(&folder, &[], &[], Some(trusted()));
    for file in &files {
        open(&mut server, file);
    }
    let uris: Vec<String> = files.iter().map(|file| uri_of(file)).collect();
    let seen = settle(&server, &[&uris[0], &uris[1]]);
    assert_eq!(seen.codes(&uris[0]), ["no-alpha"]);
    assert_eq!(seen.codes(&uris[1]), ["no-beta"]);
    server.shutdown();
}

/// (2) The vendored-config repro in a folder with no config of its own: a
/// directory without `.git` is never a repository top, so the package's
/// `.alint.yml` governs nothing and its command rule never runs, whether the
/// folder is a repository or holds one.
#[test]
fn a_vendored_config_never_governs_a_folder_without_one() {
    for in_repository in [true, false] {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("workspace");
        let canary = tmp.path().join("CANARY");
        let top = if in_repository {
            folder.clone()
        } else {
            folder.join("repo")
        };
        std::fs::create_dir_all(top.join(".git")).unwrap();
        let package = top.join("node_modules/evil-pkg");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join(".alint.yml"),
            format!(
                "version: 1\nrules:\n  - id: pwn\n    kind: command\n    paths: \"*.js\"\n    \
                 command: {}\n    level: error\n",
                touch(&canary)
            ),
        )
        .unwrap();
        let file = package.join("index.js");
        std::fs::write(&file, "module.exports = 1;\n").unwrap();

        let mut server = start(&folder, &[], &[], Some(trusted()));
        open(&mut server, &file);
        let seen = settle(&server, &[]);
        assert!(
            !canary.exists(),
            "the vendored config's command rule ran (in_repository: {in_repository})"
        );
        if let Some(diagnostics) = seen.diagnostics.get(&uri_of(&file)) {
            assert_eq!(diagnostics, &Vec::<Value>::new());
        }
        server.shutdown();
    }
}

/// (3) Deciding which config governs a file under `vendor/pkg` resolved that
/// nested config's `extends:` as if it were top-level: its `{{env.*}}` went
/// into the URL and the server connected to the host it named.
#[test]
fn a_nested_configs_extends_is_never_resolved() {
    let (listener, port) = listener();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        format!("version: 1\nnested_configs: true\nrules:\n{NO_TODO}"),
    )
    .unwrap();
    let package = root.join("vendor/pkg");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join(".alint.yml"),
        format!(
            "version: 1\nextends:\n  - \
             \"https://127.0.0.1:{port}/{{{{env.AUDIT_CANARY}}}}.yml#sha256-{}\"\nrules: []\n",
            "0".repeat(64)
        ),
    )
    .unwrap();
    let file = package.join("a.txt");
    std::fs::write(&file, "TODO\n").unwrap();

    let mut server = start(root, &[], &[("AUDIT_CANARY", "leaked")], Some(trusted()));
    open(&mut server, &file);
    // Waits for the publish; a server that connects first hangs in the
    // handshake and never publishes, so check the listener as it goes.
    let deadline = Instant::now() + Duration::from_secs(30);
    let file_uri = uri_of(&file);
    loop {
        assert!(
            !connected(&listener, Duration::ZERO),
            "the server fetched a nested config's extends"
        );
        let msg = server
            .rx
            .recv_timeout(Duration::from_millis(100))
            .unwrap_or(Value::Null);
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == file_uri {
            break;
        }
        assert!(Instant::now() < deadline, "no publish for {file_uri}");
    }
    assert!(!connected(&listener, Duration::from_millis(500)));
    server.shutdown();
}

/// (4) `--config` lints with that config (the user's own, so its command rule
/// runs whatever the client says), and `--no-gitignore` lints an ignored
/// file. Both were silently ignored before.
#[test]
fn the_server_honors_config_and_no_gitignore() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("ws");
    std::fs::create_dir_all(workspace.join("ignored")).unwrap();
    std::fs::write(workspace.join(".gitignore"), "ignored/\n").unwrap();
    std::fs::write(
        workspace.join(".alint.yml"),
        format!("version: 1\nrules:\n{NO_TODO}"),
    )
    .unwrap();
    let canary = tmp.path().join("MINE_RAN");
    let mine = tmp.path().join("mine.yml");
    std::fs::write(
        &mine,
        format!(
            "version: 1\nrules:\n  - id: no-fixme\n    kind: file_content_forbidden\n    \
             paths: \"**/*.txt\"\n    pattern: FIXME\n    level: error\n  - id: run\n    \
             kind: command\n    paths: \"*.txt\"\n    command: {}\n    level: error\n",
            touch(&canary)
        ),
    )
    .unwrap();
    let file = workspace.join("a.txt");
    std::fs::write(&file, "TODO FIXME\n").unwrap();

    let mut server = start(&workspace, &["-c", mine.to_str().unwrap()], &[], None);
    open(&mut server, &file);
    let seen = settle(&server, &[&uri_of(&file)]);
    assert_eq!(seen.codes(&uri_of(&file)), ["no-fixme"]);
    assert!(canary.exists(), "the user's own config runs trusted");
    assert_eq!(seen.trust_notes(), 0);
    server.shutdown();

    let ignored = workspace.join("ignored/b.txt");
    std::fs::write(&ignored, "TODO\n").unwrap();
    for (args, expected) in [
        (&[][..], Vec::<String>::new()),
        (&["--no-gitignore"][..], vec!["no-todo".to_string()]),
    ] {
        let mut server = start(&workspace, args, &[], None);
        open(&mut server, &ignored);
        let seen = settle(&server, &[&uri_of(&ignored)]);
        assert_eq!(seen.codes(&uri_of(&ignored)), expected, "{args:?}");
        server.shutdown();
    }
}

/// (4) `--no-spawn` refuses the config, trusted or not, naming the rule.
#[test]
fn no_spawn_refuses_a_spawning_config_in_the_server() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let canary = root.join("CANARY");
    let config = root.join(".alint.yml");
    std::fs::write(
        &config,
        format!(
            "version: 1\nrules:\n  - id: run\n    kind: command\n    paths: \"*.txt\"\n    \
             command: {}\n    level: error\n{NO_TODO}",
            touch(&canary)
        ),
    )
    .unwrap();
    let file = root.join("a.txt");
    std::fs::write(&file, "TODO\n").unwrap();
    let mut server = start(root, &["--no-spawn"], &[], Some(trusted()));
    open(&mut server, &file);
    let seen = settle(&server, &[&uri_of(&config)]);
    let text = seen.text(&uri_of(&config));
    assert!(
        text.contains(r#"rule \"run\" (kind: command) spawns a process; --no-spawn refuses it"#),
        "{text}"
    );
    assert!(!canary.exists());
    server.shutdown();
}

/// A `--config` that does not exist fails at startup, not as a diagnostic on
/// a file the editor cannot show.
#[test]
fn a_missing_server_config_fails_at_startup() {
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(["lsp", "-c", "missing.yml"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config file not found"), "{stderr}");
}
