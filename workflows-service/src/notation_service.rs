//! Restate virtual-object service exposing the two state-machine
//! timelines per Notation.
//!
//! One Restate virtual object per Notation (the object key is the
//! stringified `notation_id`). Hosts both machine kinds under the
//! same key so signals serialize on a single logical journal —
//! questionnaire and workflow handlers can never interleave for
//! the same Notation.
//!
//! Each `*_signal` handler:
//!   1. Reads the stored spec yaml + current state from Restate's
//!      keyed state (`ctx.get`).
//!   2. Computes the next state by parsing the spec and looking
//!      up the transition.
//!   3. Writes the new state back (`ctx.set`).
//!   4. Inside `ctx.run("append-event", …)`, appends one row to
//!      the `notation_events` Postgres journal.
//!
//! The `ctx.run` wrapper is the load-bearing trick: it makes the
//! Postgres write a Restate-journaled side effect, so a replay
//! reuses the cached row id instead of double-writing.

use std::sync::Arc;

use restate_sdk::prelude::*;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use tracing::Instrument;
use workflows::{
    dispatch_step, dispatches_side_effect, EmailService, MachineKind, QuestionnaireSpec, StateName,
    StepDeps, WorkflowSpec,
};

use crate::journal::{answer_payload, append_event, TransitionRecord};

/// Build a handler span parented on the W3C trace context the caller injected
/// into the ingress POST (see `telemetry`), extracted from this invocation's
/// headers. Lets a `web`-initiated workflow and its durable steps render as one
/// trace. Shared by every Notation handler; a no-op parent (fresh root) when no
/// `traceparent` is present (dev / KIND / OSS forks). `key` is the opaque
/// notation id — an allow-listed id, never client content.
fn traced_handler_span(handler: &'static str, headers: &HeaderMap, key: &str) -> tracing::Span {
    let span = tracing::info_span!("notation.handler", handler = handler, key = %key);
    telemetry::set_span_parent(
        &span,
        headers.get("traceparent").map(String::as_str),
        headers.get("tracestate").map(String::as_str),
    );
    span
}

/// Restate state key for the questionnaire spec yaml.
const QUESTIONNAIRE_SPEC_KEY: &str = "questionnaire_spec_yaml";
/// Restate state key for the questionnaire's current state name.
const QUESTIONNAIRE_STATE_KEY: &str = "questionnaire_state";
/// Restate state key for the workflow spec yaml.
const WORKFLOW_SPEC_KEY: &str = "workflow_spec_yaml";
/// Restate state key for the workflow's current state name.
const WORKFLOW_STATE_KEY: &str = "workflow_state";

/// JSON request body for a `*_start` handler.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StartBody {
    pub spec_yaml: String,
    /// `true` for workflows that have no `notations` row (e.g. the
    /// `onboarding__welcome` trigger). The signal handler skips the
    /// `notation_events` journal append for these so the FK to
    /// `notations` doesn't fire. Defaults to `false` for legacy
    /// callers that always start a notation-backed workflow.
    #[serde(default)]
    pub ephemeral: bool,
}

/// JSON request body for `questionnaire_signal`. The `value`
/// becomes the `answer_value` payload of the journaled event.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuestionnaireSignalBody {
    pub condition: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub acting_person_id: Option<uuid::Uuid>,
}

/// JSON request body for `workflow_signal`. The optional `value`
/// carries a JSON-serialized payload for step dispatch: `email_send__*`
/// reads it as a [`workflows::EmailPayload`] and `document_open__*`
/// reads it as a [`workflows::DocumentPayload`]; other step kinds
/// ignore it. `ephemeral` propagates the flag set at start time so each
/// signal also skips the journal — required because the worker doesn't
/// persist start-side flags across the keyed state today.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowSignalBody {
    pub condition: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub acting_person_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub ephemeral: bool,
}

/// JSON response body for a `*_signal` handler.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignalResponse {
    pub next_state: String,
}

/// JSON response body for `*_current_state`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CurrentStateResponse {
    pub state: Option<String>,
}

#[restate_sdk::object]
#[name = "notation"]
pub trait Notation {
    /// Begin the questionnaire walk for this Notation. Idempotent
    /// — if the spec is already stored, this is a no-op.
    async fn questionnaire_start(body: Json<StartBody>) -> Result<(), HandlerError>;
    /// Advance the questionnaire one step.
    async fn questionnaire_signal(
        body: Json<QuestionnaireSignalBody>,
    ) -> Result<Json<SignalResponse>, HandlerError>;
    #[shared]
    async fn questionnaire_current_state() -> Result<Json<CurrentStateResponse>, HandlerError>;

