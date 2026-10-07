//! Shared LSP test harness: spawns the real `alint lsp` binary and
//! speaks framed JSON-RPC over stdio. Used by `lsp_smoke.rs` (the full
//! linear e2e script) and `lsp_protocol.rs` (focused contract tests).
//!
//! Unix-only: callers build `file://` URIs by hand, which would need
//! drive-letter handling on Windows. The server itself is
//! cross-platform; this is a test-harness simplification.

use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::Value;

pub const RECV_TIMEOUT: Duration = Duration::from_secs(20);

/// Frame a JSON-RPC message with its `Content-Length` header.
pub fn frame(msg: &Value) -> Vec<u8> {
    let body = msg.to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

/// Spawn a reader thread that parses framed LSP messages off `stdout`
/// and forwards each as a JSON value.
fn spawn_reader(stdout: std::process::ChildStdout) -> Receiver<Value> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut content_len = 0usize;
            loop {
                let mut line = Vec::new();
                let mut byte = [0u8; 1];
                loop {
                    match reader.read_exact(&mut byte) {
                        Ok(()) => {
                            line.push(byte[0]);
                            if byte[0] == b'\n' {
                                break;
                            }
                        }
                        Err(_) => return,
                    }
                }
                let text = String::from_utf8_lossy(&line);
                let trimmed = text.trim_end();
                if trimmed.is_empty() {
                    break;
                }
                if let Some(n) = trimmed.strip_prefix("Content-Length:") {
                    content_len = n.trim().parse().unwrap_or(0);
                }
            }
            if content_len == 0 {
                continue;
            }
            let mut buf = vec![0u8; content_len];
            if reader.read_exact(&mut buf).is_err() {
                return;
            }
            if let Ok(value) = serde_json::from_slice::<Value>(&buf)
                && tx.send(value).is_err()
            {
                return;
            }
        }
    });
    rx
}

/// A spawned `alint lsp` process plus a channel of parsed inbound
/// messages. Dropping it kills and reaps the child.
pub struct Server {
    pub child: Child,
    pub stdin: std::process::ChildStdin,
    pub rx: Receiver<Value>,
}

impl Server {
    fn from_child(mut child: Child) -> Server {
        let stdin = child.stdin.take().unwrap();
        let rx = spawn_reader(child.stdout.take().unwrap());
        Server { child, stdin, rx }
    }

    /// Write a framed JSON-RPC message to the server's stdin.
    pub fn send(&mut self, msg: &Value) {
        self.stdin.write_all(&frame(msg)).unwrap();
        self.stdin.flush().unwrap();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Spawn `target/release|debug` `alint lsp` with `root` as the working
/// directory, stdin/stdout piped and stderr discarded.
pub fn spawn_server(root: &Path) -> Server {
    let child = Command::new(env!("CARGO_BIN_EXE_alint"))
        .arg("lsp")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn alint lsp");
    Server::from_child(child)
}

/// Block until the response to request `id` arrives, returning its
/// `result`.
pub fn recv_response(rx: &Receiver<Value>, id: i64) -> Value {
    let deadline = Instant::now() + RECV_TIMEOUT;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for response");
        let msg = rx
            .recv_timeout(remaining)
            .expect("timed out waiting for response");
        if msg["id"] == id {
            return msg["result"].clone();
        }
    }
}

/// Block until a `textDocument/publishDiagnostics` notification for
/// `uri` arrives, returning its `diagnostics` array.
pub fn wait_for_diagnostics(rx: &Receiver<Value>, uri: &str) -> Vec<Value> {
    let deadline = Instant::now() + RECV_TIMEOUT;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for publishDiagnostics");
        let msg = rx
            .recv_timeout(remaining)
            .expect("timed out waiting for publishDiagnostics");
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == uri {
            return msg["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
    }
}
