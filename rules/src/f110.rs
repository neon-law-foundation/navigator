#![allow(clippy::doc_markdown)]
//! `N110` — a notation template under a known jurisdiction must encode
//! that jurisdiction in its path.
//!
//! Substantive legal templates live at
//! `notation_templates/<jurisdiction>/<scope>/<forum>/<practice_area>/<name>.md`,
//! so the mark on file carries its own reach: `united_states/nevada/
//! state/business_associations/llc.md` is a Nevada business-associations
//! document filed with the state and nothing else. This rule enforces
//! that grammar **for any file placed under a known jurisdiction root**,
//! with the scope, forum, and practice area each drawn from a closed list
//! (the single source of truth lives here, so extending the vocabulary is
//! a one-line edit with a test).
//!
//! The four segments after the jurisdiction are all **mandatory**:
//!
//! - **scope** — `federal` or one of the firm's states of admission
//!   (`nevada`, `california`, `washington`); never a state the firm
//!   cannot practice in.
//! - **forum** — the counterparty / sovereign the document is filed with
//!   (`state`, `clark_county`, `irs`, …), or `internal` when there is no
//!   government counterparty. Forum is required everywhere: a document
//!   with no government counterparty is `internal`, not absent.
//! - **practice_area** — the body of law, drawn from the standard MBE/MEE
//!   subjects plus the firm's own practice areas (`debt_relief`,
//!   `taxation`, …).
//!
//! The rule **fails closed**: an unknown scope, forum, or practice area,
//! or the wrong path depth, is a violation — never a silent pass — so the
//! convention cannot quietly rot.
//!
//! Files that are **not** under a known jurisdiction root are skipped:
//! the operational branch (`engagements/`, `filings/`, `services/`) and
//! the brand quarantine (`neon_law/`) carry no jurisdiction segment, so
//! `N110` says nothing about them. Snake-case of
//! the filename itself is `N103`'s job; this rule owns the directory
//! grammar.

use crate::{is_snake_case, line_byte_range, Rule, SourceFile, Violation};
use std::path::Path;

/// The practice areas that name the fourth path segment of a substantive
/// template — the standard MBE/MEE subject list plus the firm's own
/// practice areas the bar list does not cover (`debt_relief`, `taxation`,
/// `intellectual_property`, `immigration`, `landlord_tenant`). Single
/// source of truth; extend with a one-line edit and a test.
pub const PRACTICE_AREAS: &[&str] = &[
    "business_associations",
    "civil_procedure",
    "conflict_of_laws",
    "constitutional_law",
    "contracts",
    "criminal_law_and_procedure",
    "debt_relief",
    "evidence",
    "family_law",
    "immigration",
    "intellectual_property",
    "landlord_tenant",
    "real_property",
    "secured_transactions",
    "taxation",
    "torts",
    "trusts_and_estates",
];

