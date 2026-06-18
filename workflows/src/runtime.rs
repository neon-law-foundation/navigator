//! Durable-runtime abstraction — Restate today, anything that
//! implements [`StateMachineRuntime`] tomorrow.
//!
//! A Notation runs *two* state machines back-to-back: a
//! questionnaire walker that asks one question per signal, then
//! the post-intake workflow that drives staff review, signing,
//! mailroom, and so on. Both share the same wire shape (a
//! [`crate::WorkflowSpec`] graph of named states and condition-keyed
//! transitions); the [`MachineKind`] arg partitions the two timelines
//! at the trait level so the application can't accidentally cross
//! the streams. The runtime key is therefore
//! `(MachineKind, notation_id)`.
//!
//! The crate stays runtime-agnostic so the rest of the application
//! depends only on this trait. A Restate adapter that uses the
//! `restate-sdk` crate's `Context` to record each transition as a
//! `ctx.run()` side effect (so reruns are deterministic) plugs in
//! behind the same surface. The shipped `InMemoryRuntime` is what
//! tests and local-dev mode use — it records events in a `Vec`
//! and accepts external `signal`s synchronously.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::spec::{MachineKind, StateName, WorkflowSpec};
use crate::step::step_kind_for;

/// One immutable record of "this notation moved from <previous>
/// to <current> via <condition>". The durable runtime persists
/// these so a crash + replay reaches the same terminal state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowEvent {
    pub notation_id: Uuid,
    pub from: StateName,
    pub to: StateName,
    pub condition: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkflowRuntimeError {
    #[error("unknown state `{0:?}`")]
    UnknownState(StateName),
    #[error("no transition from `{from:?}` on condition `{condition}`")]
    NoTransition { from: StateName, condition: String },
    #[error("state `{0:?}` is terminal and accepts no further signals")]
    AlreadyTerminal(StateName),
    #[error("notation `{0}` not found")]
    UnknownNotation(Uuid),
    #[error("transport: {0}")]
    Transport(String),
}

/// Generalized durable runtime keyed by `(MachineKind,
/// notation_id)`. One service can host both the questionnaire
/// walker and the workflow runner per Notation; the [`MachineKind`]
/// arg picks which timeline a call targets.
#[async_trait]
pub trait StateMachineRuntime: Send + Sync {
    /// Start a new state-machine instance keyed by `(kind,
    /// notation_id)`. Idempotent: starting the same key twice is a
    /// no-op.
    async fn start(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        spec: &WorkflowSpec,
    ) -> Result<(), WorkflowRuntimeError>;

    /// Send an external signal (`condition`) for `(kind,
    /// notation_id)`. Advances the state machine if the current
    /// state has a transition keyed by that condition. `payload`,
    /// when present, is carried as the journaled event's
    /// `answer_value` — questionnaire signals stamp the
    /// respondent's answer here; workflow signals never have one
    /// and pass `None`.
    async fn signal(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        condition: &str,
        payload: Option<&str>,
    ) -> Result<StateName, WorkflowRuntimeError>;

    /// Current state for `(kind, notation_id)`, or `None` if it
    /// was never started.
    async fn current_state(&self, kind: MachineKind, notation_id: Uuid) -> Option<StateName>;

    /// Replay-friendly event log. Production adapters return the
    /// list from the underlying durable store.
    async fn events(&self, kind: MachineKind, notation_id: Uuid) -> Vec<WorkflowEvent>;

    /// Ephemeral variant of [`Self::start`] — used by workflows that
    /// don't have a `notations` row (the `onboarding__welcome`
    /// trigger is the canonical example). Durable adapters set a
    /// wire-level `ephemeral: true` flag so the worker skips the
    /// `notation_events` journal append (the FK to `notations`
    /// would otherwise fire). The default impl simply forwards to
    /// `start`, since in-memory runtimes don't journal anyway.
    async fn start_ephemeral(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        spec: &WorkflowSpec,
    ) -> Result<(), WorkflowRuntimeError> {
        self.start(kind, notation_id, spec).await
    }

