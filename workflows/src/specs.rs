//! Workflow specs that the workspace ships. Loaded at compile time
//! via `include_str!` so binaries don't have to read template + spec
//! files off disk at boot — they're part of the crate.
//!
//! Each bundled template has a paired
//! `workflows/specs/<code>.yaml` carrying the *same* `workflow:` and
//! `questionnaire:` blocks that live in its markdown frontmatter
//! today. The standalone YAML is the format `cli scaffold` will
//! generate first; the template markdown keeps the rendering body
//! (and, for now, a mirrored copy of the spec that the integrity test
//! pins against).
//!
//! Adding a new workflow: drop a notation template under
//! `notation_templates/<category>/<name>.md`, write the same `workflow:` +
//! `questionnaire:` blocks into `workflows/specs/<code>.yaml`, and
//! add the file to [`BUNDLED_SPEC_YAML`] below. The coherence test
//! in `workflows/tests/spec_coherence.rs` catches any drift between
//! the two sources.

use serde::Deserialize;

use crate::spec::{QuestionnaireSpec, WorkflowSpec, WorkflowSpecError};

/// Raw markdown body for the retainer-intake notation template. Used
/// by the rendering layer (`views::notation::render_filled_in`) and
/// the integrity / coherence tests; the workflow spec itself now
/// loads from [`RETAINER_INTAKE_SPEC_YAML`].
pub const RETAINER_INTAKE_TEMPLATE: &str =
    include_str!("../../notation_templates/engagements/retainer.md");

/// Standalone YAML carrying both `questionnaire:` and `workflow:`
/// blocks for the retainer intake template.
pub const RETAINER_INTAKE_SPEC_YAML: &str = include_str!("../specs/onboarding__retainer.yaml");

/// Welcome-email workflow spec. Lives outside [`BUNDLED_SPEC_YAML`]
/// because the welcome flow is a notification, not a legal-document
/// notation — the N-family lint rules (staff_review required, state
/// names map to question codes) assume the latter and don't apply.
/// The worker reads this constant directly when handling the
/// `onboarding__welcome` notation.
pub const WELCOME_SPEC_YAML: &str = include_str!("../specs/onboarding__welcome.yaml");

/// Parsed welcome workflow spec.
#[must_use]
pub fn welcome_spec() -> WorkflowSpec {
    workflow_spec_from_yaml(WELCOME_SPEC_YAML)
        .expect("welcome spec is bundled; its workflow block must parse")
}

/// Workshop completion certificate workflow spec. Like the welcome flow
/// it lives outside [`BUNDLED_SPEC_YAML`] — it's a notification, not a
/// legal-document notation, so the N-family lint rules don't apply.
/// `BEGIN --requested--> email_send__certificate --email_sent--> END`.
pub const WORKSHOP_CERTIFICATE_SPEC_YAML: &str =
    include_str!("../specs/workshop__certificate.yaml");

/// Parsed workshop-certificate workflow spec.
#[must_use]
pub fn workshop_certificate_spec() -> WorkflowSpec {
    workflow_spec_from_yaml(WORKSHOP_CERTIFICATE_SPEC_YAML)
        .expect("workshop certificate spec is bundled; its workflow block must parse")
}

