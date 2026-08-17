//! End-to-end integration: spawn the `navigator-lsp` binary,
//! exchange Content-Length-framed JSON-RPC over its stdio, and
//! assert it speaks the protocol correctly. This is the "does
//! Neovim/VS Code see what we expect?" check.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

fn binary_path() -> PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop(); // tests/
    if p.ends_with("deps") {
        p.pop();
    }
    p.push("navigator-lsp");
    p
}

struct LspChild {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl LspChild {
    fn spawn() -> Self {
        let mut child = Command::new(binary_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn navigator-lsp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, value: &Value) {
        let body = serde_json::to_string(value).unwrap();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut content_length: Option<usize> = None;
        loop {
            let mut header = String::new();
            self.stdout.read_line(&mut header).unwrap();
            let trimmed = header.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                content_length = Some(rest.trim().parse().unwrap());
            }
        }
        let len = content_length.expect("missing Content-Length");
        let mut buf = vec![0u8; len];
        self.stdout.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    fn shutdown(mut self) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "shutdown",
        }));
        let _ = self.recv();
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "exit",
        }));
        let _ = self.child.wait();
    }
}

#[test]
fn initialize_responds_with_capabilities() {
    let mut server = LspChild::spawn();
    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "capabilities": {} },
    }));
    let response = server.recv();
    assert_eq!(response["id"], 1);
    let caps = &response["result"]["capabilities"];
    assert!(caps["hoverProvider"].as_bool().unwrap_or(false));
    assert!(caps["codeActionProvider"].is_object());
    let info = &response["result"]["serverInfo"];
    assert_eq!(info["name"], "navigator-lsp");
    server.shutdown();
}

#[test]
fn did_open_publishes_diagnostics_for_hard_tab() {
    let mut server = LspChild::spawn();
    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "capabilities": {} },
    }));
    let _init = server.recv();
    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {},
    }));
    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///t.md",
                "languageId": "markdown",
                "version": 1,
                "text": "ok\n\thard tab\n",
            }
        },
    }));
    let notif = server.recv();
    assert_eq!(notif["method"], "textDocument/publishDiagnostics");
    let codes: Vec<String> = notif["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["code"].as_str().map(str::to_string))
        .collect();
    assert!(
        codes.iter().any(|c| c == "M010"),
        "expected M010 in diagnostics, got: {codes:?}"
    );
    server.shutdown();
}

#[test]
fn code_action_request_returns_source_fix_all() {
    let mut server = LspChild::spawn();
    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "capabilities": {} },
    }));
    let _init = server.recv();
    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///t.md",
                "languageId": "markdown",
                "version": 1,
                "text": "ok\n\thard tab\n",
            }
        },
    }));
    let _diag = server.recv();
    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": "file:///t.md" },
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 1 }
            },
            "context": { "diagnostics": [] }
        }
    }));
    let response = server.recv();
    assert_eq!(response["id"], 2);
    let kinds: Vec<String> = response["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["kind"].as_str().map(str::to_string))
        .collect();
    assert!(
        kinds.iter().any(|k| k == "source.fixAll"),
        "expected source.fixAll, got: {kinds:?}"
    );
    server.shutdown();
}