    /// Ephemeral variant of [`Self::signal`]. See
    /// [`Self::start_ephemeral`] for the rationale.
    async fn signal_ephemeral(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        condition: &str,
        payload: Option<&str>,
    ) -> Result<StateName, WorkflowRuntimeError> {
        self.signal(kind, notation_id, condition, payload).await
    }
}

/// In-memory runtime used by tests and local dev. Persists nothing;
/// reset on each process start.
#[derive(Default, Clone)]
pub struct InMemoryRuntime {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    /// `(kind, notation_id) -> (spec, current_state, history)`.
    instances: std::collections::HashMap<
        (MachineKind, Uuid),
        (WorkflowSpec, StateName, Vec<WorkflowEvent>),
    >,
}

impl InMemoryRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquire the inner mutex, treating poisoning as recoverable.
    /// A poisoned lock means *some other* thread panicked while
    /// holding it — that thread's contribution to the state may be
    /// inconsistent, but the runtime as a whole is a per-process
    /// dev/test scratchpad and refusing to lock would cascade the
    /// panic to every concurrent caller.
    fn lock_inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait]
impl StateMachineRuntime for InMemoryRuntime {
    async fn start(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        spec: &WorkflowSpec,
    ) -> Result<(), WorkflowRuntimeError> {
        let mut inner = self.lock_inner();
        inner
            .instances
            .entry((kind, notation_id))
            .or_insert_with(|| (spec.clone(), StateName::begin(), Vec::new()));
        Ok(())
    }

    async fn signal(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        condition: &str,
        // Ignored: `WorkflowEvent` has no payload column. The Postgres
        // projection lives in `RestateRuntime`; this runtime exists
        // for transition-order tests.
        _payload: Option<&str>,
    ) -> Result<StateName, WorkflowRuntimeError> {
        let mut inner = self.lock_inner();
        let (spec, state, history) = inner
            .instances
            .get_mut(&(kind, notation_id))
            .ok_or(WorkflowRuntimeError::UnknownNotation(notation_id))?;

        if spec.is_terminal(state) {
            return Err(WorkflowRuntimeError::AlreadyTerminal(state.clone()));
        }
        let transitions = spec
            .transitions_from(state)
            .ok_or_else(|| WorkflowRuntimeError::UnknownState(state.clone()))?;
        let next = transitions
            .lookup(condition)
            .ok_or_else(|| WorkflowRuntimeError::NoTransition {
                from: state.clone(),
                condition: condition.to_string(),
            })?
            .clone();
        let _kind = step_kind_for(&next); // for tracing in a real adapter
        history.push(WorkflowEvent {
            notation_id,
            from: state.clone(),
            to: next.clone(),
            condition: condition.to_string(),
        });
        *state = next.clone();
        Ok(next)
    }

    async fn current_state(&self, kind: MachineKind, notation_id: Uuid) -> Option<StateName> {
        self.lock_inner()
            .instances
            .get(&(kind, notation_id))
            .map(|(_, s, _)| s.clone())
    }

