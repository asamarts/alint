//! `alint lsp` regression tests for the 2026-10 audit findings, each
//! driving the real binary over stdio:
//!   1. diagnostic columns are UTF-16 code units (the LSP default),
//!   2. a config file's own findings survive a full check (not wiped by
//!      the "clear the old config error" publish),
//!   3. a full check overlays unsaved buffers instead of republishing
//!      stale on-disk results,
//!   4. config discovery (the workspace folder's `.alint.yml`), a broken
//!      config disables the stale engine, `baseline:` is honored,
//!      multi-root workspaces, and `exit` terminates the process.
//!
//! Unix-only for the same `file://` reason as the shared harness.
#![cfg(unix)]

use crate::lsp_common;

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use lsp_common::{Server, recv_response, spawn_server, wait_for_diagnostics};

/// Read inbound messages until every URI in `expect` has been published and
/// then none arrives for `quiet`, returning the LAST `publishDiagnostics`
/// per URI — the state the editor ends up showing. Waiting for the expected
/// publishes first keeps a slow server (a loaded host, or the crate's other
/// tests running alongside in the same binary) from ending the quiet window
/// before the first one arrives. One 60 s deadline bounds the whole wait, so a
/// server that never goes quiet fails the test instead of hanging it.
fn settle(rx: &Receiver<Value>, expect: &[&str], quiet: Duration) -> HashMap<String, Vec<Value>> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut last: HashMap<String, Vec<Value>> = HashMap::new();
    loop {
        let waiting = expect.iter().any(|uri| !last.contains_key(*uri));
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = if waiting {
            remaining
        } else {
            quiet.min(remaining)
        };
        let Ok(msg) = rx.recv_timeout(timeout) else {
            break;
        };
        if msg["method"] == "textDocument/publishDiagnostics" {
            last.insert(
                msg["params"]["uri"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                msg["params"]["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
    last
}

const QUIET: Duration = Duration::from_millis(1500);

fn uri_of(path: &Path) -> String {
    format!("file://{}", path.to_str().unwrap())
}

fn initialize(server: &mut Server, params: &Value) {
    server.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": params
    }));
    recv_response(&server.rx, 1);
    server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
}

fn start(root: &Path) -> Server {
    let mut server = spawn_server(root);
    initialize(
        &mut server,
        &json!({ "rootUri": uri_of(root), "capabilities": {} }),
    );
    server
}

fn open(server: &mut Server, uri: &str, text: &str) {
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": uri, "languageId": "plaintext", "version": 1, "text": text
        }}
    }));
}

fn change(server: &mut Server, uri: &str, version: i32, text: &str) {
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": text }]
        }
    }));
}

fn codes(diags: &[Value]) -> Vec<String> {
    diags
        .iter()
        .map(|d| d["code"].as_str().unwrap_or_default().to_string())
        .collect()
}

const NO_TODO: &str = "version: 1\nrules:\n  \
     - id: no-todo\n    kind: file_content_forbidden\n    \
     paths: \"**/*.txt\"\n    pattern: 'TODO'\n    level: error\n";

/// (1) A zero-width char after two emoji: alint reports `char` column 4;
/// the LSP position must be UTF-16 character 5 (each emoji is two code
/// units), spanning the one-unit ZWSP.
#[test]
fn lsp_diagnostic_columns_are_utf16() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-zw\n    kind: no_zero_width_chars\n    \
         paths: \"*.txt\"\n    level: error\n",
    )
    .unwrap();
    let text = "\u{1F600}\u{1F600}x\u{200B}y\n";
    std::fs::write(root.join("a.txt"), text).unwrap();
    let file_uri = uri_of(&root.join("a.txt"));

    let mut server = start(root);
    open(&mut server, &file_uri, text);
    let diags = wait_for_diagnostics(&server.rx, &file_uri);
    assert_eq!(diags.len(), 1, "{diags:?}");
    let range = &diags[0]["range"];
    assert_eq!(range["start"]["line"], 0);
    assert_eq!(
        range["start"]["character"], 5,
        "two emoji = 4 UTF-16 units, then 'x': the ZWSP is at character 5: {range}"
    );
    assert_eq!(range["end"]["character"], 6, "{range}");
    server.shutdown();
}

