//! In-memory document cache + JSON-RPC dispatcher. Keeping I/O out
//! of this module so unit tests can drive request/response purely
//! over `Server::handle_message`.

// `lsp_types::Uri` contains an `UnsafeCell` for percent-decode
// memoization; clippy can't see that the cell is never mutated
// through our `HashMap` keys (we own them by value and never
// reach inside). Allow the lint at module scope.
#![allow(clippy::mutable_key_type)]

use std::collections::HashMap;
use std::path::PathBuf;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, Hover, HoverContents, MarkupContent,
    MarkupKind, OneOf, PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri, WorkspaceEdit,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diagnostics::{lint_buffer, violation_to_diagnostic};
use crate::position::range_to_lsp_range;

/// In-memory state of the language server: one entry per open
/// document. The cache mirrors what the editor has on screen —
/// `didChange` notifications replace it; `didClose` evicts it.
#[derive(Debug, Default)]
pub struct Server {
    documents: HashMap<Uri, String>,
}

/// A single outgoing message produced by the server in response to
/// some incoming traffic. Either a JSON-RPC response (paired with
/// an `id`) or a server-originated notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outgoing {
    Response { id: Value, result: Value },
    Notification { method: String, params: Value },
}

impl Server {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Static capability advertisement returned from `initialize`.
    /// Kept as a free function so tests can assert the shape without
    /// constructing a `Server`.
    #[must_use]
    pub fn capabilities() -> ServerCapabilities {
        ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            code_action_provider: Some(lsp_types::CodeActionProviderCapability::Options(
                lsp_types::CodeActionOptions {
                    code_action_kinds: Some(vec![
                        CodeActionKind::QUICKFIX,
                        CodeActionKind::SOURCE_FIX_ALL,
                    ]),
                    resolve_provider: Some(false),
                    work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
                },
            )),
            hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
            ..ServerCapabilities::default()
        }
    }

    /// Open a document and return the diagnostics-publish
    /// notification an LSP server is expected to emit.
    pub fn did_open(&mut self, params: DidOpenTextDocumentParams) -> Outgoing {
        let uri = params.text_document.uri.clone();
        self.documents
            .insert(uri.clone(), params.text_document.text);
        self.publish_diagnostics(&uri)
    }

    /// Replace a document's content (FULL sync) and re-publish.
    pub fn did_change(&mut self, params: DidChangeTextDocumentParams) -> Outgoing {
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.into_iter().next() {
            self.documents.insert(uri.clone(), change.text);
        }
        self.publish_diagnostics(&uri)
    }

    pub fn did_close(&mut self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
    }

    fn publish_diagnostics(&self, uri: &Uri) -> Outgoing {
        let text = self.documents.get(uri).cloned().unwrap_or_default();
        let diagnostics = self.diagnostics_for(uri, &text);
        let params = PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics,
            version: None,
        };
        Outgoing::Notification {
            method: "textDocument/publishDiagnostics".to_string(),
            params: serde_json::to_value(params).expect("serialize publish diagnostics"),
        }
    }

    fn diagnostics_for(&self, uri: &Uri, text: &str) -> Vec<Diagnostic> {
        let path = uri_to_path(uri);
        let (_file, violations) = lint_buffer(path, text.to_string());
        violations
            .iter()
            .map(|v| violation_to_diagnostic(text, v))
            .collect()
    }

    /// Compute every quick-fix code action that intersects the
    /// requested range, plus a single `source.fixAll` action that
    /// applies every safe-by-construction autofix.
    pub fn code_actions(&self, uri: &Uri, range: lsp_types::Range) -> Vec<CodeActionOrCommand> {
        let Some(text) = self.documents.get(uri) else {
            return Vec::new();
        };
        let path = uri_to_path(uri);
        let (file, violations) = lint_buffer(path, text.clone());
        let rule_set: Vec<Box<dyn rules::Rule>> = rules::navigator_classified_rules(&file);
        let mut actions = Vec::new();
        let mut all_edits: Vec<(rules::TextEdit, &'static str)> = Vec::new();
        for v in &violations {
            let rule = rule_set.iter().find(|r| r.code() == v.code);
            let Some(rule) = rule else { continue };
            let Some(edit) = rule.fix(&file, v) else {
                continue;
            };
            let lsp_range = range_to_lsp_range(text, &v.range);
            if intersects(&range, &lsp_range) {
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("{}: fix", v.code),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![violation_to_diagnostic(text, v)]),
                    edit: Some(workspace_edit(uri, text, std::slice::from_ref(&edit))),
                    command: None,
                    is_preferred: Some(true),
                    disabled: None,
                    data: None,
                }));
            }
            all_edits.push((edit, v.code));
        }
        if !all_edits.is_empty() {
            // Sort by start asc + code asc; drop overlaps keeping the
            // lower-coded edit; this mirrors `cli validate --fix`.
            all_edits.sort_by(|a, b| a.0.range.start.cmp(&b.0.range.start).then(a.1.cmp(b.1)));
            let mut kept: Vec<rules::TextEdit> = Vec::with_capacity(all_edits.len());
            for (edit, _) in all_edits {
                if let Some(prev) = kept.last() {
                    if edit.range.start < prev.range.end {
                        continue;
                    }
                }
                kept.push(edit);
            }
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Neon Law Navigator: fix all auto-fixable problems".to_string(),
                kind: Some(CodeActionKind::SOURCE_FIX_ALL),
                diagnostics: None,
                edit: Some(workspace_edit(uri, text, &kept)),
                command: None,
                is_preferred: None,
                disabled: None,
                data: None,
            }));
        }
        actions
    }

    /// Hover at `position`: if any violation's range covers that
    /// byte offset, return the rule description + the violation
    /// message.
    pub fn hover(&self, uri: &Uri, position: lsp_types::Position) -> Option<Hover> {
        let text = self.documents.get(uri)?;
        let path = uri_to_path(uri);
        let (_file, violations) = lint_buffer(path, text.clone());
        let byte = position_to_byte(text, position)?;
        let v = violations
            .iter()
            .find(|v| v.range.start <= byte && byte <= v.range.end)?;
        let body = format!(
            "**{}** — {}\n\n{}",
            v.code,
            rules::description_for_code(v.code),
            v.message,
        );
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: body,
            }),
            range: Some(range_to_lsp_range(text, &v.range)),
        })
    }
}

