#![allow(clippy::doc_markdown)]
//! `N110` — a notation template under a known jurisdiction must encode
//! that jurisdiction in its path.
//!
//! Substantive legal templates live at
//! `notation_templates/<jurisdiction>/<scope>/<bar_exam_topic>/<name>.md`,
//! so the mark on file carries its own reach: `united_states/nevada/
//! trusts_and_estates/trust.md` is a Nevada trusts-and-estates document
//! and nothing else. This rule enforces that grammar **for any file
//! placed under a known jurisdiction root**, with the scope and topic
//! drawn from closed lists (the single source of truth lives here, so
//! extending the vocabulary is a one-line edit with a test).
//!
//! The rule **fails closed**: an unknown scope or bar-exam topic, or the
//! wrong path depth, is a violation — never a silent pass — so the
//! convention cannot quietly rot.
//!
//! Files that are **not** under a known jurisdiction root are skipped:
//! the operational branch (`engagements/`, `correspondence/`, `filings/`,
//! `services/`) and the grandfathered legacy flat folders predate this
//! grammar and carry no jurisdiction segment, so `N110` says nothing
//! about them. Snake-case of the filename itself is `N103`'s job; this
//! rule owns the directory grammar.

use crate::{is_snake_case, line_byte_range, Rule, SourceFile, Violation};
use std::path::Path;

/// The bar-exam topics (standard MBE/MEE subject list) that name the
/// third path segment of a substantive template. Single source of truth.
pub const BAR_EXAM_TOPICS: &[&str] = &[
    "business_associations",
    "civil_procedure",
    "conflict_of_laws",
    "constitutional_law",
    "contracts",
    "criminal_law_and_procedure",
    "evidence",
    "family_law",
    "real_property",
    "secured_transactions",
    "torts",
    "trusts_and_estates",
];

/// Known jurisdiction roots and the scopes each allows for its second
/// path segment. `federal` plus the firm's states of admission — never a
/// state the firm cannot practice in. New jurisdictions (e.g. `germany`,
/// with its own scopes) are added as a row here.
pub const JURISDICTIONS: &[(&str, &[&str])] = &[(
    "united_states",
    &["federal", "nevada", "california", "washington"],
)];

pub struct F110JurisdictionPath;

impl F110JurisdictionPath {
    pub const CODE: &'static str = "N110";

    /// The segments of `path` that follow the `notation_templates`
    /// component, as `&str`. `None` when the path has no
    /// `notation_templates` component at all (the rule then says
    /// nothing — it only governs the canonical tree).
    fn segments_under_tree(path: &Path) -> Option<Vec<&str>> {
        let mut comps = path.components();
        let mut found = false;
        for c in comps.by_ref() {
            if let std::path::Component::Normal(seg) = c {
                if seg == "notation_templates" {
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return None;
        }
        Some(
            comps
                .filter_map(|c| match c {
                    std::path::Component::Normal(seg) => seg.to_str(),
                    _ => None,
                })
                .collect(),
        )
    }
}

impl Rule for F110JurisdictionPath {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        "Notation template under a jurisdiction must match <jurisdiction>/<scope>/<bar_exam_topic>/<name>.md"
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let Some(rel) = Self::segments_under_tree(&file.path) else {
            return Vec::new();
        };
        // The first segment under the tree decides whether this rule
        // governs the file. Only a known jurisdiction root opts in;
        // the operational branch and legacy flat folders are skipped.
        let Some(first) = rel.first() else {
            return Vec::new();
        };
        let Some((jurisdiction, scopes)) = JURISDICTIONS.iter().find(|(j, _)| j == first) else {
            return Vec::new();
        };

        let report = |message: String| -> Vec<Violation> {
            vec![Violation {
                code: Self::CODE,
                path: file.path.clone(),
                line: 1,
                range: line_byte_range(&file.contents, 1),
                message,
            }]
        };

        // Expected shape: [jurisdiction, scope, topic, file.md] — exactly
        // four segments (one scope dir, one topic dir, one file).
        if rel.len() != 4 {
            return report(format!(
                "Template under `{jurisdiction}/` must live at \
                 `<jurisdiction>/<scope>/<bar_exam_topic>/<name>.md`; found `{}`",
                rel.join("/")
            ));
        }

        let scope = rel[1];
        let topic = rel[2];
        let mut violations = Vec::new();
        if !scopes.contains(&scope) {
            violations.push(format!(
                "Unknown scope `{scope}` under `{jurisdiction}/`; expected one of {scopes:?}"
            ));
        }
        if !BAR_EXAM_TOPICS.contains(&topic) {
            violations.push(format!(
                "Unknown bar-exam topic `{topic}`; expected one of the standard MBE/MEE subjects \
                 ({BAR_EXAM_TOPICS:?})"
            ));
        } else if !is_snake_case(topic) {
            // Topics in the closed list are already snake_case; this guards
            // a future list edit that slips in a non-snake entry.
            violations.push(format!("Bar-exam topic `{topic}` is not snake_case"));
        }

        violations
            .into_iter()
            .map(|message| Violation {
                code: Self::CODE,
                path: file.path.clone(),
                line: 1,
                range: line_byte_range(&file.contents, 1),
                message,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::F110JurisdictionPath;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn at(path: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from(path),
            contents: "---\ntitle: x\n---\nbody\n".to_string(),
        }
    }

    #[test]
    fn accepts_a_well_formed_jurisdiction_path() {
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/nevada/trusts_and_estates/trust.md",
        ));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn accepts_federal_scope() {
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/federal/evidence/business_records.md",
        ));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn skips_operational_branch() {
        // Not under a jurisdiction root — the rule says nothing.
        for p in [
            "notation_templates/engagements/retainer.md",
            "notation_templates/correspondence/closing_letter.md",
            "notation_templates/filings/nevada/modified_business_tax.md",
            "notation_templates/services/contract_review.md",
        ] {
            assert!(F110JurisdictionPath.lint(&at(p)).is_empty(), "{p}");
        }
    }

    #[test]
    fn skips_grandfathered_legacy_flat_folders() {
        let v = F110JurisdictionPath.lint(&at("notation_templates/trust/nevada.md"));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn skips_files_outside_the_tree() {
        let v = F110JurisdictionPath.lint(&at("web/content/marketing/service.md"));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn flags_wrong_depth() {
        // Missing the topic level.
        let v = F110JurisdictionPath.lint(&at("notation_templates/united_states/nevada/trust.md"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "N110");
        assert!(v[0].message.contains("must live at"), "{}", v[0].message);
    }

    #[test]
    fn flags_unknown_scope() {
        // Arizona is not a firm admission.
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/arizona/torts/negligence.md",
        ));
        assert_eq!(v.len(), 1);
        assert!(
            v[0].message.contains("Unknown scope `arizona`"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn flags_unknown_bar_topic() {
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/nevada/space_law/asteroid.md",
        ));
        assert_eq!(v.len(), 1);
        assert!(
            v[0].message.contains("Unknown bar-exam topic `space_law`"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn flags_both_unknown_scope_and_topic() {
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/arizona/space_law/x.md",
        ));
        assert_eq!(v.len(), 2);
    }
}