/// (2) A path-less finding is anchored on the config file. The full check
/// must leave it visible: before the fix, a trailing "clear the old config
/// error" publish replaced it with an empty list.
#[test]
fn lsp_config_file_findings_are_not_wiped() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let config = "version: 1\nrules:\n  - id: need-license\n    kind: file_exists\n    \
                  paths: LICENSE\n    level: error\n";
    std::fs::write(root.join(".alint.yml"), config).unwrap();
    let cfg_uri = uri_of(&root.join(".alint.yml"));

    let mut server = start(root);
    open(&mut server, &cfg_uri, config);
    let last = settle(&server.rx, &[&cfg_uri], QUIET);
    let diags = last.get(&cfg_uri).expect("config file got diagnostics");
    assert_eq!(
        codes(diags),
        vec!["need-license"],
        "the anchored finding must be the config file's final state: {last:?}"
    );
    server.shutdown();
}

/// (3) An unsaved buffer that trips a rule keeps its diagnostic when a
/// full check runs (another document opens / saves): the check overlays
/// the buffer instead of republishing the clean on-disk content.
#[test]
fn lsp_full_check_does_not_clobber_unsaved_buffer() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), NO_TODO).unwrap();
    std::fs::write(root.join("a.txt"), "clean\n").unwrap();
    std::fs::write(root.join("b.txt"), "clean\n").unwrap();
    let a = uri_of(&root.join("a.txt"));
    let b = uri_of(&root.join("b.txt"));

    let mut server = start(root);
    open(&mut server, &a, "clean\n");
    wait_for_diagnostics(&server.rx, &a);
    change(&mut server, &a, 2, "a TODO, unsaved\n");
    let after_change = wait_for_diagnostics(&server.rx, &a);
    assert_eq!(codes(&after_change), vec!["no-todo"]);

    // Opening b triggers a full check from disk.
    open(&mut server, &b, "clean\n");
    let last = settle(&server.rx, &[&a], QUIET);
    let final_a = last.get(&a).expect("the full check republishes a.txt");
    assert_eq!(
        codes(final_a),
        vec!["no-todo"],
        "a full check must not paint the clean on-disk a.txt over its unsaved buffer"
    );
    server.shutdown();
}

/// (4a) The workspace folder's config governs every file in it, as `alint
/// check` from the folder does: a file under `pkg/` is linted by the root's
/// config, not by `pkg/.alint.yml`, which (without `nested_configs: true`)
/// the CLI never reads. Until v0.23 the nearest config governed, which gave
/// a vendored package's `.alint.yml` the trust of the user's own.
#[test]
fn lsp_a_package_config_does_not_govern_without_nested_configs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), NO_TODO).unwrap();
    std::fs::create_dir(root.join("pkg")).unwrap();
    std::fs::write(
        root.join("pkg/.alint.yml"),
        "version: 1\nrules:\n  - id: pkg-no-fixme\n    kind: file_content_forbidden\n    \
         paths: \"*.txt\"\n    pattern: 'FIXME'\n    level: error\n",
    )
    .unwrap();
    let text = "TODO and FIXME\n";
    std::fs::write(root.join("pkg/a.txt"), text).unwrap();
    let file_uri = uri_of(&root.join("pkg/a.txt"));

    let cli = Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(["check", "--format", "json"])
        .current_dir(root)
        .output()
        .unwrap();
    let cli_out = String::from_utf8_lossy(&cli.stdout);
    assert!(
        cli_out.contains("no-todo") && !cli_out.contains("pkg-no-fixme"),
        "{cli_out}"
    );

    let mut server = start(root);
    open(&mut server, &file_uri, text);
    let diags = wait_for_diagnostics(&server.rx, &file_uri);
    assert_eq!(
        codes(&diags),
        vec!["no-todo"],
        "pkg/a.txt is governed by the workspace's .alint.yml: {diags:?}"
    );
    server.shutdown();
}