    /// Begin the post-intake workflow for this Notation.
    /// Idempotent.
    async fn workflow_start(body: Json<StartBody>) -> Result<(), HandlerError>;
    /// Advance the workflow one step.
    async fn workflow_signal(
        body: Json<WorkflowSignalBody>,
    ) -> Result<Json<SignalResponse>, HandlerError>;
    #[shared]
    async fn workflow_current_state() -> Result<Json<CurrentStateResponse>, HandlerError>;
}

/// Service struct held by the Restate endpoint. Carries the shared
/// Postgres handle, the worker-side [`workflows::EmailService`] that
/// `email_send__*` step dispatch routes through, and the
/// [`cloud::StorageService`] that `document_open__*` step dispatch
/// persists rendered PDFs to.
#[derive(Clone)]
pub struct NotationService {
    db: Arc<DatabaseConnection>,
    email: Arc<dyn EmailService>,
    storage: Arc<dyn cloud::StorageService>,
}

impl NotationService {
    #[must_use]
    pub fn new(
        db: DatabaseConnection,
        email: Arc<dyn EmailService>,
        storage: Arc<dyn cloud::StorageService>,
    ) -> Self {
        Self {
            db: Arc::new(db),
            email,
            storage,
        }
    }
}

/// Parse the Restate object key into a notation id.
fn parse_notation_id(key: &str) -> Result<uuid::Uuid, HandlerError> {
    key.parse::<uuid::Uuid>().map_err(|e| {
        HandlerError::from(TerminalError::new(format!(
            "object key `{key}` is not a valid notation_id: {e}"
        )))
    })
}

impl Notation for NotationService {
    async fn questionnaire_start(
        &self,
        ctx: ObjectContext<'_>,
        body: Json<StartBody>,
    ) -> Result<(), HandlerError> {
        let span = traced_handler_span("questionnaire_start", ctx.headers(), ctx.key());
        async move {
            let body = body.0;
            if ctx.get::<String>(QUESTIONNAIRE_SPEC_KEY).await?.is_some() {
                return Ok(()); // idempotent
            }
            // Validate the yaml round-trips before storing so a later
            // signal can't fail mid-flight.
            QuestionnaireSpec::from_yaml(&body.spec_yaml)
                .map_err(|e| TerminalError::new(format!("questionnaire spec: {e}")))?;
            ctx.set(QUESTIONNAIRE_SPEC_KEY, body.spec_yaml);
            ctx.set(QUESTIONNAIRE_STATE_KEY, StateName::BEGIN.to_string());
            Ok(())
        }
        .instrument(span)
        .await
    }

    async fn questionnaire_signal(
        &self,
        ctx: ObjectContext<'_>,
        body: Json<QuestionnaireSignalBody>,
    ) -> Result<Json<SignalResponse>, HandlerError> {
        let span = traced_handler_span("questionnaire_signal", ctx.headers(), ctx.key());
        async move {
            let body = body.0;
            let notation_id = parse_notation_id(ctx.key())?;
            let spec_yaml: String = ctx
                .get::<String>(QUESTIONNAIRE_SPEC_KEY)
                .await?
                .ok_or_else(|| TerminalError::new("questionnaire has not been started"))?;
            let spec = QuestionnaireSpec::from_yaml(&spec_yaml)
                .map_err(|e| TerminalError::new(format!("spec: {e}")))?;
            let from = ctx
                .get::<String>(QUESTIONNAIRE_STATE_KEY)
                .await?
                .unwrap_or_else(|| StateName::BEGIN.to_string());
            let from_state = StateName::from(from.as_str());
            let next = next_state(spec.inner(), &from_state, &body.condition)?;

            ctx.set(QUESTIONNAIRE_STATE_KEY, next.as_str().to_string());

            let db = Arc::clone(&self.db);
            let payload = body.value.as_deref().map(answer_payload);
            let from_str = from.clone();
            let to_str = next.as_str().to_string();
            let condition = body.condition.clone();
            ctx.run(|| async move {
                let recorded_at = chrono::Utc::now().to_rfc3339();
                append_event(
                    db.as_ref(),
                    TransitionRecord {
                        notation_id,
                        acting_person_id: body.acting_person_id,
                        machine_kind: MachineKind::Questionnaire.as_str(),
                        from_state: &from_str,
                        to_state: &to_str,
                        condition: &condition,
                        payload_json: payload,
                        recorded_at: &recorded_at,
                    },
                )
                .await
                .map(|_| ())
                .map_err(|e| HandlerError::from(TerminalError::new(format!("journal: {e}"))))
            })
            .name("append-questionnaire-event")
            .await?;

            Ok(Json(SignalResponse {
                next_state: next.as_str().to_string(),
            }))
        }
        .instrument(span)
        .await
    }