/// Every bundled spec keyed by its template `code`. Wired up so
/// callers (and `cli scaffold`) can locate the YAML by code without
/// reaching into the filesystem.
pub const BUNDLED_SPEC_YAML: &[(&str, &str)] = &[
    ("onboarding__retainer", RETAINER_INTAKE_SPEC_YAML),
    (
        "onboarding__estate",
        include_str!("../specs/onboarding__estate.yaml"),
    ),
    (
        "onboarding__nest",
        include_str!("../specs/onboarding__nest.yaml"),
    ),
    (
        "onboarding__nest_corp",
        include_str!("../specs/onboarding__nest_corp.yaml"),
    ),
    (
        "onboarding__nest_business_trust",
        include_str!("../specs/onboarding__nest_business_trust.yaml"),
    ),
    (
        "onboarding__nexus",
        include_str!("../specs/onboarding__nexus.yaml"),
    ),
    (
        "closing__letter",
        include_str!("../specs/closing__letter.yaml"),
    ),
    (
        "llc__california",
        include_str!("../specs/llc__california.yaml"),
    ),
    (
        "trusts__nevada",
        include_str!("../specs/trusts__nevada.yaml"),
    ),
    ("will__simple", include_str!("../specs/will__simple.yaml")),
    (
        "dissolution__nevada",
        include_str!("../specs/dissolution__nevada.yaml"),
    ),
    (
        "annual_report__nevada",
        include_str!("../specs/annual_report__nevada.yaml"),
    ),
    (
        "nv_state_tax_filing__modified_business_tax",
        include_str!("../specs/nv_state_tax_filing__modified_business_tax.yaml"),
    ),
    (
        "nonprofit_501c3_formation__nevada",
        include_str!("../specs/nonprofit_501c3_formation__nevada.yaml"),
    ),
    (
        "form_990__annual_report",
        include_str!("../specs/form_990__annual_report.yaml"),
    ),
    (
        "charitable_solicitation_registration__nevada",
        include_str!("../specs/charitable_solicitation_registration__nevada.yaml"),
    ),
    (
        "nautilus__notice_of_representation",
        include_str!("../specs/nautilus__notice_of_representation.yaml"),
    ),
    (
        "nautilus__debt_validation",
        include_str!("../specs/nautilus__debt_validation.yaml"),
    ),
    (
        "nautilus__cease_communication",
        include_str!("../specs/nautilus__cease_communication.yaml"),
    ),
    (
        "nautilus__fcra_dispute",
        include_str!("../specs/nautilus__fcra_dispute.yaml"),
    ),
    (
        "nautilus__settlement_letter",
        include_str!("../specs/nautilus__settlement_letter.yaml"),
    ),
    (
        "services__contract_review",
        include_str!("../specs/services__contract_review.yaml"),
    ),
];

/// Look up the bundled standalone YAML for `code`. Returns `None`
/// if no bundled spec carries that code.
#[must_use]
pub fn bundled_spec_yaml(code: &str) -> Option<&'static str> {
    BUNDLED_SPEC_YAML
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, y)| *y)
}

/// Parsed `retainer_intake` workflow spec, sourced from the
/// standalone YAML.
#[must_use]
pub fn retainer_intake_spec() -> WorkflowSpec {
    workflow_spec_from_yaml(RETAINER_INTAKE_SPEC_YAML)
        .expect("retainer spec is bundled; its workflow block must parse")
}

/// Parsed `retainer_intake` questionnaire spec, sourced from the
/// standalone YAML.
#[must_use]
pub fn retainer_intake_questionnaire() -> QuestionnaireSpec {
    questionnaire_spec_from_yaml(RETAINER_INTAKE_SPEC_YAML)
        .expect("retainer spec is bundled; its questionnaire block must parse")
}

/// Parse a standalone spec YAML (containing `workflow:` and
/// optionally `questionnaire:`) and return the workflow spec.
pub fn workflow_spec_from_yaml(yaml: &str) -> Result<WorkflowSpec, WorkflowSpecError> {
    let wrapper: WorkflowFrontmatter =
        serde_yaml::from_str(yaml).map_err(|e| WorkflowSpecError::Yaml(e.to_string()))?;
    wrapper.workflow.validate()?;
    Ok(wrapper.workflow)
}

/// Parse a standalone spec YAML and return the questionnaire spec.
pub fn questionnaire_spec_from_yaml(yaml: &str) -> Result<QuestionnaireSpec, WorkflowSpecError> {
    let wrapper: QuestionnaireFrontmatter =
        serde_yaml::from_str(yaml).map_err(|e| WorkflowSpecError::Yaml(e.to_string()))?;
    wrapper.questionnaire.inner().validate()?;
    Ok(wrapper.questionnaire)
}

/// Extract the `workflow:` block from a notation template's YAML
/// frontmatter and parse it as a [`WorkflowSpec`]. Used by the
/// integrity / shape-lock tests, which validate that every template's
/// frontmatter is structurally coherent regardless of whether
/// production code reads from the markdown or the standalone YAML.
pub fn workflow_spec_from_template(markdown: &str) -> Result<WorkflowSpec, WorkflowSpecError> {
    let frontmatter = extract_frontmatter(markdown)
        .ok_or_else(|| WorkflowSpecError::Yaml("template has no YAML frontmatter".into()))?;
    workflow_spec_from_yaml(frontmatter)
}

