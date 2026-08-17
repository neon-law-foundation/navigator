//! `E004` — every event must link to its Luma event.
//!
//! Luma owns everything about attending — where, how, the guest list,
//! add-to-calendar — and the page just shows the event picture and invites
//! the visitor to check it out on Luma. An event that cannot point at a Luma
//! event has nothing to invite anyone to, so `luma_url` is required. A blank
//! value counts as absent.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct E004EventLumaLink;

impl E004EventLumaLink {
    pub const CODE: &'static str = "E004";
}

fn non_empty_field(fm: &str, key: &str) -> bool {
    frontmatter::field(fm, key).is_some_and(|v| !v.trim().is_empty())
}

impl Rule for E004EventLumaLink {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        "Events must declare a `luma_url` to check it out on Luma."
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
            return report("Missing frontmatter (an event needs a `luma_url`)");
        };

        if non_empty_field(fm, "luma_url") {
            return Vec::new();
        }
        report("An event must declare a `luma_url` — Luma hosts the event and its RSVPs")
    }
}

#[cfg(test)]
mod tests {
    use super::E004EventLumaLink;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("web/content/events/20260720_x.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_with_luma_url() {
        let f = file("---\nluma_url: https://luma.com/q8skxd6l\n---\n");
        assert!(E004EventLumaLink.lint(&f).is_empty());
    }

    #[test]
    fn flags_without_luma_url() {
        let v = E004EventLumaLink.lint(&file("---\nstarts_at: \"2026-07-20T11:00:00\"\n---\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "E004");
    }

    #[test]
    fn flags_with_blank_luma_url() {
        let v = E004EventLumaLink.lint(&file("---\nluma_url: \"  \"\n---\n"));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn flags_when_frontmatter_missing() {
        let v = E004EventLumaLink.lint(&file("no frontmatter here\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "E004");
    }
}
