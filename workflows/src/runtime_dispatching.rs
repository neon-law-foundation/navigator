//! In-process worker dispatch wrapper.
//!
//! [`DispatchingRuntime`] composes any [`StateMachineRuntime`] with an
//! [`EmailService`] and a [`cloud::StorageService`] so transitions into
//! an `email_send__<slug>` state dispatch the email *inline*, and
//! transitions into a `document_open__<slug>` state render + persist the
//! document *inline* — no separate `workflows-service` worker process
//! required. Used by:
//!
//! - Local `cargo run -p web` (no Restate broker → falls back to
//!   `InMemoryRuntime`; without this wrapper the welcome email
//!   never fires locally because there's no worker to consume the
//!   step).
//! - The BDD suite (`features/`), which runs the same in-process
//!   path the local dev binary uses.
//!
//! Production keeps the trigger → broker → worker shape: `web` POSTs
//! to Restate Cloud, and `workflows-service` (the actual worker pod)
//! does the dispatch. Wrapping the prod `RestateRuntime` in
//! `DispatchingRuntime` would double-send.

use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use crate::dispatch::{dispatch_step, StepDeps};
use crate::email::EmailService;
use crate::runtime::{SignalContext, StateMachineRuntime, WorkflowEvent, WorkflowRuntimeError};
use crate::spec::{MachineKind, StateName, WorkflowSpec};

/// Wraps an [`InMemoryRuntime`]-style runtime to add in-process
/// dispatch on `email_send__*`, `document_open__*`, and the compliance
/// submission steps (`mailroom_send`, `certified_mail`, `e_filing`,
/// `filing__*`). Delegates everything else.
pub struct DispatchingRuntime {
    inner: Arc<dyn StateMachineRuntime>,
    email: Arc<dyn EmailService>,
    storage: Arc<dyn cloud::StorageService>,
    /// Optional database handle for the compliance-submission side
    /// effect (records a `filings` row). Optional — unlike `storage`
    /// (an `FsStorage` temp dir is cheap), a database needs a Postgres
    /// testcontainer, so the email/document unit tests stay DB-free.
    /// Set via [`DispatchingRuntime::with_db`]; a submission step
    /// reached without a db configured errors clearly.
    db: Option<store::Db>,
}

impl DispatchingRuntime {
    #[must_use]
    pub fn new(
        inner: Arc<dyn StateMachineRuntime>,
        email: Arc<dyn EmailService>,
        storage: Arc<dyn cloud::StorageService>,
    ) -> Self {
        Self {
            inner,
            email,
            storage,
            db: None,
        }
    }

    /// Attach a database handle so compliance-submission steps can
    /// record a `filings` row in-process. Required for any workflow that
    /// reaches a `mailroom_send` / `certified_mail` / `e_filing` /
    /// `filing__*` step.
    #[must_use]
    pub fn with_db(mut self, db: store::Db) -> Self {
        self.db = Some(db);
        self
    }

    /// Run the matching step side effect inline through the shared
    /// [`dispatch_step`] registry — the same arm the `workflows-service`
    /// worker runs inside `ctx.run`. `email_send__*` POSTs through the
    /// wrapped `EmailService`, `document_open__*` renders + persists via
    /// the wrapped `StorageService`, and the submission steps
    /// (`mailroom_send`, `certified_mail`, `e_filing`, `filing__*`)
    /// record a `filings` row through the wrapped database. Every other
    /// state is a no-op.
    ///
    /// Errors surface as [`WorkflowRuntimeError::Transport`] so the
    /// trigger sees a failure shape consistent with the broker path.
    async fn maybe_dispatch(
        &self,
        notation_id: Uuid,
        next: &StateName,
        payload: Option<&str>,
    ) -> Result<(), WorkflowRuntimeError> {
        let deps = StepDeps::new(
            Arc::clone(&self.email),
            Arc::clone(&self.storage),
            self.db.clone(),
        );
        dispatch_step(&deps, notation_id, next, payload)
            .await
            .map_err(|e| WorkflowRuntimeError::Transport(e.to_string()))
    }
}

#[async_trait]
impl StateMachineRuntime for DispatchingRuntime {
    async fn start(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        spec: &WorkflowSpec,
    ) -> Result<(), WorkflowRuntimeError> {
        self.inner.start(kind, notation_id, spec).await
    }

