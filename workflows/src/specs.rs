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
//! `templates/forms/...` or `templates/neon_law/...`,
//! write the same `workflow:` + `questionnaire:` blocks into
//! `workflows/specs/<code>.yaml`, and add the file to
//! [`BUNDLED_SPEC_YAML`] below. The coherence test in
//! `workflows/tests/spec_coherence.rs` catches any drift between the two
//! sources.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::spec::{QuestionnaireSpec, WorkflowSpec, WorkflowSpecError};

/// Raw markdown body for the retainer-intake notation template. Used
/// by the rendering layer (`views::notation::render_filled_in`) and
/// the integrity / coherence tests; the workflow spec itself now
/// loads from [`RETAINER_INTAKE_SPEC_YAML`].
pub const RETAINER_INTAKE_TEMPLATE: &str =
    include_str!("../../templates/neon_law/shared/retainer.md");

/// Standalone YAML carrying both `questionnaire:` and `workflow:`
/// blocks for the retainer intake template.
pub const RETAINER_INTAKE_SPEC_YAML: &str = include_str!("../specs/onboarding__retainer.yaml");
pub const NEST_RETAINER_SPEC_YAML: &str = include_str!("../specs/onboarding__retainer_nest.yaml");

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
    ("onboarding__retainer_nest", NEST_RETAINER_SPEC_YAML),
    (
        "onboarding__estate",
        include_str!("../specs/onboarding__estate.yaml"),
    ),
    (
        "nv__llc_formation",
        include_str!("../specs/nv__llc_formation.yaml"),
    ),
    (
        "nv__profit_corp_formation",
        include_str!("../specs/nv__profit_corp_formation.yaml"),
    ),
    (
        "nv__business_trust_formation",
        include_str!("../specs/nv__business_trust_formation.yaml"),
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
        "ca__llc_operating_agreement",
        include_str!("../specs/ca__llc_operating_agreement.yaml"),
    ),
    (
        "trusts__nevada",
        include_str!("../specs/trusts__nevada.yaml"),
    ),
    ("will__simple", include_str!("../specs/will__simple.yaml")),
    (
        "nv__dissolution",
        include_str!("../specs/nv__dissolution.yaml"),
    ),
    (
        "nv__annual_report",
        include_str!("../specs/nv__annual_report.yaml"),
    ),
    (
        "nv__modified_business_tax",
        include_str!("../specs/nv__modified_business_tax.yaml"),
    ),
    (
        "nv__nonprofit_501c3_formation",
        include_str!("../specs/nv__nonprofit_501c3_formation.yaml"),
    ),
    ("us__form_990", include_str!("../specs/us__form_990.yaml")),
    (
        "nv__charitable_solicitation_registration",
        include_str!("../specs/nv__charitable_solicitation_registration.yaml"),
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
    (
        "us__naturalization",
        include_str!("../specs/us__naturalization.yaml"),
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

/// Parse the optional `prompts:` map from a standalone spec YAML.
pub fn prompt_overrides_from_yaml(
    yaml: &str,
) -> Result<BTreeMap<String, String>, WorkflowSpecError> {
    let wrapper: PromptFrontmatter =
        serde_yaml::from_str(yaml).map_err(|e| WorkflowSpecError::Yaml(e.to_string()))?;
    Ok(wrapper.prompts)
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

/// Extract the optional `prompts:` map from a notation template's
/// YAML frontmatter.
pub fn prompt_overrides_from_template(
    markdown: &str,
) -> Result<BTreeMap<String, String>, WorkflowSpecError> {
    let frontmatter = extract_frontmatter(markdown)
        .ok_or_else(|| WorkflowSpecError::Yaml("template has no YAML frontmatter".into()))?;
    prompt_overrides_from_yaml(frontmatter)
}

#[derive(Deserialize)]
struct WorkflowFrontmatter {
    workflow: WorkflowSpec,
}

#[derive(Deserialize)]
struct QuestionnaireFrontmatter {
    questionnaire: QuestionnaireSpec,
}

#[derive(Deserialize)]
struct PromptFrontmatter {
    #[serde(default)]
    prompts: BTreeMap<String, String>,
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
            "custom_text__client_name",
            "custom_text__client_email",
            "custom_text__project_name",
            "custom_text__product_description",
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
            "onboarding__retainer_nest",
            "onboarding__estate",
            "nv__llc_formation",
            "onboarding__nexus",
            "closing__letter",
            "ca__llc_operating_agreement",
            "trusts__nevada",
            "will__simple",
            "nv__dissolution",
            "nv__annual_report",
            "nv__modified_business_tax",
            "nv__nonprofit_501c3_formation",
            "us__form_990",
            "nv__charitable_solicitation_registration",
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
