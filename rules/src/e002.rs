//! `E002` — a markdown file is either an event or a notation template,
//! never both.
//!
//! An event declares a `starts_at` timestamp; a notation template
//! declares a `questionnaire:` and/or `workflow:` state machine. The two
//! contracts are mutually exclusive: a file that carries a timestamp must
//! not carry questionnaire/workflow, and vice versa. This rule runs in
//! both rule sets so the conflict is caught whichever way the file
//! classifies.

use serde_yaml::Value;

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct E002EventTemplateExclusive;

impl E002EventTemplateExclusive {
    pub const CODE: &'static str = "E002";
}

fn has_key(contents: &str, key: &str) -> bool {
    let Some(fm) = frontmatter::extract(contents) else {
        return false;
    };
    let Ok(Value::Mapping(map)) = serde_yaml::from_str::<Value>(fm) else {
        return false;
    };
    map.contains_key(Value::String(key.to_string()))
}

impl Rule for E002EventTemplateExclusive {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        "A file is either an event (`starts_at`) or a notation template \
         (`questionnaire`/`workflow`), never both."
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let is_event = has_key(&file.contents, "starts_at");
        let is_template =
            has_key(&file.contents, "questionnaire") || has_key(&file.contents, "workflow");
        if is_event && is_template {
            return vec![Violation {
                code: Self::CODE,
                path: file.path.clone(),
                line: 1,
                range: line_byte_range(&file.contents, 1),
                message: "A file with a `starts_at` timestamp is an event and must not also \
                          declare `questionnaire`/`workflow` (those make it a notation template)"
                    .to_string(),
            }];
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::E002EventTemplateExclusive;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("doc.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_for_event_without_machine() {
        let f = file("---\nstarts_at: \"2026-07-02T11:00:00\"\ntimezone: America/Denver\n---\n");
        assert!(E002EventTemplateExclusive.lint(&f).is_empty());
    }

    #[test]
    fn passes_for_template_without_timestamp() {
        let f = file("---\ntitle: T\nquestionnaire:\n  BEGIN:\n    _: client_name\n---\n");
        assert!(E002EventTemplateExclusive.lint(&f).is_empty());
    }

    #[test]
    fn flags_event_that_also_has_questionnaire() {
        let f = file(
            "---\nstarts_at: \"2026-07-02T11:00:00\"\nquestionnaire:\n  BEGIN:\n    _: x\n---\n",
        );
        let v = E002EventTemplateExclusive.lint(&f);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "E002");
    }

    #[test]
    fn flags_template_that_also_has_timestamp() {
        let f =
            file("---\nworkflow:\n  BEGIN:\n    x: END\nstarts_at: \"2026-07-02T11:00:00\"\n---\n");
        let v = E002EventTemplateExclusive.lint(&f);
        assert_eq!(v.len(), 1);
    }
}