    async fn events(&self, kind: MachineKind, notation_id: Uuid) -> Vec<WorkflowEvent> {
        self.lock_inner()
            .instances
            .get(&(kind, notation_id))
            .map(|(_, _, h)| h.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{InMemoryRuntime, StateMachineRuntime, WorkflowRuntimeError};
    use crate::spec::{MachineKind, QuestionnaireSpec, StateName, WorkflowSpec};
    use uuid::Uuid;

    /// Stable test UUIDs so signal/event assertions stay readable
    /// without splattering `Uuid::from_u128(_)` over every line.
    const N1: Uuid = Uuid::from_u128(1);
    const N7: Uuid = Uuid::from_u128(7);
    const N42: Uuid = Uuid::from_u128(42);
    const N999: Uuid = Uuid::from_u128(999);

    const SPEC: &str = r"
BEGIN:
  created: staff_review__for_grantor
staff_review__for_grantor:
  approve: notarization__for_grantor
  reject: END
notarization__for_grantor:
  signed: END
  refused: END
END: {}
";

    const QUESTIONNAIRE: &str = r"
BEGIN:
  _: client_name
client_name:
  _: client_email
client_email:
  _: END
END: {}
";

    fn spec() -> WorkflowSpec {
        WorkflowSpec::from_yaml(SPEC).unwrap()
    }

    fn questionnaire() -> QuestionnaireSpec {
        QuestionnaireSpec::from_yaml(QUESTIONNAIRE).unwrap()
    }

    #[tokio::test]
    async fn start_initializes_at_begin() {
        let rt = InMemoryRuntime::new();
        StateMachineRuntime::start(&rt, MachineKind::Workflow, N1, &spec())
            .await
            .unwrap();
        assert_eq!(
            StateMachineRuntime::current_state(&rt, MachineKind::Workflow, N1).await,
            Some(StateName::begin())
        );
    }

    #[tokio::test]
    async fn signal_advances_through_the_state_machine_to_end() {
        let rt = InMemoryRuntime::new();
        let s = spec();
        StateMachineRuntime::start(&rt, MachineKind::Workflow, N1, &s)
            .await
            .unwrap();
        let next = StateMachineRuntime::signal(&rt, MachineKind::Workflow, N1, "created", None)
            .await
            .unwrap();
        assert_eq!(next.as_str(), "staff_review__for_grantor");
        let next = StateMachineRuntime::signal(&rt, MachineKind::Workflow, N1, "approve", None)
            .await
            .unwrap();
        assert_eq!(next.as_str(), "notarization__for_grantor");
        let next = StateMachineRuntime::signal(&rt, MachineKind::Workflow, N1, "signed", None)
            .await
            .unwrap();
        assert_eq!(next, StateName::end());
    }

    #[tokio::test]
    async fn signal_records_every_transition_in_the_event_log() {
        let rt = InMemoryRuntime::new();
        let s = spec();
        StateMachineRuntime::start(&rt, MachineKind::Workflow, N1, &s)
            .await
            .unwrap();
        StateMachineRuntime::signal(&rt, MachineKind::Workflow, N1, "created", None)
            .await
            .unwrap();
        StateMachineRuntime::signal(&rt, MachineKind::Workflow, N1, "approve", None)
            .await
            .unwrap();
        let events = StateMachineRuntime::events(&rt, MachineKind::Workflow, N1).await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].from, StateName::begin());
        assert_eq!(events[0].to.as_str(), "staff_review__for_grantor");
        assert_eq!(events[0].condition, "created");
        assert_eq!(events[1].to.as_str(), "notarization__for_grantor");
    }

    #[tokio::test]
    async fn signal_with_unknown_condition_returns_no_transition() {
        let rt = InMemoryRuntime::new();
        let s = spec();
        StateMachineRuntime::start(&rt, MachineKind::Workflow, N1, &s)
            .await
            .unwrap();
        let err = StateMachineRuntime::signal(&rt, MachineKind::Workflow, N1, "bogus", None)
            .await
            .unwrap_err();
        assert!(matches!(err, WorkflowRuntimeError::NoTransition { .. }));
    }

    #[tokio::test]
    async fn signal_after_reaching_terminal_state_errors() {
        let rt = InMemoryRuntime::new();
        let s = spec();
        StateMachineRuntime::start(&rt, MachineKind::Workflow, N1, &s)
            .await
            .unwrap();
        StateMachineRuntime::signal(&rt, MachineKind::Workflow, N1, "created", None)
            .await
            .unwrap();
        StateMachineRuntime::signal(&rt, MachineKind::Workflow, N1, "reject", None)
            .await
            .unwrap();
        assert_eq!(
            StateMachineRuntime::current_state(&rt, MachineKind::Workflow, N1).await,
            Some(StateName::end())
        );
        let err = StateMachineRuntime::signal(&rt, MachineKind::Workflow, N1, "anything", None)
            .await
            .unwrap_err();
        assert!(matches!(err, WorkflowRuntimeError::AlreadyTerminal(_)));
    }

    #[tokio::test]
    async fn signal_for_unknown_notation_errors() {
        let rt = InMemoryRuntime::new();
        let err = StateMachineRuntime::signal(&rt, MachineKind::Workflow, N999, "anything", None)
            .await
            .unwrap_err();
        assert!(matches!(err, WorkflowRuntimeError::UnknownNotation(id) if id == N999));
    }

