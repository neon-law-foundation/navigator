//! `navigator-lsp` — Language Server Protocol entry point for
//! Neon Law Navigator's markdown rules. Speaks JSON-RPC over stdio so any
//! LSP-aware editor (Neovim, Helix, VS Code, Zed, Emacs) can attach
//! by setting `cmd = "navigator-lsp"` against `*.md` files.

use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use lsp::Server;
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    HoverParams, InitializeParams,
};
use serde_json::{json, Value};

fn main() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut server = Server::new();
    while let Some(message) = read_message(&mut input)? {
        if let Some(reply) = handle(&mut server, &message)? {
            write_message(&mut output, &reply)?;
        }
    }
    Ok(())
}

fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header).context("read header")?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = header.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(rest.trim().parse().context("parse Content-Length")?);
        }
    }
    let len = content_length.context("missing Content-Length header")?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).context("read body")?;
    let value = serde_json::from_slice(&buf).context("parse JSON-RPC body")?;
    Ok(Some(value))
}

fn write_message<W: Write>(writer: &mut W, value: &Value) -> Result<()> {
    let body = serde_json::to_string(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    writer.flush()?;
    Ok(())
}

fn handle(server: &mut Server, message: &Value) -> Result<Option<Value>> {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = message.get("id").cloned();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "initialize" => {
            let _: InitializeParams =
                serde_json::from_value(params).unwrap_or_else(|_| InitializeParams::default());
            let result = lsp::state::InitializeResult::default_payload();
            Ok(Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            })))
        }
        "initialized" => Ok(None),
        "shutdown" => Ok(Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": null,
        }))),
        "exit" => std::process::exit(0),
        "textDocument/didOpen" => {
            let params: DidOpenTextDocumentParams = serde_json::from_value(params)?;
            let out = server.did_open(params);
            Ok(notification(out))
        }
        "textDocument/didChange" => {
            let params: DidChangeTextDocumentParams = serde_json::from_value(params)?;
            let out = server.did_change(params);
            Ok(notification(out))
        }
        "textDocument/didClose" => {
            let params: DidCloseTextDocumentParams = serde_json::from_value(params)?;
            server.did_close(params);
            Ok(None)
        }
        "textDocument/codeAction" => {
            let params: lsp_types::CodeActionParams = serde_json::from_value(params)?;
            let actions = server.code_actions(&params.text_document.uri, params.range);
            Ok(Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": actions,
            })))
        }
        "textDocument/hover" => {
            let params: HoverParams = serde_json::from_value(params)?;
            let hover = server.hover(
                &params.text_document_position_params.text_document.uri,
                params.text_document_position_params.position,
            );
            Ok(Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": hover,
            })))
        }
        _ => {
            // Unknown method: respond with method-not-found for
            // requests, ignore notifications.
            if id.is_some() {
                Ok(Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": format!("method not found: {method}") },
                })))
            } else {
                Ok(None)
            }
        }
    }
}

fn notification(out: lsp::state::Outgoing) -> Option<Value> {
    match out {
        lsp::state::Outgoing::Notification { method, params } => Some(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        })),
        lsp::state::Outgoing::Response { id, result } => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })),
    }
}
