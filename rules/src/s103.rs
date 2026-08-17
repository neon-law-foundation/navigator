//! `S103` — a declared `kind:` frontmatter key must name a recognized
//! document kind.
//!
//! `kind:` is the single, cross-cutting discriminator that names what a
//! file is (see [`crate::kind`]). It is optional today — a file without
//! one is classified by structural inference — but when present its value
//! must be one of the known kinds. An unrecognized value is a blocking
//! error: the file would silently fall back to inference and misclassify.
//!
//! S-family rules run on **every** Neon Law Navigator markdown file
//! regardless of its kind, so this check fires on a notation template, an
//! event, a blog post, or plain prose alike.

use crate::kind::{self, Kind};
use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

/// `S103` — `kind:` must be a recognized value when declared.
pub struct S103KindEnum;

impl S103KindEnum {
    pub const CODE: &'static str = "S103";
}

impl Rule for S103KindEnum {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        crate::description_for_code(Self::CODE)
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let Some(fm) = frontmatter::extract(&file.contents) else {
            return Vec::new();
        };
        // Only fires when `kind:` is present. An absent `kind` is fine —
        // the classifier falls back to structural inference. `field`
        // returns `None` both when the key is absent AND when its value is
        // a non-scalar (a sequence/mapping), so distinguish the two: a
        // present-but-non-scalar `kind` is a mistake we must flag, not
        // silently ignore.
        let raw = frontmatter::field(fm, "kind");
        if raw.as_deref().is_some_and(|v| Kind::parse(v).is_some()) {
            return Vec::new();
        }
        if raw.is_none() && !frontmatter_has_key(fm, "kind") {
            return Vec::new();
        }
        let line = kind_line(&file.contents);
        let message = match raw.as_deref() {
            None => format!(
                "Frontmatter `kind:` must be one of: {} (it is not a scalar value)",
                kind::VALID.join(", ")
            ),
            Some("") => format!(
                "Frontmatter `kind:` is empty (expected one of: {})",
                kind::VALID.join(", ")
            ),
            Some(v) => format!(
                "Invalid `kind:` value `{v}` (expected one of: {})",
                kind::VALID.join(", ")
            ),
        };
        vec![Violation {
            code: Self::CODE,
            path: file.path.clone(),
            line,
            range: line_byte_range(&file.contents, line),
            message,
        }]
    }
}

/// Whether the leading frontmatter declares `key` as a top-level mapping
/// key — used to tell "key absent" from "key present but non-scalar",
/// which `frontmatter::field` collapses to the same `None`.
fn frontmatter_has_key(fm: &str, key: &str) -> bool {
    serde_yaml::from_str::<serde_yaml::Value>(fm)
        .ok()
        .and_then(|v| {
            v.as_mapping()
                .map(|m| m.contains_key(serde_yaml::Value::String(key.to_string())))
        })
        .unwrap_or(false)
}

/// The 1-based line of the top-level `kind:` key inside the leading
/// frontmatter, so the diagnostic underlines the offending line rather than
/// the file's first line or a nested `kind:` child. Falls back to line 1 if
/// it can't be located.
fn kind_line(contents: &str) -> usize {
    contents
        .lines()
        .enumerate()
        .take_while(|(idx, line)| *idx == 0 || *line != "---")
        .find(|(_, line)| !line.starts_with([' ', '\t']) && line.starts_with("kind:"))
        .map_or(1, |(idx, _)| idx + 1)
}

#[cfg(test)]
mod tests {
    use super::S103KindEnum;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("test.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn absent_kind_is_not_a_violation() {
        assert!(S103KindEnum
            .lint(&file("---\ntitle: T\n---\n\nBody.\n"))
            .is_empty());
        assert!(S103KindEnum
            .lint(&file("plain prose, no frontmatter"))
            .is_empty());
    }

    #[test]
    fn every_recognized_kind_passes() {
        for value in crate::kind::VALID {
            let body = format!("---\ntitle: T\nkind: {value}\n---\n");
            assert!(
                S103KindEnum.lint(&file(&body)).is_empty(),
                "kind `{value}` should pass",
            );
        }
    }

    #[test]
    fn unknown_kind_is_flagged() {
        let v = S103KindEnum.lint(&file("---\ntitle: T\nkind: bogus\n---\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "S103");
        assert!(v[0].message.contains("Invalid `kind:` value `bogus`"));
        // Underlines the `kind:` line (line 3), not the file's first line.
        assert_eq!(v[0].line, 3);
    }

    #[test]
    fn empty_kind_is_flagged() {
        let v = S103KindEnum.lint(&file("---\nkind:\n---\n"));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("`kind:` is empty"));
    }

    #[test]
    fn non_scalar_kind_is_flagged_not_silently_ignored() {
        // A sequence value is a genuine authoring mistake; `frontmatter::field`
        // returns None for it, but the key IS present, so S103 must fire.
        let v = S103KindEnum.lint(&file("---\ntitle: T\nkind:\n  - retainer\n---\n"));
        assert_eq!(v.len(), 1, "got {v:?}");
        assert!(
            v[0].message.contains("not a scalar"),
            "got {}",
            v[0].message
        );
    }

    #[test]
    fn a_nested_kind_child_does_not_trip_the_rule_or_misplace_the_line() {
        // Only the top-level `kind:` is validated; a nested `kind:` child is
        // ignored, and the diagnostic underlines the top-level line (4).
        let v = S103KindEnum.lint(&file("---\nmeta:\n  kind: whatever\nkind: bogus\n---\n"));
        assert_eq!(v.len(), 1, "got {v:?}");
        assert_eq!(v[0].line, 4, "underlines the top-level kind line");
    }
}
