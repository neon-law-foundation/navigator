//! Run the Navigator rule set over a buffer and produce LSP
//! `Diagnostic`s. The default rule set matches `cli validate
//! --markdown-only` so a notation that's clean in the CLI is clean
//! in the LSP and vice versa.

use std::path::PathBuf;

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};
use rules::{description_for_code, Rule, SourceFile, Violation};

use crate::position::range_to_lsp_range;

/// Lint `text` with the markdown-only rule set and return both the
/// raw violations (so callers can wire `fix()` later) and the LSP
/// diagnostic projection.
#[must_use]
pub fn lint_buffer(path: PathBuf, text: String) -> (SourceFile, Vec<Violation>) {
    let file = SourceFile {
        path,
        contents: text,
    };
    let rule_set: Vec<Box<dyn Rule>> = rules::navigator_markdown_only_rules();
    let mut violations = Vec::new();
    for rule in &rule_set {
        violations.extend(rule.lint(&file));
    }
    (file, violations)
}

/// Project a single `Violation` onto the LSP diagnostic shape.
/// `text` is the source the violation was computed against and is
/// used to map byte offsets to UTF-16 positions.
#[must_use]
pub fn violation_to_diagnostic(text: &str, v: &Violation) -> Diagnostic {
    Diagnostic {
        range: range_to_lsp_range(text, &v.range),
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String(v.code.to_string())),
        code_description: None,
        source: Some("navigator".to_string()),
        message: format!("{} — {}", description_for_code(v.code), v.message),
        related_information: None,
        tags: None,
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{lint_buffer, violation_to_diagnostic};
    use lsp_types::{DiagnosticSeverity, NumberOrString};

    #[test]
    fn lint_buffer_flags_a_hard_tab() {
        let (_file, violations) = lint_buffer(
            std::path::PathBuf::from("t.md"),
            "ok\n\thard tab\n".to_string(),
        );
        assert!(
            violations.iter().any(|v| v.code == "M010"),
            "expected M010 violation, got: {:?}",
            violations.iter().map(|v| v.code).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn violation_to_diagnostic_carries_code_and_warning_severity() {
        let text = "ok\n\thard tab\n";
        let (_file, violations) = lint_buffer(std::path::PathBuf::from("t.md"), text.to_string());
        let m010 = violations.iter().find(|v| v.code == "M010").unwrap();
        let diag = violation_to_diagnostic(text, m010);
        assert_eq!(diag.severity, Some(DiagnosticSeverity::WARNING));
        assert!(matches!(diag.code, Some(NumberOrString::String(ref s)) if s == "M010"));
        assert_eq!(diag.source.as_deref(), Some("navigator"));
        // Line 2 (1-based in source → 0-based in LSP).
        assert_eq!(diag.range.start.line, 1);
    }
}