    async fn questionnaire_current_state(
        &self,
        ctx: SharedObjectContext<'_>,
    ) -> Result<Json<CurrentStateResponse>, HandlerError> {
        let span = traced_handler_span("questionnaire_current_state", ctx.headers(), ctx.key());
        async move {
            let state = ctx.get::<String>(QUESTIONNAIRE_STATE_KEY).await?;
            Ok(Json(CurrentStateResponse { state }))
        }
        .instrument(span)
        .await
    }

    async fn workflow_start(
        &self,
        ctx: ObjectContext<'_>,
        body: Json<StartBody>,
    ) -> Result<(), HandlerError> {
        let span = traced_handler_span("workflow_start", ctx.headers(), ctx.key());
        async move {
            let body = body.0;
            if ctx.get::<String>(WORKFLOW_SPEC_KEY).await?.is_some() {
                return Ok(()); // idempotent
            }
            WorkflowSpec::from_yaml(&body.spec_yaml)
                .map_err(|e| TerminalError::new(format!("workflow spec: {e}")))?;
            ctx.set(WORKFLOW_SPEC_KEY, body.spec_yaml);
            ctx.set(WORKFLOW_STATE_KEY, StateName::BEGIN.to_string());
            Ok(())
        }
        .instrument(span)
        .await
    }

    #[allow(clippy::too_many_lines)]
    async fn workflow_signal(
        &self,
        ctx: ObjectContext<'_>,
        body: Json<WorkflowSignalBody>,
    ) -> Result<Json<SignalResponse>, HandlerError> {
        let span = traced_handler_span("workflow_signal", ctx.headers(), ctx.key());
        async move {
            let body = body.0;
            let notation_id = parse_notation_id(ctx.key())?;
            let spec_yaml: String = ctx
                .get::<String>(WORKFLOW_SPEC_KEY)
                .await?
                .ok_or_else(|| TerminalError::new("workflow has not been started"))?;
            let spec = WorkflowSpec::from_yaml(&spec_yaml)
                .map_err(|e| TerminalError::new(format!("spec: {e}")))?;
            let from = ctx
                .get::<String>(WORKFLOW_STATE_KEY)
                .await?
                .unwrap_or_else(|| StateName::BEGIN.to_string());
            let from_state = StateName::from(from.as_str());
            let next = next_state(&spec, &from_state, &body.condition)?;

            ctx.set(WORKFLOW_STATE_KEY, next.as_str().to_string());

            // Ephemeral workflows (e.g. `onboarding__welcome`) have no
            // `notations` row, so the `notation_events` FK to
            // `notations.id` would fire. Skip the journal in that case;
            // the durability for ephemeral steps lives downstream (the
            // `sent_emails` audit row for `email_send__*` dispatch).
            if !body.ephemeral {
                let db = Arc::clone(&self.db);
                let from_str = from.clone();
                let to_str = next.as_str().to_string();
                let condition = body.condition.clone();
                let acting_person_id = body.acting_person_id;
                ctx.run(|| async move {
                    let recorded_at = chrono::Utc::now().to_rfc3339();
                    append_event(
                        db.as_ref(),
                        TransitionRecord {
                            notation_id,
                            acting_person_id,
                            machine_kind: MachineKind::Workflow.as_str(),
                            from_state: &from_str,
                            to_state: &to_str,
                            condition: &condition,
                            payload_json: None,
                            recorded_at: &recorded_at,
                        },
                    )
                    .await
                    .map(|_| ())
                    .map_err(|e| HandlerError::from(TerminalError::new(format!("journal: {e}"))))
                })
                .name("append-workflow-event")
                .await?;
            }

            // Step-kind side effects, routed through the one shared
            // `workflows::dispatch_step` registry so the prod worker and the
            // in-process dev/BDD runtime run the exact same dispatch arm —
            // `email_send__*` (SendGrid), `document_open__*` (render + GCS
            // persist), and the submission steps `mailroom_send` /
            // `certified_mail` / `e_filing` / `filing__*` (a durable
            // `filings` row, guaranteed `staff_review`-gated by the spec).
            //
            // The `ctx.run` boundary stays here, outside the registry, so the
            // side effect is journaled and a replay reuses the cached outcome
            // rather than re-emailing / double-filing / double-writing. We
            // only open the journal entry when the step actually dispatches,
            // so human/wait states (`staff_review`, `_signature`, …) stay
            // journal-free as before.
            if dispatches_side_effect(&next) {
                let deps = StepDeps::new(
                    Arc::clone(&self.email),
                    Arc::clone(&self.storage),
                    Some((*self.db).clone()),
                );
                let state = next.clone();
                let value = body.value.clone();
                ctx.run(|| async move {
                    dispatch_step(&deps, notation_id, &state, value.as_deref())
                        .await
                        .map_err(|e| {
                            HandlerError::from(TerminalError::new(format!("dispatch: {e}")))
                        })
                })
                .name("dispatch-step")
                .await?;
            }

            // The firm signing the closing letter (`firm_signature__*`)
            // closes the matter: flip the bound Project `open` → `closed`.
            // The symmetric bookend to the client-signed retainer that
            // opened it. Journaled so a replay reuses the outcome rather
            // than re-updating; skipped for ephemeral (notation-less)
            // workflows, which never carry a firm-signature step.
            if !body.ephemeral && workflows::closes_matter(&from_state) {
                let db = Arc::clone(&self.db);
                ctx.run(|| async move {
                    workflows::close_matter(db.as_ref(), notation_id)
                        .await
                        .map_err(|e| HandlerError::from(TerminalError::new(format!("close: {e}"))))
                })
                .name("close-matter")
                .await?;
            }

            Ok(Json(SignalResponse {
                next_state: next.as_str().to_string(),
            }))
        }
        .instrument(span)
        .await
    }