/// Extract the `questionnaire:` block from a notation template's
/// YAML frontmatter and parse it as a [`QuestionnaireSpec`].
pub fn questionnaire_spec_from_template(
    markdown: &str,
) -> Result<QuestionnaireSpec, WorkflowSpecError> {
    let frontmatter = extract_frontmatter(markdown)
        .ok_or_else(|| WorkflowSpecError::Yaml("template has no YAML frontmatter".into()))?;
    questionnaire_spec_from_yaml(frontmatter)
}

#[derive(Deserialize)]
struct WorkflowFrontmatter {
    workflow: WorkflowSpec,
}

#[derive(Deserialize)]
struct QuestionnaireFrontmatter {
    questionnaire: QuestionnaireSpec,
}

fn extract_frontmatter(contents: &str) -> Option<&str> {
    let after_open = contents.strip_prefix("---\n")?;
    if let Some(end) = after_open.find("\n---\n") {
        return Some(&after_open[..end]);
    }
    after_open.strip_suffix("\n---")
}

#[cfg(test)]
mod tests {
    use super::{bundled_spec_yaml, retainer_intake_questionnaire, retainer_intake_spec};
    use crate::spec::StateName;

    #[test]
    fn retainer_intake_spec_parses_from_bundled_yaml() {
        let spec = retainer_intake_spec();
        assert!(spec
            .transitions_from(&StateName::begin())
            .and_then(|t| t.lookup("intake_submitted"))
            .is_some());
    }

    #[test]
    fn retainer_intake_questionnaire_walks_client_name_to_product_description() {
        let q = retainer_intake_questionnaire();
        // BEGIN → client_name → client_email → project_name →
        // product_description → END. Walk via the `_` condition.
        let mut here = StateName::begin();
        let order = [
            "client_name",
            "client_email",
            "project_name",
            "product_description",
            "END",
        ];
        for expected in order {
            let next = q
                .transitions_from(&here)
                .and_then(|t| t.lookup("_"))
                .cloned()
                .expect("each non-terminal state must have an `_` transition");
            assert_eq!(next.as_str(), expected, "from {here:?}");
            here = next;
        }
        assert!(q.is_terminal(&StateName::end()));
    }

    #[test]
    fn retainer_signature_wait_ends_on_both_received_and_declined() {
        // A signed envelope and a declined/voided one both leave the
        // wait state for END; the journal records which condition fired.
        let spec = retainer_intake_spec();
        let wait = StateName::from("sent_for_signature__pending");
        let received = spec
            .transitions_from(&wait)
            .and_then(|t| t.lookup("signature_received"))
            .expect("signature_received edge exists");
        let declined = spec
            .transitions_from(&wait)
            .and_then(|t| t.lookup("signature_declined"))
            .expect("signature_declined edge exists");
        assert_eq!(received.as_str(), "END");
        assert_eq!(declined.as_str(), "END");
    }

    #[test]
    fn bundled_spec_yaml_resolves_every_known_code() {
        for code in [
            "onboarding__retainer",
            "onboarding__estate",
            "onboarding__nest",
            "onboarding__nexus",
            "closing__letter",
            "llc__california",
            "trusts__nevada",
            "will__simple",
            "dissolution__nevada",
            "annual_report__nevada",
            "nv_state_tax_filing__modified_business_tax",
            "nonprofit_501c3_formation__nevada",
            "form_990__annual_report",
            "charitable_solicitation_registration__nevada",
        ] {
            assert!(
                bundled_spec_yaml(code).is_some(),
                "no bundled YAML for {code}",
            );
        }
    }

    #[test]
    fn bundled_spec_yaml_returns_none_for_unknown_code() {
        assert!(bundled_spec_yaml("does__not_exist").is_none());
    }

    #[test]
    fn welcome_spec_drives_signup_through_email_send_to_end() {
        let spec = super::welcome_spec();
        // BEGIN --signup_recorded--> email_send__welcome --email_sent--> END
        let after_begin = spec
            .transitions_from(&StateName::begin())
            .and_then(|t| t.lookup("signup_recorded"))
            .cloned()
            .expect("BEGIN must transition on signup_recorded");
        assert_eq!(after_begin.as_str(), "email_send__welcome");
        let after_send = spec
            .transitions_from(&after_begin)
            .and_then(|t| t.lookup("email_sent"))
            .cloned()
            .expect("email_send state must transition on email_sent");
        assert_eq!(after_send.as_str(), "END");
    }
}