/// The forums that name the third path segment — the counterparty or
/// sovereign a document is filed with, or `internal` when there is no
/// government counterparty. `internal` means "no government/sovereign on
/// the other side" — a document that stays within the client relationship
/// — **not** "internal to the firm"; the firm-vs-client distinction lives
/// elsewhere. Counties and agencies live here (they are not jurisdiction
/// rows); tribal nations are sovereigns that *could* become jurisdiction
/// rows later. Single source of truth.
///
/// The list is deliberately **flat / scope-agnostic**: a county or agency
/// forum (`clark_county`, `washoe_county`, …) validates under any scope,
/// so a geographically odd path like `california/washoe_county/...` passes
/// N110. Keying forums per scope is a future refinement, not a gap — the
/// firm's template set is small enough that the flat list is the simpler,
/// fixed-depth grammar we want today.
pub const FORUMS: &[&str] = &[
    "internal",
    "state",
    "clark_county",
    "washoe_county",
    "secretary_of_state",
    "department_of_taxation",
    "irs",
    "uspto",
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
        "Notation template under a jurisdiction must match \
         <jurisdiction>/<scope>/<forum>/<practice_area>/<name>.md"
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let Some(rel) = Self::segments_under_tree(&file.path) else {
            return Vec::new();
        };
        // The first segment under the tree decides whether this rule
        // governs the file. Only a known jurisdiction root opts in;
        // the operational branch, the brand quarantine, and legacy flat
        // folders are skipped.
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

        // Expected shape: [jurisdiction, scope, forum, practice_area,
        // file.md] — exactly five segments. Forum is mandatory (`internal`
        // when there is no government counterparty), so the depth is
        // fixed.
        if rel.len() != 5 {
            return report(format!(
                "Template under `{jurisdiction}/` must live at \
                 `<jurisdiction>/<scope>/<forum>/<practice_area>/<name>.md`; found `{}`",
                rel.join("/")
            ));
        }

        let scope = rel[1];
        let forum = rel[2];
        let practice_area = rel[3];
        let mut violations = Vec::new();
        if !scopes.contains(&scope) {
            violations.push(format!(
                "Unknown scope `{scope}` under `{jurisdiction}/`; expected one of {scopes:?}"
            ));
        } else if !is_snake_case(scope) {
            // Scopes in the closed list are already snake_case; this guards
            // a future list edit that slips in a non-snake entry.
            violations.push(format!("Scope `{scope}` is not snake_case"));
        }
        if !FORUMS.contains(&forum) {
            violations.push(format!(
                "Unknown forum `{forum}`; expected one of {FORUMS:?} \
                 (use `internal` when there is no government counterparty)"
            ));
        } else if !is_snake_case(forum) {
            // Forums in the closed list are already snake_case; this guards
            // a future list edit that slips in a non-snake entry.
            violations.push(format!("Forum `{forum}` is not snake_case"));
        }
        if !PRACTICE_AREAS.contains(&practice_area) {
            violations.push(format!(
                "Unknown practice area `{practice_area}`; expected one of the standard MBE/MEE \
                 subjects or a firm practice area ({PRACTICE_AREAS:?})"
            ));
        } else if !is_snake_case(practice_area) {
            // Areas in the closed list are already snake_case; this guards
            // a future list edit that slips in a non-snake entry.
            violations.push(format!("Practice area `{practice_area}` is not snake_case"));
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
            "notation_templates/united_states/nevada/internal/trusts_and_estates/trust.md",
        ));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn accepts_a_state_forum_filing() {
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/nevada/state/business_associations/llc.md",
        ));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn accepts_federal_scope_with_agency_forum() {
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/federal/irs/taxation/form_990.md",
        ));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn accepts_a_firm_practice_area() {
        // `debt_relief` is not on the bar list but is a firm practice area.
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/federal/internal/debt_relief/fcra_dispute.md",
        ));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn skips_operational_branch() {
        // Not under a jurisdiction root — the rule says nothing.
        for p in [
            "notation_templates/engagements/retainer.md",
            "notation_templates/filings/nevada/modified_business_tax.md",
            "notation_templates/services/contract_review.md",
        ] {
            assert!(F110JurisdictionPath.lint(&at(p)).is_empty(), "{p}");
        }
    }

    #[test]
    fn skips_brand_quarantine() {
        // `neon_law/` carries no jurisdiction segment — neither a product
        // folder nor the cross-product `shared/` bin.
        for p in [
            "notation_templates/neon_law/nautilus/retainer.md",
            "notation_templates/neon_law/shared/closing_letter.md",
        ] {
            assert!(F110JurisdictionPath.lint(&at(p)).is_empty(), "{p}");
        }
    }

    #[test]
    fn skips_unknown_non_jurisdiction_roots() {
        let v = F110JurisdictionPath.lint(&at("notation_templates/archive/trust.md"));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn skips_files_outside_the_tree() {
        let v = F110JurisdictionPath.lint(&at("web/content/marketing/service.md"));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn flags_missing_forum_depth() {
        // Four segments — the old grammar, now missing the forum level.
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/nevada/trusts_and_estates/trust.md",
        ));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "N110");
        assert!(v[0].message.contains("must live at"), "{}", v[0].message);
    }

    #[test]
    fn flags_too_shallow_depth() {
        let v = F110JurisdictionPath.lint(&at("notation_templates/united_states/nevada/trust.md"));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("must live at"), "{}", v[0].message);
    }

    #[test]
    fn flags_unknown_scope() {
        // Arizona is not a firm admission.
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/arizona/internal/torts/negligence.md",
        ));
        assert_eq!(v.len(), 1);
        assert!(
            v[0].message.contains("Unknown scope `arizona`"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn flags_unknown_forum() {
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/nevada/mars_colony/torts/negligence.md",
        ));
        assert_eq!(v.len(), 1);
        assert!(
            v[0].message.contains("Unknown forum `mars_colony`"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn flags_unknown_practice_area() {
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/nevada/internal/space_law/asteroid.md",
        ));
        assert_eq!(v.len(), 1);
        assert!(
            v[0].message.contains("Unknown practice area `space_law`"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn flags_unknown_scope_forum_and_practice_area_together() {
        let v = F110JurisdictionPath.lint(&at(
            "notation_templates/united_states/arizona/mars_colony/space_law/x.md",
        ));
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn closed_lists_stay_snake_case() {
        // The `is_snake_case` guards on scope, forum, and practice_area are
        // unreachable while every list entry is snake_case — which is
        // exactly the invariant they defend. This test fails the moment a
        // future list edit slips in a non-snake entry, catching the drift
        // at test time the way the inline guards catch it at lint time.
        use super::{FORUMS, JURISDICTIONS, PRACTICE_AREAS};
        use crate::is_snake_case;
        for forum in FORUMS {
            assert!(is_snake_case(forum), "forum `{forum}` is not snake_case");
        }
        for area in PRACTICE_AREAS {
            assert!(
                is_snake_case(area),
                "practice area `{area}` is not snake_case"
            );
        }
        for (_, scopes) in JURISDICTIONS {
            for scope in *scopes {
                assert!(is_snake_case(scope), "scope `{scope}` is not snake_case");
            }
        }
    }
}
