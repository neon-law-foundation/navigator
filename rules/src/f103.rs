#![allow(clippy::doc_markdown)]
//! `F103` — markdown filename basename must be snake_case.
//!
//! Templates under `templates/` catalogue domain documents (an
//! LLC formation, a 501(c)(3) formation, a Nevada annual report).
//! Each template's frontmatter `code` is already snake_case with a
//! `__` category separator (`onboarding__retainer_nest`,
//! `nonprofit__nevada_501c3_formation`). Naming the file in
//! snake_case too lets a directory listing read like the codes it
//! seeds — `retainer_nest.md`, `nevada_501c3_formation.md`,
//! `form990_annual_report.md` — so the file on disk and the row it
//! becomes in `templates` share one spelling.
//!
//! A valid basename is lower-case ASCII letters and digits joined by
//! single `_` separators: no leading, trailing, or doubled `_`, and
//! no PascalCase, camelCase, kebab-case, or spaces. A digit run sits
//! *inside* a token rather than getting its own separator — `form990`
//! and `501c3` are each one token — so the names that read best
//! (`form990_annual_report`, `nevada_501c3_formation`) are accepted as
//! authored. The suggestion offered on a violation is a best-effort
//! lower-snake rewrite, not necessarily the canonical authored name.
//!
//! Note: this convention is for `.md` content files. It is the
//! mirror image of the `PascalCase` convention that still governs
//! Restate *workflow* service names (see `is_pascal_case` below,
//! which the `workflows-service` registry test reuses) — the two are
//! deliberately separate conventions for separate things.
//!
//! Examples:
//!
//! - `my_document.md`       accepted (snake_case)
//! - `form990_annual_report.md` accepted (digit run glued to its token)
//! - `MyDocument.md`        rejected (PascalCase)
//! - `myDocument.md`        rejected (camelCase)
//! - `my-document.md`       rejected (hyphens)
//! - `my__document.md`      rejected (doubled underscore)

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct F103SnakeCaseFilename;

impl F103SnakeCaseFilename {
    pub const CODE: &'static str = "F103";
}

impl Rule for F103SnakeCaseFilename {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let Some(stem) = file.path.file_stem().and_then(|s| s.to_str()) else {
            return Vec::new();
        };
        if is_snake_case(stem) {
            return Vec::new();
        }
        let suggested = to_snake_case(stem);
        vec![Violation {
            code: Self::CODE,
            path: file.path.clone(),
            line: 1,
            range: line_byte_range(&file.contents, 1),
            message: format!("Filename `{stem}` is not snake_case. Rename to `{suggested}.md`."),
        }]
    }
}

