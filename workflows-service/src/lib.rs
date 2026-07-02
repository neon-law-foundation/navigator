//! Restate worker for the `notation` virtual object.
//!
//! Each Notation runs *two* state machines back-to-back: a
//! questionnaire walker followed by the post-intake workflow.
//! This crate hosts them as a single Restate virtual-object
//! service keyed by `notation_id`, so signals to either timeline
//! serialize on one logical journal and Postgres-side projections
//! land in the same atomic-with-respect-to-replay envelope.
//!
//! The lib exports the service trait + impl for integration tests
//! and a `main.rs` binary that boots the HTTP server Restate
//! discovers and dispatches into.
//!
//! See `docs/glossary.md` (Workflow Runtime, Restate, Durable
//! execution) for the architectural arc.

pub mod email_config;
pub mod heartbeat;
pub mod journal;
pub mod notation_service;
pub mod notify_config;
pub mod project_provisioning;
pub mod registry;

pub use email_config::{from_env as email_from_env, EmailConfigError};
pub use notation_service::{
    CurrentStateResponse, NotationService, QuestionnaireSignalBody, SignalResponse, StartBody,
    WorkflowSignalBody,
};
pub use notify_config::from_env as notifier_from_env;
pub use project_provisioning::{ProjectProvisioning, ProjectProvisioningService};

#[cfg(test)]
mod machine_kind_token_tests {
    // The `notation_events.machine_kind` column stores string tokens
    // produced by two declarations: `workflows::MachineKind::as_str`
    // (the runtime enum) and `store::entity::notation_event::MACHINE_*`
    // (the constants the SQL projection's filter queries use). If
    // they ever drift, the projection silently misses rows because
    // `Column::MachineKind.eq(...)` no longer matches what the worker
    // wrote. This crate is the single place that depends on both
    // sides, so the equality check belongs here.
    use store::entity::notation_event::{MACHINE_QUESTIONNAIRE, MACHINE_WORKFLOW};
    use workflows::MachineKind;

    #[test]
    fn workflows_enum_tokens_equal_store_constants() {
        assert_eq!(MachineKind::Questionnaire.as_str(), MACHINE_QUESTIONNAIRE);
        assert_eq!(MachineKind::Workflow.as_str(), MACHINE_WORKFLOW);
    }
}