    #[tokio::test]
    async fn start_is_idempotent_for_same_id() {
        let rt = InMemoryRuntime::new();
        let s = spec();
        StateMachineRuntime::start(&rt, MachineKind::Workflow, N1, &s)
            .await
            .unwrap();
        StateMachineRuntime::signal(&rt, MachineKind::Workflow, N1, "created", None)
            .await
            .unwrap();
        // Starting again must not reset the state.
        StateMachineRuntime::start(&rt, MachineKind::Workflow, N1, &s)
            .await
            .unwrap();
        assert_eq!(
            StateMachineRuntime::current_state(&rt, MachineKind::Workflow, N1)
                .await
                .unwrap()
                .as_str(),
            "staff_review__for_grantor"
        );
    }

    #[tokio::test]
    async fn questionnaire_and_workflow_for_same_notation_id_are_isolated_timelines() {
        // The Council of Twelve consensus: one Restate service per
        // notation, two MachineKinds keyed independently. Verify
        // in-memory: signaling the workflow machine does not advance
        // the questionnaire, and vice versa.
        let rt = InMemoryRuntime::new();
        let q = questionnaire();
        let w = spec();
        StateMachineRuntime::start(&rt, MachineKind::Questionnaire, N42, q.inner())
            .await
            .unwrap();
        StateMachineRuntime::start(&rt, MachineKind::Workflow, N42, &w)
            .await
            .unwrap();

        // Advance the questionnaire one step.
        let q_next =
            StateMachineRuntime::signal(&rt, MachineKind::Questionnaire, N42, "_", Some("Libra"))
                .await
                .unwrap();
        assert_eq!(q_next.as_str(), "client_name");

        // The workflow machine is still at BEGIN.
        assert_eq!(
            StateMachineRuntime::current_state(&rt, MachineKind::Workflow, N42).await,
            Some(StateName::begin())
        );
        // And the questionnaire is at client_name.
        assert_eq!(
            StateMachineRuntime::current_state(&rt, MachineKind::Questionnaire, N42).await,
            Some(StateName::from("client_name"))
        );
    }

    #[tokio::test]
    async fn questionnaire_walks_from_begin_to_end_via_underscore() {
        let rt = InMemoryRuntime::new();
        let q = questionnaire();
        StateMachineRuntime::start(&rt, MachineKind::Questionnaire, N1, q.inner())
            .await
            .unwrap();
        let s =
            StateMachineRuntime::signal(&rt, MachineKind::Questionnaire, N1, "_", Some("Libra"))
                .await
                .unwrap();
        assert_eq!(s.as_str(), "client_name");
        let s = StateMachineRuntime::signal(
            &rt,
            MachineKind::Questionnaire,
            N1,
            "_",
            Some("libra@example.com"),
        )
        .await
        .unwrap();
        assert_eq!(s.as_str(), "client_email");
        let s = StateMachineRuntime::signal(&rt, MachineKind::Questionnaire, N1, "_", None)
            .await
            .unwrap();
        assert_eq!(s, StateName::end());
        // Events recorded under the questionnaire key only.
        let events = StateMachineRuntime::events(&rt, MachineKind::Questionnaire, N1).await;
        assert_eq!(events.len(), 3);
        // Workflow timeline saw nothing.
        assert!(StateMachineRuntime::events(&rt, MachineKind::Workflow, N1)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn state_machine_runtime_keys_isolate_kinds_at_the_trait_level() {
        // Same id, different MachineKind — two separate instances.
        let rt = InMemoryRuntime::new();
        let s = spec();
        StateMachineRuntime::start(&rt, MachineKind::Workflow, N7, &s)
            .await
            .unwrap();
        assert_eq!(
            StateMachineRuntime::current_state(&rt, MachineKind::Workflow, N7).await,
            Some(StateName::begin())
        );
        assert_eq!(
            StateMachineRuntime::current_state(&rt, MachineKind::Questionnaire, N7).await,
            None
        );
    }
}