    async fn workflow_current_state(
        &self,
        ctx: SharedObjectContext<'_>,
    ) -> Result<Json<CurrentStateResponse>, HandlerError> {
        let span = traced_handler_span("workflow_current_state", ctx.headers(), ctx.key());
        async move {
            let state = ctx.get::<String>(WORKFLOW_STATE_KEY).await?;
            Ok(Json(CurrentStateResponse { state }))
        }
        .instrument(span)
        .await
    }
}

/// Pure state-machine step: given a spec, a current state, and a
/// condition, return the next state name. Surface as a
/// `TerminalError` (caller-visible failure, not Restate-retryable)
/// on `AlreadyTerminal`, `NoTransition`, or `UnknownState`, since
/// those are deterministic outcomes that a replay would reproduce.
fn next_state(
    spec: &WorkflowSpec,
    from: &StateName,
    condition: &str,
) -> Result<StateName, HandlerError> {
    if spec.is_terminal(from) {
        return Err(TerminalError::new(format!("`{}` is terminal", from.as_str())).into());
    }
    let transitions = spec.transitions_from(from).ok_or_else(|| {
        HandlerError::from(TerminalError::new(format!(
            "unknown state `{}`",
            from.as_str()
        )))
    })?;
    transitions.lookup(condition).cloned().ok_or_else(|| {
        HandlerError::from(TerminalError::new(format!(
            "no transition from `{}` on condition `{condition}`",
            from.as_str()
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::next_state;
    use workflows::{StateName, WorkflowSpec};

    const SPEC: &str = r"
BEGIN:
  _: client_name
client_name:
  _: client_email
client_email:
  _: END
END: {}
";

    #[test]
    fn next_state_advances_through_a_questionnaire_walk() {
        let spec = WorkflowSpec::from_yaml(SPEC).unwrap();
        let n = next_state(&spec, &StateName::begin(), "_").unwrap();
        assert_eq!(n.as_str(), "client_name");
        let n = next_state(&spec, &n, "_").unwrap();
        assert_eq!(n.as_str(), "client_email");
        let n = next_state(&spec, &n, "_").unwrap();
        assert_eq!(n, StateName::end());
    }

    #[test]
    fn next_state_returns_terminal_error_on_already_end() {
        let spec = WorkflowSpec::from_yaml(SPEC).unwrap();
        let err = next_state(&spec, &StateName::end(), "_").unwrap_err();
        // HandlerError implements Debug only; format that for the
        // message assertion.
        assert!(format!("{err:?}").contains("terminal"));
    }

    #[test]
    fn next_state_returns_terminal_error_on_unknown_condition() {
        let spec = WorkflowSpec::from_yaml(SPEC).unwrap();
        let err = next_state(&spec, &StateName::begin(), "bogus").unwrap_err();
        assert!(format!("{err:?}").contains("no transition"));
    }
}