/// True when `name` is a well-formed snake_case basename: non-empty,
/// only lower-case ASCII letters / digits / `_`, with no leading,
/// trailing, or doubled underscore. Digits may appear anywhere inside
/// a token (`form990`, `501c3`), so a digit run never needs its own
/// separator.
#[must_use]
pub fn is_snake_case(name: &str) -> bool {
    if name.is_empty() || name.starts_with('_') || name.ends_with('_') || name.contains("__") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Best-effort lower-snake rewrite for the violation message — splits
/// camel/Pascal boundaries (a lower-or-digit followed by an uppercase),
/// folds `-`/space/`_` runs to a single `_`, and lower-cases. Used only
/// to suggest a fix; the authored filename is chosen to read best.
fn to_snake_case(name: &str) -> String {
    let mut out = String::new();
    let mut prev_alnum = false;
    for c in name.chars() {
        if c == '_' || c == '-' || c == ' ' {
            if !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            prev_alnum = false;
        } else if c.is_ascii_uppercase() {
            if prev_alnum {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_alnum = false;
        } else {
            out.push(c);
            prev_alnum = c.is_ascii_lowercase() || c.is_ascii_digit();
        }
    }
    out.trim_matches('_').to_string()
}

/// True when `name` starts with an uppercase ASCII letter, contains
/// no separators (`_`, `-`, space), and is otherwise alphanumeric
/// ASCII.
///
/// Public not for `F103` — which now enforces snake_case on template
/// filenames — but for the `workflows-service` registry test, which
/// reuses this exact predicate to assert every registered Restate
/// *workflow* name is PascalCase. Template filenames and workflow
/// service names are intentionally separate conventions.
#[must_use]
pub fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    name.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::{is_pascal_case, is_snake_case, to_snake_case, F103SnakeCaseFilename};
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file_named(name: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from(name),
            contents: "---\ntitle: x\n---\n".to_string(),
        }
    }

    #[test]
    fn accepts_canonical_snake_case_filenames() {
        assert!(F103SnakeCaseFilename
            .lint(&file_named("retainer.md"))
            .is_empty());
        assert!(F103SnakeCaseFilename
            .lint(&file_named("my_document.md"))
            .is_empty());
        assert!(F103SnakeCaseFilename
            .lint(&file_named(
                "nevada_charitable_solicitation_registration.md"
            ))
            .is_empty());
    }

    #[test]
    fn accepts_digit_runs_glued_to_their_token() {
        // The two edge cases from the rename: a digit run abutting a
        // word stays in one token rather than getting its own `_`.
        assert!(F103SnakeCaseFilename
            .lint(&file_named("form990_annual_report.md"))
            .is_empty());
        assert!(F103SnakeCaseFilename
            .lint(&file_named("nevada_501c3_formation.md"))
            .is_empty());
    }

    #[test]
    fn flags_pascal_case_filename_and_suggests_snake() {
        let v = F103SnakeCaseFilename.lint(&file_named("MyDocument.md"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "F103");
        assert!(v[0].message.contains("my_document"));
    }

    #[test]
    fn flags_camel_case_filename() {
        let v = F103SnakeCaseFilename.lint(&file_named("myDocument.md"));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("my_document"));
    }

    #[test]
    fn flags_hyphen_separated_filename() {
        let v = F103SnakeCaseFilename.lint(&file_named("my-document.md"));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("my_document"));
    }

    #[test]
    fn flags_doubled_and_edge_underscores() {
        assert_eq!(
            F103SnakeCaseFilename
                .lint(&file_named("my__document.md"))
                .len(),
            1
        );
        assert_eq!(
            F103SnakeCaseFilename.lint(&file_named("_leading.md")).len(),
            1
        );
        assert_eq!(
            F103SnakeCaseFilename
                .lint(&file_named("trailing_.md"))
                .len(),
            1
        );
    }

    #[test]
    fn is_snake_case_predicate_matches_expected_inputs() {
        assert!(is_snake_case("foo"));
        assert!(is_snake_case("foo_bar"));
        assert!(is_snake_case("form990_annual_report"));
        assert!(is_snake_case("nevada_501c3_formation"));
        assert!(!is_snake_case(""));
        assert!(!is_snake_case("Foo"));
        assert!(!is_snake_case("fooBar"));
        assert!(!is_snake_case("foo-bar"));
        assert!(!is_snake_case("foo bar"));
        assert!(!is_snake_case("foo__bar"));
        assert!(!is_snake_case("_foo"));
        assert!(!is_snake_case("foo_"));
    }

    #[test]
    fn to_snake_case_handles_each_form() {
        assert_eq!(to_snake_case("MyDocument"), "my_document");
        assert_eq!(to_snake_case("myDocument"), "my_document");
        assert_eq!(to_snake_case("my-document"), "my_document");
        assert_eq!(to_snake_case("my document"), "my_document");
        assert_eq!(to_snake_case("FcraDispute"), "fcra_dispute");
        assert_eq!(
            to_snake_case("Form990AnnualReport"),
            "form990_annual_report"
        );
    }

    /// `is_pascal_case` stays public for the `workflows-service`
    /// registry test (Restate workflow names remain PascalCase even
    /// though template filenames are now snake_case).
    #[test]
    fn is_pascal_case_predicate_still_matches_expected_inputs() {
        assert!(is_pascal_case("Archives"));
        assert!(is_pascal_case("BillingCanary"));
        assert!(is_pascal_case("Foo123"));
        assert!(!is_pascal_case(""));
        assert!(!is_pascal_case("notation"));
        assert!(!is_pascal_case("foo_bar"));
    }
}