/// (4a') `nested_configs: true` on the workspace config: like the CLI,
/// a file under `pkg/` gets the root's rules AND `pkg/.alint.yml`'s.
#[test]
fn lsp_honors_nested_configs_like_the_cli() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nnested_configs: true\nrules:\n  - id: root-no-todo\n    \
         kind: file_content_forbidden\n    paths: \"**/*.txt\"\n    pattern: 'TODO'\n    \
         level: error\n",
    )
    .unwrap();
    std::fs::create_dir(root.join("pkg")).unwrap();
    std::fs::write(
        root.join("pkg/.alint.yml"),
        "version: 1\nrules:\n  - id: pkg-no-fixme\n    kind: file_content_forbidden\n    \
         paths: \"*.txt\"\n    pattern: 'FIXME'\n    level: error\n",
    )
    .unwrap();
    let text = "TODO and FIXME\n";
    std::fs::write(root.join("pkg/a.txt"), text).unwrap();
    let file_uri = uri_of(&root.join("pkg/a.txt"));

    let cli = Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(["check", "--format", "json"])
        .current_dir(root)
        .output()
        .unwrap();
    let cli_out = String::from_utf8_lossy(&cli.stdout);
    assert!(
        cli_out.contains("root-no-todo") && cli_out.contains("pkg-no-fixme"),
        "{cli_out}"
    );

    let mut server = start(root);
    open(&mut server, &file_uri, text);
    let last = settle(&server.rx, &[&file_uri], QUIET);
    let mut got = codes(last.get(&file_uri).expect("pkg/a.txt is published"));
    got.sort();
    assert_eq!(got, vec!["pkg-no-fixme", "root-no-todo"], "{last:?}");

    // The per-keystroke path uses the same governing config.
    change(&mut server, &file_uri, 2, "TODO FIXME again\n");
    let mut got = codes(&wait_for_diagnostics(&server.rx, &file_uri));
    got.sort();
    assert_eq!(got, vec!["pkg-no-fixme", "root-no-todo"]);
    server.shutdown();
}

/// (4b) Once the config becomes invalid, the stale engine must not keep
/// producing per-keystroke diagnostics: the document is cleared and only
/// the config error (on the config file) remains.
#[test]
fn lsp_broken_config_disables_stale_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let cfg = root.join(".alint.yml");
    std::fs::write(&cfg, NO_TODO).unwrap();
    std::fs::write(root.join("a.txt"), "TODO\n").unwrap();
    let a = uri_of(&root.join("a.txt"));
    let cfg_uri = uri_of(&cfg);

    let mut server = start(root);
    open(&mut server, &a, "TODO\n");
    assert_eq!(
        codes(&wait_for_diagnostics(&server.rx, &a)),
        vec!["no-todo"]
    );

    std::fs::write(&cfg, "version: 1\nrules: [ this is not valid\n").unwrap();
    server.send(&json!({
        "jsonrpc": "2.0", "method": "workspace/didChangeWatchedFiles",
        "params": { "changes": [{ "uri": cfg_uri, "type": 2 }] }
    }));
    let after_break = settle(&server.rx, &[&cfg_uri, &a], QUIET);
    assert_eq!(
        after_break.get(&cfg_uri).map(Vec::len),
        Some(1),
        "the config error is shown on the config: {after_break:?}"
    );
    assert_eq!(
        after_break.get(&a).map(Vec::len),
        Some(0),
        "a.txt's stale findings are cleared: {after_break:?}"
    );

    change(&mut server, &a, 2, "TODO TODO\n");
    // Barrier: repair the config with the rule renamed. Every publish for a.txt
    // until the repaired engine's arrives must carry nothing from the stale
    // engine's `no-todo` rule; a quiet window alone could end before a late
    // stale publish.
    std::fs::write(&cfg, NO_TODO.replace("no-todo", "no-todo-repaired")).unwrap();
    server.send(&json!({
        "jsonrpc": "2.0", "method": "workspace/didChangeWatchedFiles",
        "params": { "changes": [{ "uri": cfg_uri, "type": 2 }] }
    }));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let msg = server
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("a.txt is republished once the config is repaired");
        if msg["method"] != "textDocument/publishDiagnostics" || msg["params"]["uri"] != a.as_str()
        {
            continue;
        }
        let got = codes(msg["params"]["diagnostics"].as_array().unwrap());
        assert!(
            !got.iter().any(|code| code == "no-todo"),
            "the stale engine published diagnostics after the config broke: {got:?}"
        );
        if got.iter().any(|code| code == "no-todo-repaired") {
            break;
        }
    }
    server.shutdown();
}

/// (4c) `baseline:` is honored: a grandfathered finding `alint check`
/// hides must not show in the editor.
#[test]
fn lsp_honors_config_baseline() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        format!(
            "{NO_TODO}  - id: no-fixme\n    kind: file_content_forbidden\n    \
             paths: \"**/*.txt\"\n    pattern: 'FIXME'\n    level: error\n\
             baseline: .alint-baseline.json\n"
        ),
    )
    .unwrap();
    std::fs::write(root.join("a.txt"), "old TODO\n").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(["baseline", "--quiet"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());
    let a = uri_of(&root.join("a.txt"));

    let mut server = start(root);
    open(&mut server, &a, "old TODO\n");
    let diags = wait_for_diagnostics(&server.rx, &a);
    assert!(diags.is_empty(), "baselined finding hidden: {diags:?}");

    // A NEW finding (not in the baseline) is still reported on change,
    // while the grandfathered one stays hidden.
    change(&mut server, &a, 2, "old TODO\nnew FIXME\n");
    let diags = wait_for_diagnostics(&server.rx, &a);
    assert_eq!(codes(&diags), vec!["no-fixme"], "{diags:?}");
    server.shutdown();
}