    async fn signal(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        condition: &str,
        payload: Option<&str>,
    ) -> Result<StateName, WorkflowRuntimeError> {
        // Capture the state we're leaving so the matter-close side
        // effect can key off the firm-signature step (the close fires
        // on *leaving* `firm_signature__*`, not on entering the next
        // state, so `maybe_dispatch` — which only sees `next` — can't
        // detect it).
        let from = self.inner.current_state(kind, notation_id).await;
        let next = self
            .inner
            .signal(kind, notation_id, condition, payload)
            .await?;
        self.maybe_dispatch(notation_id, &next, payload).await?;
        // The firm signing the closing letter closes the matter. Mirror
        // of the `workflows-service` worker, so the dev/KIND/test path
        // flips `projects.status` too.
        if matches!(kind, MachineKind::Workflow) {
            if let Some(from) = from.as_ref().filter(|s| crate::closes_matter(s)) {
                let db = self.db.as_ref().ok_or_else(|| {
                    WorkflowRuntimeError::Transport(format!(
                        "closing a matter (leaving {}) requires a database \
                         (DispatchingRuntime::with_db)",
                        from.as_str()
                    ))
                })?;
                crate::close_matter(db, notation_id)
                    .await
                    .map_err(|e| WorkflowRuntimeError::Transport(format!("close matter: {e}")))?;
            }
        }
        Ok(next)
    }

    async fn signal_with_context(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        condition: &str,
        payload: Option<&str>,
        context: SignalContext,
    ) -> Result<StateName, WorkflowRuntimeError> {
        let from = self.inner.current_state(kind, notation_id).await;
        let next = self
            .inner
            .signal_with_context(kind, notation_id, condition, payload, context)
            .await?;
        self.maybe_dispatch(notation_id, &next, payload).await?;
        if matches!(kind, MachineKind::Workflow) {
            if let Some(from) = from.as_ref().filter(|s| crate::closes_matter(s)) {
                let db = self.db.as_ref().ok_or_else(|| {
                    WorkflowRuntimeError::Transport(format!(
                        "closing a matter (leaving {}) requires a database \
                         (DispatchingRuntime::with_db)",
                        from.as_str()
                    ))
                })?;
                crate::close_matter(db, notation_id)
                    .await
                    .map_err(|e| WorkflowRuntimeError::Transport(format!("close matter: {e}")))?;
            }
        }
        Ok(next)
    }

    async fn current_state(&self, kind: MachineKind, notation_id: Uuid) -> Option<StateName> {
        self.inner.current_state(kind, notation_id).await
    }

    async fn events(&self, kind: MachineKind, notation_id: Uuid) -> Vec<WorkflowEvent> {
        self.inner.events(kind, notation_id).await
    }

    async fn start_ephemeral(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        spec: &WorkflowSpec,
    ) -> Result<(), WorkflowRuntimeError> {
        self.inner.start_ephemeral(kind, notation_id, spec).await
    }