fn intersects(a: &lsp_types::Range, b: &lsp_types::Range) -> bool {
    !(b.end < a.start || a.end < b.start)
}

/// Best-effort `file:///…` URI → local path. Falls back to a
/// placeholder so the rules engine has something to record in
/// `Violation.path`.
fn uri_to_path(uri: &Uri) -> PathBuf {
    let s = uri.as_str();
    if let Some(rest) = s.strip_prefix("file://") {
        let decoded = percent_decode(rest);
        return PathBuf::from(decoded);
    }
    PathBuf::from(s)
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                #[allow(clippy::cast_possible_truncation)]
                out.push(char::from((h * 16 + l) as u8));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn workspace_edit(uri: &Uri, text: &str, edits: &[rules::TextEdit]) -> WorkspaceEdit {
    let lsp_edits: Vec<OneOf<lsp_types::TextEdit, lsp_types::AnnotatedTextEdit>> = edits
        .iter()
        .map(|e| {
            OneOf::Left(lsp_types::TextEdit {
                range: range_to_lsp_range(text, &e.range),
                new_text: e.new_text.clone(),
            })
        })
        .collect();
    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        lsp_edits
            .into_iter()
            .map(|e| match e {
                OneOf::Left(te) => te,
                OneOf::Right(_) => unreachable!("only constructed Left variants"),
            })
            .collect(),
    );
    WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    }
}

fn position_to_byte(text: &str, position: lsp_types::Position) -> Option<usize> {
    let mut line: u32 = 0;
    let mut line_start = 0usize;
    for (i, ch) in text.char_indices() {
        if line == position.line {
            line_start = i;
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = i + ch.len_utf8();
        }
    }
    if line != position.line && !text.is_empty() {
        return None;
    }
    let mut character: u32 = 0;
    let mut byte = line_start;
    while byte < text.len() && character < position.character {
        let ch = text[byte..].chars().next()?;
        character += u32::try_from(ch.len_utf16()).ok()?;
        byte += ch.len_utf8();
        if ch == '\n' {
            break;
        }
    }
    Some(byte)
}

/// Shape of an `initialize` response payload — exposed so tests can
/// snapshot it without a real connection.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub capabilities: ServerCapabilities,
    pub server_info: Option<ServerInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: Option<String>,
}

