//! `E001` — an event must declare both a `starts_at` timestamp and a
//! `timezone`.
//!
//! A wall-clock timestamp is meaningless without the zone it is read in,
//! so the two fields are required together: `starts_at` names the moment,
//! `timezone` names the frame. The deeper checks (the timestamp parses,
//! `ends_at` is after `starts_at`, the zone is one we emit a `VTIMEZONE`
//! for) stay in the event loader (`web::events`); this rule guards the
//! authoring contract that both fields are present and non-empty.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct E001EventTimestamp;

impl E001EventTimestamp {
    pub const CODE: &'static str = "E001";
}

impl Rule for E001EventTimestamp {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        "Events must declare both a `starts_at` timestamp and a `timezone`."
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let report = |message: &str| -> Vec<Violation> {
            vec![Violation {
                code: Self::CODE,
                path: file.path.clone(),
                line: 1,
                range: line_byte_range(&file.contents, 1),
                message: message.to_string(),
            }]
        };

        let Some(fm) = frontmatter::extract(&file.contents) else {
            return report(
                "Missing frontmatter (an event must declare `starts_at` and `timezone`)",
            );
        };

        let mut violations = Vec::new();
        match frontmatter::field(fm, "starts_at") {
            None => violations.extend(report(
                "Frontmatter is missing required event field `starts_at`",
            )),
            Some(value) if value.is_empty() => {
                violations.extend(report("Frontmatter `starts_at` is empty"));
            }
            Some(_) => {}
        }
        match frontmatter::field(fm, "timezone") {
            None => {
                violations.extend(report(
                    "Frontmatter is missing required event field `timezone`",
                ));
            }
            Some(value) if value.is_empty() => {
                violations.extend(report("Frontmatter `timezone` is empty"));
            }
            Some(_) => {}
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::E001EventTimestamp;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("web/content/events/20260702_x.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_with_timestamp_and_timezone() {
        let f = file(
            "---\nstarts_at: \"2026-07-02T11:00:00\"\ntimezone: America/Los_Angeles\n---\n\nBody.\n",
        );
        assert!(E001EventTimestamp.lint(&f).is_empty());
    }

    #[test]
    fn flags_missing_timezone() {
        let v = E001EventTimestamp.lint(&file("---\nstarts_at: \"2026-07-02T11:00:00\"\n---\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "E001");
        assert!(v[0].message.contains("timezone"));
    }

    #[test]
    fn flags_missing_timestamp() {
        let v = E001EventTimestamp.lint(&file("---\ntimezone: America/Denver\n---\n"));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("starts_at"));
    }
}