    async fn signal_ephemeral(
        &self,
        kind: MachineKind,
        notation_id: Uuid,
        condition: &str,
        payload: Option<&str>,
    ) -> Result<StateName, WorkflowRuntimeError> {
        let next = self
            .inner
            .signal_ephemeral(kind, notation_id, condition, payload)
            .await?;
        self.maybe_dispatch(notation_id, &next, payload).await?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::DispatchingRuntime;
    use crate::document::DocumentPayload;
    use crate::email::welcome::trigger_welcome;
    use crate::email::CapturingEmail;
    use crate::runtime::{InMemoryRuntime, StateMachineRuntime};
    use crate::spec::{MachineKind, StateName, WorkflowSpec};
    use std::sync::Arc;
    use uuid::Uuid;

    async fn fs_storage(suite: &str) -> Arc<dyn cloud::StorageService> {
        Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join(format!("navigator-dispatch-{suite}")))
                .await
                .expect("temp FsStorage"),
        )
    }

    #[tokio::test]
    async fn welcome_trigger_dispatches_email_inline_through_dispatching_runtime() {
        // The trigger does start_ephemeral + 2 signals. The first
        // signal lands on `email_send__welcome`; the wrapper sees
        // that, decodes the payload, and POSTs the welcome through
        // CapturingEmail. End state is `END`.
        let inner: Arc<dyn StateMachineRuntime> = Arc::new(InMemoryRuntime::new());
        let email = Arc::new(CapturingEmail::new());
        let rt = DispatchingRuntime::new(inner, email.clone(), fs_storage("welcome").await);

        let person_id = Uuid::from_u128(13);
        trigger_welcome(&rt, person_id, "Aries", "aries@example.com")
            .await
            .expect("welcome trigger drives through to END");

        let captured = email.captured();
        assert_eq!(captured.len(), 1, "exactly one welcome dispatched");
        assert_eq!(captured[0].to, "aries@example.com");
        assert_eq!(captured[0].subject, "Welcome to Neon Law");
        assert_eq!(captured[0].template_slug.as_deref(), Some("welcome"));

        let final_state = rt.current_state(MachineKind::Workflow, person_id).await;
        assert_eq!(final_state, Some(StateName::end()));
    }

    #[tokio::test]
    async fn non_email_send_transitions_skip_dispatch() {
        // Regression guard: dispatch only fires when the new state's
        // name starts with `email_send__`. A normal workflow transition
        // (e.g. `signature__pending`) must not call EmailService.
        let spec_yaml = r"
BEGIN:
  _: signature__pending
signature__pending:
  signed: END
END: {}
";
        let spec = WorkflowSpec::from_yaml(spec_yaml).unwrap();
        let inner: Arc<dyn StateMachineRuntime> = Arc::new(InMemoryRuntime::new());
        let email = Arc::new(CapturingEmail::new());
        let rt = DispatchingRuntime::new(inner, email.clone(), fs_storage("non-email").await);

        let id = Uuid::from_u128(42);
        rt.start(MachineKind::Workflow, id, &spec).await.unwrap();
        rt.signal(MachineKind::Workflow, id, "_", None)
            .await
            .unwrap();
        assert!(
            email.captured().is_empty(),
            "no welcome on non-email transitions"
        );
    }

    #[tokio::test]
    async fn document_open_transition_renders_and_persists_inline() {
        // A transition landing on `document_open__*` decodes the
        // DocumentPayload from the signal value and renders + persists
        // the PDF through the wrapped StorageService — mirroring the
        // worker's ctx.run dispatch.
        let spec_yaml = r"
BEGIN:
  approved: document_open__retainer_pdf
document_open__retainer_pdf:
  pdf_persisted: END
END: {}
";
        let spec = WorkflowSpec::from_yaml(spec_yaml).unwrap();
        let inner: Arc<dyn StateMachineRuntime> = Arc::new(InMemoryRuntime::new());
        let email = Arc::new(CapturingEmail::new());
        let storage = fs_storage("doc-open").await;
        let rt = DispatchingRuntime::new(inner, email, storage.clone());

        let id = Uuid::from_u128(77);
        let key = format!("notations/{id}/retainer.pdf");
        let payload = serde_json::to_string(&DocumentPayload::Typst {
            storage_key: key.clone(),
            typst_source: "Retainer body.".into(),
        })
        .unwrap();

        rt.start(MachineKind::Workflow, id, &spec).await.unwrap();
        let next = rt
            .signal(MachineKind::Workflow, id, "approved", Some(&payload))
            .await
            .unwrap();
        assert_eq!(next.as_str(), "document_open__retainer_pdf");

        let stored = storage.get(&key).await.expect("PDF persisted inline");
        assert!(stored.bytes.starts_with(b"%PDF"));
    }

    #[tokio::test]
    async fn document_open_without_payload_errors() {
        let spec_yaml = r"
BEGIN:
  approved: document_open__retainer_pdf
document_open__retainer_pdf:
  pdf_persisted: END
END: {}
";
        let spec = WorkflowSpec::from_yaml(spec_yaml).unwrap();
        let inner: Arc<dyn StateMachineRuntime> = Arc::new(InMemoryRuntime::new());
        let email = Arc::new(CapturingEmail::new());
        let rt = DispatchingRuntime::new(inner, email, fs_storage("doc-open-err").await);

        let id = Uuid::from_u128(78);
        rt.start(MachineKind::Workflow, id, &spec).await.unwrap();
        let err = rt
            .signal(MachineKind::Workflow, id, "approved", None)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            crate::runtime::WorkflowRuntimeError::Transport(_)
        ));
    }
}