impl InitializeResult {
    #[must_use]
    pub fn default_payload() -> Self {
        Self {
            capabilities: Server::capabilities(),
            server_info: Some(ServerInfo {
                name: "navigator-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InitializeResult, Outgoing, Server};
    use lsp_types::{
        DidOpenTextDocumentParams, Position, Range, TextDocumentItem, Uri,
        VersionedTextDocumentIdentifier,
    };
    use std::str::FromStr;

    fn open(server: &mut Server, uri: &str, text: &str) {
        let uri = Uri::from_str(uri).unwrap();
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "markdown".to_string(),
                version: 0,
                text: text.to_string(),
            },
        };
        let out = server.did_open(params);
        if let Outgoing::Notification { method, .. } = &out {
            assert_eq!(method, "textDocument/publishDiagnostics");
        } else {
            panic!("expected notification, got {out:?}");
        }
    }

    #[test]
    fn capabilities_advertise_full_sync_quickfix_and_hover() {
        let caps = Server::capabilities();
        assert!(matches!(
            caps.text_document_sync,
            Some(lsp_types::TextDocumentSyncCapability::Kind(
                lsp_types::TextDocumentSyncKind::FULL,
            ))
        ));
        match caps.code_action_provider {
            Some(lsp_types::CodeActionProviderCapability::Options(ref o)) => {
                let kinds = o.code_action_kinds.as_ref().unwrap();
                assert!(kinds.contains(&lsp_types::CodeActionKind::QUICKFIX));
                assert!(kinds.contains(&lsp_types::CodeActionKind::SOURCE_FIX_ALL));
            }
            _ => panic!("expected code-action options"),
        }
        assert!(matches!(
            caps.hover_provider,
            Some(lsp_types::HoverProviderCapability::Simple(true))
        ));
    }

    #[test]
    fn initialize_result_default_names_the_server() {
        let r = InitializeResult::default_payload();
        let info = r.server_info.unwrap();
        assert_eq!(info.name, "navigator-lsp");
        assert!(info.version.is_some());
    }

    #[test]
    fn did_open_emits_diagnostics_for_a_buffer_with_a_violation() {
        let mut server = Server::new();
        let uri = Uri::from_str("file:///t.md").unwrap();
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "markdown".to_string(),
                version: 0,
                text: "ok\n\thard tab\n".to_string(),
            },
        };
        let out = server.did_open(params);
        let Outgoing::Notification { method, params } = out else {
            panic!("expected notification")
        };
        assert_eq!(method, "textDocument/publishDiagnostics");
        let v: lsp_types::PublishDiagnosticsParams = serde_json::from_value(params).unwrap();
        assert_eq!(v.uri, uri);
        assert!(v.diagnostics.iter().any(|d| match &d.code {
            Some(lsp_types::NumberOrString::String(s)) => s == "M010",
            _ => false,
        }));
    }

    #[test]
    fn did_change_replaces_buffer_and_clears_fixed_violations() {
        let mut server = Server::new();
        open(&mut server, "file:///t.md", "ok\n\thard tab\n");
        let uri = Uri::from_str("file:///t.md").unwrap();
        let out = server.did_change(lsp_types::DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 1,
            },
            content_changes: vec![lsp_types::TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "ok\n  hard tab\n".to_string(),
            }],
        });
        let Outgoing::Notification { params, .. } = out else {
            panic!("expected notification")
        };
        let v: lsp_types::PublishDiagnosticsParams = serde_json::from_value(params).unwrap();
        assert!(
            !v.diagnostics.iter().any(|d| matches!(
                &d.code,
                Some(lsp_types::NumberOrString::String(s)) if s == "M010"
            )),
            "M010 should be gone after the fix",
        );
    }

    #[test]
    fn code_actions_include_quickfix_and_source_fix_all() {
        let mut server = Server::new();
        open(&mut server, "file:///t.md", "ok\n\thard tab\n");
        let uri = Uri::from_str("file:///t.md").unwrap();
        let actions = server.code_actions(
            &uri,
            Range {
                start: Position {
                    line: 1,
                    character: 0,
                },
                end: Position {
                    line: 1,
                    character: 10,
                },
            },
        );
        let kinds: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                lsp_types::CodeActionOrCommand::CodeAction(ca) => ca.kind.clone(),
                lsp_types::CodeActionOrCommand::Command(_) => None,
            })
            .collect();
        assert!(kinds.contains(&lsp_types::CodeActionKind::QUICKFIX));
        assert!(kinds.contains(&lsp_types::CodeActionKind::SOURCE_FIX_ALL));
    }

    #[test]
    fn hover_returns_rule_description_when_over_a_violation() {
        let mut server = Server::new();
        open(&mut server, "file:///t.md", "ok\n\thard tab\n");
        let uri = Uri::from_str("file:///t.md").unwrap();
        // Line 2 (0-indexed: 1), character 0 — the tab.
        let h = server
            .hover(
                &uri,
                Position {
                    line: 1,
                    character: 0,
                },
            )
            .expect("expected hover");
        match h.contents {
            lsp_types::HoverContents::Markup(m) => {
                assert!(m.value.contains("M010"), "got: {}", m.value);
                assert!(
                    m.value.contains("Hard tabs"),
                    "should include rule description, got: {}",
                    m.value
                );
            }
            _ => panic!("expected markup hover"),
        }
    }

    #[test]
    fn hover_returns_none_outside_any_violation() {
        let mut server = Server::new();
        open(&mut server, "file:///t.md", "ok\n  no tab\n");
        let uri = Uri::from_str("file:///t.md").unwrap();
        assert!(server
            .hover(
                &uri,
                Position {
                    line: 0,
                    character: 1
                },
            )
            .is_none());
        let _ = uri;
    }
}
