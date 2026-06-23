#![allow(clippy::doc_markdown)]
//! Durable workflow primitives for Navigator.
//!
//! Templates carry both a `questionnaire:` spec and a `workflow:`
//! spec in their YAML frontmatter; each notation (a filled-in
//! template) runs *two* state machines back-to-back — the
//! questionnaire walker (one signal per answered question), then
//! the post-intake workflow (staff review → signing → mailroom).
//! Both share the same wire shape and are keyed under one Restate
//! service per Notation; the [`spec::MachineKind`] enum picks
//! which timeline a runtime call targets.
//!
//! This crate owns the workflow *shape* — the parsed spec, the
//! step trait, and a durable-runtime adapter trait. The actual
//! durable runtime (Restate, or a stub for local dev) is plugged
//! in through [`runtime::StateMachineRuntime`] so the rest of the
//! application doesn't bind to a specific orchestrator.

pub mod attest;
pub mod closing;
pub mod compliance;
pub mod dispatch;
pub mod document;
pub mod email;
pub mod guardrail;
pub mod intake;
pub mod nautilus;
pub mod notation_session;
pub mod notify;
pub mod runtime;
pub mod runtime_dispatching;
pub mod runtime_restate;
pub mod spec;
pub mod specs;
pub mod step;
pub mod trigger;

pub use attest::{
    attestor_from_env, dispatch_onchain_record, AttestError, AttestationRequest, Attestor,
    NullAttestor, OnChainPayload, RecordedTx,
};
pub use closing::{close_matter, closes_matter, CloseError};
pub use compliance::{
    dispatch_compliance, is_dispatched_submission, ComplianceError, CompliancePayload,
};
pub use dispatch::{dispatch_step, dispatches_side_effect, StepDeps, StepDispatchError};
pub use document::{dispatch_document_open, DocumentError, DocumentPayload};
pub use email::{
    dispatch_state, parse_slug, template_for_slug, CapturingEmail, DispatchError, EmailError,
    EmailPayload, EmailService, OutboundEmail, SendGridEmail, SendReceipt, Template,
    DEFAULT_FROM_EMAIL,
};
pub use guardrail::{staff_review_gates_filing, staff_review_precedes_submission, GateViolation};
pub use intake::{
    dispatch_document_intake, is_document_intake, IntakeArtifact, IntakeError, IntakePayload,
};
pub use nautilus::{
    classify, classify_fcra_result, classify_verification,
    continued_collection_is_possible_violation, deadline_from, firm_cut_of_savings_cents,
    litigation_referral, route, triage, CollectorMailClass, DeadlineKind, FcraDisputeResult,
    LitigationReferral, TriageRoute, VerificationOutcome, CEASE_DOES_NOT_ERASE_DEBT,
};
pub use notation_session::{
    answer_step, current_step, start_notation, NextStep, NotationSessionError, QuestionDescriptor,
    StartOutcome,
};
pub use notify::{
    ops_slack_text, CapturingNotifier, Notifier, NotifyError, SlackNotifier, SlackOpsDelivery,
};
pub use runtime::{InMemoryRuntime, StateMachineRuntime, WorkflowEvent, WorkflowRuntimeError};
pub use runtime_dispatching::DispatchingRuntime;
pub use runtime_restate::{RestateRuntime, DEFAULT_BROKER_URL, DEFAULT_SERVICE};
pub use spec::{
    ActorClass, MachineKind, QuestionnaireSpec, StateName, TransitionMap, WorkflowSpec,
    WorkflowSpecError,
};
pub use specs::{
    bundled_spec_yaml, questionnaire_spec_from_template, questionnaire_spec_from_yaml,
    retainer_intake_questionnaire, retainer_intake_spec, workflow_spec_from_template,
    workflow_spec_from_yaml, BUNDLED_SPEC_YAML, RETAINER_INTAKE_SPEC_YAML,
    RETAINER_INTAKE_TEMPLATE,
};
pub use step::{step_kind_for, StepKind};
pub use trigger::{start_workflow, TriggerError};
