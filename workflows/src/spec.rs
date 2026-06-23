//! Parsed workflow spec — the shape that lives in a template's
//! YAML frontmatter under `workflow:`.
//!
//! Wire shape:
//!
//! ```yaml
//! workflow:
//!   BEGIN:
//!     created: staff_review__for_grantor
//!   staff_review__for_grantor:
//!     approve: notarization__for_grantor
//!     reject:  END
//!   notarization__for_grantor:
//!     signed:  mailroom_send__to_signer
//!     refused: END
//!   mailroom_send__to_signer:
//!     sent:    mailroom_receive__signed_copy
//!   mailroom_receive__signed_copy:
//!     received: END
//!   END: {}
//! ```
//!
//! State names use `<prefix>__<discriminator>` form; the prefix
//! selects the [`crate::StepKind`] (system / staff_review /
//! notarization / mailroom_send / mailroom_receive).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A workflow state name, e.g., `staff_review__for_trustee`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StateName(pub String);

impl StateName {
    pub const BEGIN: &'static str = "BEGIN";
    pub const END: &'static str = "END";

    #[must_use]
    pub fn begin() -> Self {
        Self(Self::BEGIN.to_string())
    }

    #[must_use]
    pub fn end() -> Self {
        Self(Self::END.to_string())
    }

