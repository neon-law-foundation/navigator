//! `N106` — notation workflow must include a `staff_review` state.
//!
//! Exact-match check on the workflow's state keys — a state like
//! `staff_review__for_grantor` does *not* satisfy this rule; the
//! workflow must declare a bare `staff_review` state. Files without
//! frontmatter or without a `workflow:` key are silently skipped.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct F106StaffReviewRequired;

impl F106StaffReviewRequired {
    pub const CODE: &'static str = "N106";
}

#[derive(Debug, Deserialize)]
struct FrontmatterShape {
    #[serde(default)]
    workflow: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

impl Rule for F106StaffReviewRequired {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let Some(fm) = frontmatter::extract(&file.contents) else {
            return Vec::new();
        };
        let Ok(parsed) = serde_yaml::from_str::<FrontmatterShape>(fm) else {
            return Vec::new();
        };
        let Some(workflow) = parsed.workflow else {
            return Vec::new();
        };
        if workflow.contains_key("staff_review") {
            return Vec::new();
        }
        vec![Violation {
            code: Self::CODE,
            path: file.path.clone(),
            line: 1,
            range: line_byte_range(&file.contents, 1),
            message: "workflow is missing required `staff_review` state".to_string(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::F106StaffReviewRequired;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("test.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_when_workflow_has_bare_staff_review_state() {
        let body = "---
workflow:
  BEGIN:
    created: staff_review
  staff_review:
    approved: END
  END: {}
---
";
        assert!(F106StaffReviewRequired.lint(&file(body)).is_empty());
    }

    #[test]
    fn no_frontmatter_means_no_violation() {
        assert!(F106StaffReviewRequired.lint(&file("just body")).is_empty());
    }

    #[test]
    fn no_workflow_key_means_no_violation() {
        let body = "---\ntitle: T\n---\nbody\n";
        assert!(F106StaffReviewRequired.lint(&file(body)).is_empty());
    }

    #[test]
    fn flags_workflow_without_staff_review_state() {
        let body = "---
workflow:
  BEGIN:
    created: something_else
  something_else:
    done: END
  END: {}
---
";
        let v = F106StaffReviewRequired.lint(&file(body));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "N106");
        assert!(v[0].message.contains("staff_review"));
    }

    #[test]
    fn discriminator_suffix_does_not_satisfy_exact_match() {
        // `staff_review__for_grantor` is NOT a bare `staff_review`.
        // The rule deliberately requires the exact state name.
        let body = "---
workflow:
  BEGIN:
    created: staff_review__for_grantor
  staff_review__for_grantor:
    approved: END
  END: {}
---
";
        let v = F106StaffReviewRequired.lint(&file(body));
        assert_eq!(v.len(), 1);
    }
}
