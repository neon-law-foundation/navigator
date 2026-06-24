//! Cucumber runner for `features/esignature_webhook.feature`.
//!
//! Drives the inbound e-signature completion webhook
//! (`web::esignature_webhook`) against an in-memory runtime: a retainer
//! is parked at `sent_for_signature__pending` with a known envelope id,
//! then the provider's completion callback is POSTed — once validly
//! signed (advances to END), once with a forged signature (rejected,
//! stays pending).

#![allow(clippy::unused_async)]
#![allow(clippy::doc_markdown)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cucumber::{given, then, when, World};
use features::{app_state, body_string, fs_storage, in_memory_db};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use store::{entity, seed, Db};
use tower::ServiceExt;
use uuid::Uuid;
use web::webhook_auth::sign_hmac_sha256_b64;
use web::{policy::PolicyClient, SessionStore};
use workflows::{InMemoryRuntime, MachineKind, StateMachineRuntime};

const TEMPLATE_CODE: &str = "onboarding__retainer";
const HMAC_KEY: &str = "test-docusign-hmac-key";
const PARKED: &str = "sent_for_signature__pending";

/// The DocuSign Connect completion payload for one envelope.
fn completion_body(envelope_id: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "event": "envelope-completed",
        "data": {
            "envelopeId": envelope_id,
            "envelopeSummary": { "status": "completed" },
        },
    }))
    .unwrap()
}

#[derive(Default, World)]
#[world(init = Self::default)]
struct WebhookWorld {
    app: Option<axum::Router>,
    db: Option<Db>,
    runtime: Option<Arc<InMemoryRuntime>>,
    notation_id: Option<Uuid>,
    last_status: Option<StatusCode>,
    last_body: String,
}

impl std::fmt::Debug for WebhookWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebhookWorld")
            .field("notation_id", &self.notation_id)
            .field("last_status", &self.last_status)
            .finish_non_exhaustive()
    }
}

impl WebhookWorld {
    fn app(&self) -> axum::Router {
        self.app.as_ref().expect("app not built").clone()
    }
    fn db(&self) -> &Db {
        self.db.as_ref().expect("db not built")
    }
    fn runtime(&self) -> &Arc<InMemoryRuntime> {
        self.runtime.as_ref().expect("runtime not built")
    }
    fn notation_id(&self) -> Uuid {
        self.notation_id.expect("notation not built")
    }
}

#[given("a Navigator app with an HMAC-secured e-signature webhook")]
async fn build_app(world: &mut WebhookWorld) {
    let db = in_memory_db().await;
    let storage = fs_storage("esignature-webhook").await;
    seed::seed_canonical(&db, &storage)
        .await
        .expect("seed canonical");
    let runtime = Arc::new(InMemoryRuntime::new());
    let mut state = app_state(
        db.clone(),
        runtime.clone(),
        storage,
        PolicyClient::passthrough(),
        None,
        SessionStore::new("test-session-key-not-for-production"),
    );
    // Arm the HMAC gate so the webhook actually verifies signatures —
    // without this the dev posture would accept the forged callback.
    state.esignature_hmac_key = Some(HMAC_KEY.to_string());
    let router = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    world.app = Some(router);
    world.db = Some(db);
    world.runtime = Some(runtime);
}

