//! `N109` — a notation template's optional `output:` frontmatter field,
//! when present, must name a known render output format.
//!
//! The field is **optional**: a template that declares no `output:`
//! renders as a plain document, so its absence is not a violation. When
//! a template *does* declare one, a typo (`output: leter`) would
//! silently fall back to plain at render time — this rule turns that
//! into a loud validation failure instead.
//!
//! [`VALID`] must stay in step with the renderer's accepted set,
//! `pdf::OutputFormat::FRONTMATTER_VALUES`. (`rules` does not depend on
//! `pdf`, so the list is duplicated deliberately and kept small.)

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct F109OutputFormat;

impl F109OutputFormat {
    pub const CODE: &'static str = "N109";
    /// Output formats a template may declare. Mirrors
    /// `pdf::OutputFormat::FRONTMATTER_VALUES`; `plain` is the implicit
    /// default and is intentionally not declarable.
    pub const VALID: &'static [&'static str] = &["letter"];
}

impl Rule for F109OutputFormat {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let Some(fm) = frontmatter::extract(&file.contents) else {
            // Absent frontmatter is N101/N102/N105's concern, not ours.
            return Vec::new();
        };
        // The field is optional — only validate when it is present.
        let Some(value) = frontmatter::field(fm, "output") else {
            return Vec::new();
        };
        if Self::VALID.contains(&value.as_str()) {
            return Vec::new();
        }
        let message = if value.is_empty() {
            format!(
                "Frontmatter `output:` is empty (expected one of: {})",
                Self::VALID.join(", ")
            )
        } else {
            format!(
                "Invalid `output:` value `{value}` (expected one of: {})",
                Self::VALID.join(", ")
            )
        };
        vec![Violation {
            code: Self::CODE,
            path: file.path.clone(),
            line: 1,
            range: line_byte_range(&file.contents, 1),
            message,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::F109OutputFormat;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("trademark_coexistence.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_when_output_is_absent() {
        // The field is optional; no declaration means plain rendering.
        let f = file("---\ntitle: T\nconfidential: true\n---\n");
        assert!(F109OutputFormat.lint(&f).is_empty());
    }

    #[test]
    fn passes_for_a_known_format() {
        let f = file("---\ntitle: T\noutput: letter\n---\n");
        assert!(F109OutputFormat.lint(&f).is_empty());
    }

    #[test]
    fn flags_an_unknown_format() {
        let f = file("---\noutput: demand_letter\n---\n");
        let v = F109OutputFormat.lint(&f);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "N109");
        assert!(v[0].message.contains("demand_letter"));
        assert!(v[0].message.contains("letter"));
    }

    #[test]
    fn flags_an_empty_value() {
        let f = file("---\noutput:\n---\n");
        let v = F109OutputFormat.lint(&f);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("empty"));
    }

    #[test]
    fn passes_with_no_frontmatter_at_all() {
        // Frontmatter presence is enforced by other rules; N109 is silent.
        assert!(F109OutputFormat.lint(&file("Just prose.")).is_empty());
    }
}