/// (4d) Multi-root: a document in the SECOND workspace folder is linted
/// by that folder's config.
#[test]
fn lsp_lints_every_workspace_folder() {
    let one = tempfile::tempdir().unwrap();
    let two = tempfile::tempdir().unwrap();
    std::fs::write(one.path().join(".alint.yml"), NO_TODO).unwrap();
    std::fs::write(two.path().join(".alint.yml"), NO_TODO).unwrap();
    std::fs::write(two.path().join("b.txt"), "TODO\n").unwrap();
    let b = uri_of(&two.path().join("b.txt"));

    let mut server = spawn_server(one.path());
    initialize(
        &mut server,
        &json!({
            "workspaceFolders": [
                { "uri": uri_of(one.path()), "name": "one" },
                { "uri": uri_of(two.path()), "name": "two" }
            ],
            "capabilities": {}
        }),
    );
    open(&mut server, &b, "TODO\n");
    let last = settle(&server.rx, &[&b], QUIET);
    assert_eq!(
        last.get(&b).map(|d| codes(d)),
        Some(vec!["no-todo".to_string()]),
        "{last:?}"
    );
    server.shutdown();
}

/// Wait for the next `publishDiagnostics` for `uri`, returning its
/// version and diagnostics.
fn next_publish(rx: &Receiver<Value>, uri: &str) -> (Option<i64>, Vec<Value>) {
    loop {
        let msg = rx
            .recv_timeout(Duration::from_secs(60))
            .expect("timed out waiting for publishDiagnostics");
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == uri {
            return (
                msg["params"]["version"].as_i64(),
                msg["params"]["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
}

/// (5) `didChange` handlers run concurrently: a slow evaluation of an
/// older (large) buffer must not publish after the newer (small) one, or
/// its stale diagnostics stick on screen.
#[test]
fn lsp_stale_reeval_result_is_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), NO_TODO).unwrap();
    std::fs::write(root.join("a.txt"), "clean\n").unwrap();
    let a = uri_of(&root.join("a.txt"));
    let big = format!("{}TODO\n", format!("{}\n", "x".repeat(200)).repeat(40_000));

    let mut server = start(root);
    open(&mut server, &a, "clean\n");
    wait_for_diagnostics(&server.rx, &a);

    // Calibrate: how long one evaluation of the big buffer takes.
    let t0 = Instant::now();
    change(&mut server, &a, 2, &big);
    let (_, diags) = next_publish(&server.rx, &a);
    assert_eq!(codes(&diags), vec!["no-todo"]);
    let eval = t0.elapsed();
    change(&mut server, &a, 3, "clean\n");
    next_publish(&server.rx, &a);

    // Race: the big buffer, immediately superseded by a clean one.
    change(&mut server, &a, 4, &big);
    change(&mut server, &a, 5, "clean again\n");
    let quiet = (eval * 4).max(Duration::from_secs(2));
    let mut last = None;
    while let Ok(msg) = server.rx.recv_timeout(quiet) {
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == a {
            assert_ne!(
                msg["params"]["version"].as_i64(),
                Some(4),
                "the superseded buffer's diagnostics must be dropped: {msg}"
            );
            last = Some(msg);
        }
    }
    let last = last.expect("the current buffer is published");
    assert_eq!(last["params"]["version"].as_i64(), Some(5), "{last}");
    assert_eq!(last["params"]["diagnostics"], json!([]), "{last}");
    server.shutdown();
}

/// Wait up to `limit` for the child to exit on its own (stdin stays open).
fn wait_exit(server: &mut Server, limit: Duration) -> Option<i32> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Some(status) = server.child.try_wait().unwrap() {
            return status.code();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// (4e) `shutdown` + `exit` terminates the process with code 0 even
/// though the client keeps stdin open.
#[test]
fn lsp_exits_zero_after_shutdown_and_exit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut server = start(tmp.path());
    server.shutdown();
    assert_eq!(wait_exit(&mut server, Duration::from_secs(10)), Some(0));
}

/// (4e) `exit` without a prior `shutdown` exits with code 1 (LSP spec).
#[test]
fn lsp_exits_one_on_exit_without_shutdown() {
    let tmp = tempfile::tempdir().unwrap();
    let mut server = start(tmp.path());
    server.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
    assert_eq!(wait_exit(&mut server, Duration::from_secs(10)), Some(1));
}