#[given(regex = r#"^a retainer parked at sent_for_signature__pending with envelope id "([^"]+)"$"#)]
async fn park_retainer(world: &mut WebhookWorld, envelope_id: String) {
    let db = world.db().clone();
    let tmpl = entity::template::Entity::find()
        .filter(entity::template::Column::Code.eq(TEMPLATE_CODE))
        .one(&db)
        .await
        .unwrap()
        .expect("seed_canonical inserts onboarding__retainer");
    let person = entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&db).await;
    let proj = entity::project::ActiveModel {
        name: ActiveValue::Set("retainer matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let notation = entity::notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(person.id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(proj.id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let notation_id = notation.id;

    // Drive the workflow timeline to the parked state through the same
    // in-memory runtime the webhook will later signal. The retainer
    // engagement is attorney-reviewed at staff_review before reaching
    // the signature wait — mirror that path exactly.
    let rt = world.runtime().as_ref();
    let spec = workflows::retainer_intake_spec();
    StateMachineRuntime::start(rt, MachineKind::Workflow, notation_id, &spec)
        .await
        .unwrap();
    for condition in [
        "intake_submitted",
        "retainer_rendered",
        "approved",
        "pdf_persisted",
    ] {
        StateMachineRuntime::signal(rt, MachineKind::Workflow, notation_id, condition, None)
            .await
            .unwrap();
    }
    assert_eq!(
        StateMachineRuntime::current_state(rt, MachineKind::Workflow, notation_id)
            .await
            .unwrap()
            .as_str(),
        PARKED,
        "workflow should be parked before the callback"
    );

    // Persist the parked state + the provider's envelope id, exactly as
    // the retainer walk does at send time.
    let mut active: entity::notation::ActiveModel = notation.into();
    active.state = ActiveValue::Set(PARKED.into());
    active.signature_request_id = ActiveValue::Set(Some(envelope_id));
    active.update(&db).await.unwrap();

    world.notation_id = Some(notation_id);
}

async fn post_callback(world: &mut WebhookWorld, envelope_id: &str, signature: &str) {
    let body = completion_body(envelope_id);
    let resp = world
        .app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/esignature/any-path-token")
                .header("content-type", "application/json")
                .header("x-docusign-signature-1", signature)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    world.last_status = Some(resp.status());
    world.last_body = body_string(resp).await;
}

#[when(
    regex = r#"^the provider posts a validly-signed completion callback for envelope "([^"]+)"$"#
)]
async fn post_valid(world: &mut WebhookWorld, envelope_id: String) {
    let signature = sign_hmac_sha256_b64(HMAC_KEY.as_bytes(), &completion_body(&envelope_id));
    post_callback(world, &envelope_id, &signature).await;
}

#[when(
    regex = r#"^an attacker posts a completion callback with a forged signature for envelope "([^"]+)"$"#
)]
async fn post_forged(world: &mut WebhookWorld, envelope_id: String) {
    // A plausible-looking but wrong base64 digest.
    post_callback(world, &envelope_id, "Zm9yZ2VkLXNpZ25hdHVyZS1ub3QtdmFsaWQ=").await;
}

#[then(regex = r"^the response status is (\d+)$")]
async fn assert_status(world: &mut WebhookWorld, code: u16) {
    assert_eq!(
        world.last_status.expect("no status captured").as_u16(),
        code,
        "body: {}",
        world.last_body
    );
}

#[then(regex = r#"^the retainer workflow has advanced to "([^"]+)"$"#)]
async fn assert_advanced(world: &mut WebhookWorld, state: String) {
    let events = StateMachineRuntime::events(
        world.runtime().as_ref(),
        MachineKind::Workflow,
        world.notation_id(),
    )
    .await;
    let last = events.last().expect("at least one transition");
    assert_eq!(last.to.as_str(), state, "events: {events:?}");
}

#[then(regex = r#"^the retainer workflow is still at "([^"]+)"$"#)]
async fn assert_still_at(world: &mut WebhookWorld, state: String) {
    let current = StateMachineRuntime::current_state(
        world.runtime().as_ref(),
        MachineKind::Workflow,
        world.notation_id(),
    )
    .await
    .expect("workflow exists");
    assert_eq!(current.as_str(), state);
}

#[then(regex = r#"^the notation row state is "([^"]+)"$"#)]
async fn assert_row_state(world: &mut WebhookWorld, state: String) {
    let row = entity::notation::Entity::find_by_id(world.notation_id())
        .one(world.db())
        .await
        .unwrap()
        .expect("notation row");
    assert_eq!(row.state, state);
}

#[tokio::main]
async fn main() {
    WebhookWorld::cucumber()
        .run("tests/features/esignature_webhook.feature")
        .await;
}
