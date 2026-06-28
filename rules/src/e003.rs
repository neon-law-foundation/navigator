//! `E003` — an event must say where to show up: a physical
//! `location_address` or an online `meeting_url` (or both, for a hybrid
//! event).
//!
//! In-person and online are not mutually exclusive — a hybrid event can
//! carry a venue address *and* a join link — so the rule requires *at
//! least one* of the two, not exactly one. A blank value counts as
//! absent.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct E003EventLocationOrMeeting;

impl E003EventLocationOrMeeting {
    pub const CODE: &'static str = "E003";
}

fn non_empty_field(fm: &str, key: &str) -> bool {
    frontmatter::field(fm, key).is_some_and(|v| !v.trim().is_empty())
}

impl Rule for E003EventLocationOrMeeting {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        "Events must declare at least one of `location_address` or `meeting_url`."
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
                "Missing frontmatter (an event needs a `location_address` or a `meeting_url`)",
            );
        };

        if non_empty_field(fm, "location_address") || non_empty_field(fm, "meeting_url") {
            return Vec::new();
        }
        report(
            "An event must declare a physical `location_address` or an online `meeting_url` \
             (a hybrid event may declare both)",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::E003EventLocationOrMeeting;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("web/content/events/20260702_x.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_with_physical_address() {
        let f =
            file("---\nstarts_at: \"2026-07-02T11:00:00\"\nlocation_address: 1920 4th Ave\n---\n");
        assert!(E003EventLocationOrMeeting.lint(&f).is_empty());
    }

    #[test]
    fn passes_with_meeting_url() {
        let f = file(
            "---\nstarts_at: \"2026-07-02T11:00:00\"\nmeeting_url: https://meet.example/x\n---\n",
        );
        assert!(E003EventLocationOrMeeting.lint(&f).is_empty());
    }

    #[test]
    fn passes_for_hybrid_with_both() {
        let f =
            file("---\nlocation_address: 1920 4th Ave\nmeeting_url: https://meet.example/x\n---\n");
        assert!(E003EventLocationOrMeeting.lint(&f).is_empty());
    }

    #[test]
    fn flags_when_neither_present() {
        let v = E003EventLocationOrMeeting
            .lint(&file("---\nstarts_at: \"2026-07-02T11:00:00\"\n---\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "E003");
    }

    #[test]
    fn flags_when_both_blank() {
        let v = E003EventLocationOrMeeting.lint(&file(
            "---\nlocation_address: \"\"\nmeeting_url: \"  \"\n---\n",
        ));
        assert_eq!(v.len(), 1);
    }
}