    /// Prefix used by [`crate::step_kind_for`] to pick the step
    /// type. For `staff_review__for_trustee` returns
    /// `"staff_review"`; for `BEGIN` or `END` returns the whole
    /// name verbatim.
    #[must_use]
    pub fn prefix(&self) -> &str {
        self.0.split_once("__").map_or(self.0.as_str(), |(p, _)| p)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for StateName {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Transitions out of a state: `condition -> next state`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TransitionMap(pub BTreeMap<String, StateName>);

impl TransitionMap {
    #[must_use]
    pub fn lookup(&self, condition: &str) -> Option<&StateName> {
        self.0.get(condition)
    }

    pub fn conditions(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Actor class allowed to transition out of a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorClass {
    /// Driven by the durable runtime — no human in the loop.
    System,
    /// A staff member triggers the transition.
    Staff,
    /// The respondent (the person/entity the notation is for)
    /// triggers the transition.
    Respondent,
}

/// Which of a Notation's two state machines a runtime call targets.
///
/// A Notation runs *two* state machines back-to-back: a
/// [`QuestionnaireSpec`] walks the respondent through the
/// declared questions, then a [`WorkflowSpec`] drives the
/// resulting document to final disposition. Both share the same
/// runtime surface; this enum is the key partitioner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MachineKind {
    /// The questionnaire walker — asks one question per signal.
    Questionnaire,
    /// The post-intake workflow — drives staff review, signing,
    /// mailroom, etc.
    Workflow,
}

impl MachineKind {
    /// Stable lowercase token used in Restate handler URLs and
    /// glossary copy.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Questionnaire => "questionnaire",
            Self::Workflow => "workflow",
        }
    }
}

/// Full workflow spec parsed from frontmatter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowSpec {
    pub states: BTreeMap<StateName, TransitionMap>,
}

#[derive(Debug, Error)]
pub enum WorkflowSpecError {
    #[error("missing required state `BEGIN`")]
    MissingBegin,
    #[error("missing required state `END`")]
    MissingEnd,
    #[error("state `{from}` has transition `{condition}` to unknown state `{to}`")]
    DanglingTransition {
        from: String,
        condition: String,
        to: String,
    },
    #[error("yaml parse error: {0}")]
    Yaml(String),
}

impl WorkflowSpec {
    /// Parse from a YAML document. Validates `BEGIN`/`END` presence
    /// and that every transition target exists.
    pub fn from_yaml(yaml: &str) -> Result<Self, WorkflowSpecError> {
        let spec: Self =
            serde_yaml::from_str(yaml).map_err(|e| WorkflowSpecError::Yaml(e.to_string()))?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<(), WorkflowSpecError> {
        if !self.states.contains_key(&StateName::begin()) {
            return Err(WorkflowSpecError::MissingBegin);
        }
        if !self.states.contains_key(&StateName::end()) {
            return Err(WorkflowSpecError::MissingEnd);
        }
        for (from, transitions) in &self.states {
            for (condition, target) in &transitions.0 {
                if !self.states.contains_key(target) {
                    return Err(WorkflowSpecError::DanglingTransition {
                        from: from.0.clone(),
                        condition: condition.clone(),
                        to: target.0.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Transitions out of `state`, or `None` if no such state.
    #[must_use]
    pub fn transitions_from(&self, state: &StateName) -> Option<&TransitionMap> {
        self.states.get(state)
    }

    #[must_use]
    pub fn is_terminal(&self, state: &StateName) -> bool {
        self.states.get(state).is_some_and(TransitionMap::is_empty)
    }
}

/// Parsed `questionnaire:` block from a notation template's
/// frontmatter. Same wire shape as [`WorkflowSpec`] — a graph of
/// named states keyed by transition condition — but distinct at
/// the type level so the application can't accidentally hand a
/// questionnaire spec to a workflow runtime call (or vice versa).
///
/// Wire shape (matches the retainer template's
/// [`notation_templates/onboarding/retainer.md`](../../../notation_templates/onboarding/retainer.md)
/// `questionnaire:` block):
///
/// ```yaml
/// questionnaire:
///   BEGIN:
///     _: client_name
///   client_name:
///     _: client_email
///   client_email:
///     _: END
///   END: {}
/// ```
///
/// State names are bare question codes (no `__discriminator`
/// suffix in practice — questionnaires only ever ask one
/// respondent), and the canonical transition condition is the
/// underscore literal `_` since the only signal that advances a
/// questionnaire is "the respondent answered."
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuestionnaireSpec(pub WorkflowSpec);

impl QuestionnaireSpec {
    /// Parse from a YAML document. Reuses [`WorkflowSpec`]'s
    /// validation: `BEGIN` and `END` required, every transition
    /// target must resolve.
    pub fn from_yaml(yaml: &str) -> Result<Self, WorkflowSpecError> {
        WorkflowSpec::from_yaml(yaml).map(Self)
    }

    /// Borrow the underlying [`WorkflowSpec`] — useful when a
    /// runtime trait method takes the canonical machine spec.
    #[must_use]
    pub fn inner(&self) -> &WorkflowSpec {
        &self.0
    }

    /// Consume into the underlying [`WorkflowSpec`].
    #[must_use]
    pub fn into_inner(self) -> WorkflowSpec {
        self.0
    }

    /// Transitions out of `state`, or `None` if no such state.
    #[must_use]
    pub fn transitions_from(&self, state: &StateName) -> Option<&TransitionMap> {
        self.0.transitions_from(state)
    }

    /// Whether `state` is terminal (no outgoing transitions).
    #[must_use]
    pub fn is_terminal(&self, state: &StateName) -> bool {
        self.0.is_terminal(state)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActorClass, MachineKind, QuestionnaireSpec, StateName, WorkflowSpec, WorkflowSpecError,
    };

    const TRUST_WORKFLOW: &str = r"
BEGIN:
  created: staff_review__for_grantor
staff_review__for_grantor:
  approve: notarization__for_grantor
  reject: END
notarization__for_grantor:
  signed: mailroom_send__to_signer
  refused: END
mailroom_send__to_signer:
  sent: mailroom_receive__signed_copy
mailroom_receive__signed_copy:
  received: END
END: {}
";

    #[test]
    fn parses_a_realistic_trust_workflow() {
        let spec = WorkflowSpec::from_yaml(TRUST_WORKFLOW).expect("valid spec");
        assert_eq!(spec.states.len(), 6);
        let begin = spec.transitions_from(&StateName::begin()).unwrap();
        assert_eq!(
            begin.lookup("created").unwrap().as_str(),
            "staff_review__for_grantor"
        );
        assert!(spec.is_terminal(&StateName::end()));
    }

    #[test]
    fn rejects_spec_without_begin_state() {
        let err = WorkflowSpec::from_yaml("END: {}\n").unwrap_err();
        assert!(matches!(err, WorkflowSpecError::MissingBegin));
    }

    #[test]
    fn rejects_spec_without_end_state() {
        let err = WorkflowSpec::from_yaml("BEGIN: {created: somewhere}\n").unwrap_err();
        assert!(matches!(err, WorkflowSpecError::MissingEnd));
    }

    #[test]
    fn rejects_dangling_transition_target() {
        let err = WorkflowSpec::from_yaml("BEGIN: {go: nowhere}\nEND: {}\n").unwrap_err();
        assert!(matches!(
            err,
            WorkflowSpecError::DanglingTransition { to, .. } if to == "nowhere"
        ));
    }

    #[test]
    fn state_name_prefix_strips_double_underscore_discriminator() {
        assert_eq!(
            StateName::from("staff_review__for_trustee").prefix(),
            "staff_review"
        );
        assert_eq!(StateName::begin().prefix(), "BEGIN");
        assert_eq!(StateName::from("notarization").prefix(), "notarization");
    }

    #[test]
    fn actor_class_serialization_matches_yaml_lowercase() {
        let yaml = serde_yaml::to_string(&ActorClass::Staff).unwrap();
        assert_eq!(yaml.trim(), "staff");
        let back: ActorClass = serde_yaml::from_str("system").unwrap();
        assert_eq!(back, ActorClass::System);
    }

    #[test]
    fn yaml_parse_error_surfaces_as_workflow_spec_error_yaml_variant() {
        // Plain string at the top level — fails to deserialize into
        // the spec's BTreeMap shape before validation runs.
        let err = WorkflowSpec::from_yaml("just_a_scalar_string\n").unwrap_err();
        assert!(matches!(err, WorkflowSpecError::Yaml(_)), "got {err:?}");
    }

    const RETAINER_QUESTIONNAIRE: &str = r"
BEGIN:
  _: client_name
client_name:
  _: client_email
client_email:
  _: project_name
project_name:
  _: product_description
product_description:
  _: END
END: {}
";

    #[test]
    fn questionnaire_spec_parses_the_retainer_questionnaire_block() {
        let q = QuestionnaireSpec::from_yaml(RETAINER_QUESTIONNAIRE).expect("valid");
        let first = q
            .transitions_from(&StateName::begin())
            .and_then(|t| t.lookup("_"))
            .map(StateName::as_str);
        assert_eq!(first, Some("client_name"));
        assert!(q.is_terminal(&StateName::end()));
    }

    #[test]
    fn questionnaire_spec_reuses_workflow_spec_validation_for_missing_begin() {
        let err = QuestionnaireSpec::from_yaml("END: {}\n").unwrap_err();
        assert!(matches!(err, WorkflowSpecError::MissingBegin));
    }

    #[test]
    fn questionnaire_spec_exposes_underlying_workflow_spec() {
        let q = QuestionnaireSpec::from_yaml(RETAINER_QUESTIONNAIRE).unwrap();
        // `inner()` borrows the same graph; `into_inner()` consumes.
        assert_eq!(q.inner().states.len(), 6);
        let unwrapped = q.into_inner();
        assert!(unwrapped.is_terminal(&StateName::end()));
    }

    #[test]
    fn machine_kind_serializes_as_lowercase_tokens() {
        let q = serde_yaml::to_string(&MachineKind::Questionnaire).unwrap();
        let w = serde_yaml::to_string(&MachineKind::Workflow).unwrap();
        assert_eq!(q.trim(), "questionnaire");
        assert_eq!(w.trim(), "workflow");
    }

    #[test]
    fn machine_kind_as_str_matches_serde_tokens() {
        assert_eq!(MachineKind::Questionnaire.as_str(), "questionnaire");
        assert_eq!(MachineKind::Workflow.as_str(), "workflow");
    }
}
