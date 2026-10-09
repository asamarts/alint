//! `alint lsp` regression tests for the 2026-10 audit findings, each
//! driving the real binary over stdio:
//!   1. diagnostic columns are UTF-16 code units (the LSP default),
//!   2. a config file's own findings survive a full check (not wiped by
//!      the "clear the old config error" publish),
//!   3. a full check overlays unsaved buffers instead of republishing
//!      stale on-disk results,
//!   4. per-document config discovery (nearest `.alint.yml`), a broken
//!      config disables the stale engine, `baseline:` is honored,
//!      multi-root workspaces, and `exit` terminates the process.
//!
//! Unix-only for the same `file://` reason as the shared harness.
#![cfg(unix)]

mod lsp_common;

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use lsp_common::{Server, recv_response, spawn_server, wait_for_diagnostics};

/// Read inbound messages until none arrives for `quiet`, returning the
/// LAST `publishDiagnostics` per URI — the state the editor ends up
/// showing.
fn settle(rx: &Receiver<Value>, quiet: Duration) -> HashMap<String, Vec<Value>> {
    let mut last = HashMap::new();
    while let Ok(msg) = rx.recv_timeout(quiet) {
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

fn shutdown(server: &mut Server) {
    server.send(&json!({ "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null }));
    recv_response(&server.rx, 99);
    server.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
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
    shutdown(&mut server);
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
    let last = settle(&server.rx, QUIET);
    let diags = last.get(&cfg_uri).expect("config file got diagnostics");
    assert_eq!(
        codes(diags),
        vec!["need-license"],
        "the anchored finding must be the config file's final state: {last:?}"
    );
    shutdown(&mut server);
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
    let last = settle(&server.rx, QUIET);
    let final_a = last.get(&a).expect("the full check republishes a.txt");
    assert_eq!(
        codes(final_a),
        vec!["no-todo"],
        "a full check must not paint the clean on-disk a.txt over its unsaved buffer"
    );
    shutdown(&mut server);
}

/// (4a) Per-document discovery: a file under `pkg/` is linted by
/// `pkg/.alint.yml` (the nearest config), not the workspace root's.
#[test]
fn lsp_uses_nearest_config_per_document() {
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

    let mut server = start(root);
    open(&mut server, &file_uri, text);
    let diags = wait_for_diagnostics(&server.rx, &file_uri);
    assert_eq!(
        codes(&diags),
        vec!["pkg-no-fixme"],
        "pkg/a.txt is governed by pkg/.alint.yml: {diags:?}"
    );
    shutdown(&mut server);
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
    let after_break = settle(&server.rx, QUIET);
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
    let after_edit = settle(&server.rx, QUIET);
    if let Some(diags) = after_edit.get(&a) {
        assert!(
            diags.is_empty(),
            "no diagnostics from the stale engine: {diags:?}"
        );
    }
    shutdown(&mut server);
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
    shutdown(&mut server);
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
    let last = settle(&server.rx, QUIET);
    assert_eq!(
        last.get(&b).map(|d| codes(d)),
        Some(vec!["no-todo".to_string()]),
        "{last:?}"
    );
    shutdown(&mut server);
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
    shutdown(&mut server);
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
