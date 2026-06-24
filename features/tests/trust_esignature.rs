//! Cucumber runner for `features/trust_esignature.feature`.
//!
//! Walks a `trusts__nevada` notation through the admin walker and the
//! generalized post-questionnaire send path (the same one the retainer
//! uses), proving e-signature is no longer retainer-specific: the trust
//! parks at `sent_for_signature__pending` with a provider envelope id,
//! and the rendered trust instrument carries the real-property
//! notarization caveat. A sibling `web/tests/trust_esignature_loop.rs`
//! proves the anchor strings reach the `DocuSign` wire.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cucumber::{gherkin::Step, given, then, when, World};
use features::{app_state, body_string, form_encode, fs_storage, in_memory_db};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use store::{entity, seed, Db};
use tower::ServiceExt;
use uuid::Uuid;
use web::{policy::PolicyClient, SessionStore};
use workflows::{bundled_spec_yaml, workflow_spec_from_yaml, InMemoryRuntime, StateName};

const TEMPLATE_CODE: &str = "trusts__nevada";

#[derive(Default, World)]
#[world(init = Self::default)]
struct TrustWorld {
    app: Option<axum::Router>,
    db: Option<Db>,
    notation_id: Option<Uuid>,
    last_body: String,
    final_status: Option<StatusCode>,
}

impl std::fmt::Debug for TrustWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrustWorld")
            .field("notation_id", &self.notation_id)
            .field("final_status", &self.final_status)
            .finish_non_exhaustive()
    }
}

impl TrustWorld {
    fn app(&self) -> axum::Router {
        self.app.as_ref().expect("app not built").clone()
    }

    fn db(&self) -> &Db {
        self.db.as_ref().expect("db not built")
    }

    fn notation_id(&self) -> Uuid {
        self.notation_id.expect("notation_id not built")
    }
}

#[given("a fresh Navigator app with the canonical templates seeded")]
async fn build_app(world: &mut TrustWorld) {
    let db = in_memory_db().await;
    let storage = fs_storage("trust-esign").await;
    seed::seed_canonical(&db, &storage)
        .await
        .expect("seed canonical");
    let runtime = Arc::new(InMemoryRuntime::new());
    let state = app_state(
        db.clone(),
        runtime,
        storage,
        PolicyClient::passthrough(),
        None,
        SessionStore::new("test-session-key-not-for-production"),
    );
    let router = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    world.app = Some(router);
    world.db = Some(db);
}

#[given(regex = r#"^a trust notation for the settlor "([^"]+)" <([^>]+)>$"#)]
async fn seed_notation(world: &mut TrustWorld, name: String, email: String) {
    let db = world.db().clone();
    let tmpl = entity::template::Entity::find()
        .filter(entity::template::Column::Code.eq(TEMPLATE_CODE))
        .one(&db)
        .await
        .unwrap()
        .expect("seed_canonical inserts trusts__nevada");
    let person = entity::person::ActiveModel {
        name: ActiveValue::Set(name),
        email: ActiveValue::Set(email),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&db).await;
    let proj = entity::project::ActiveModel {
        name: ActiveValue::Set("trust matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let notation_id = entity::notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(person.id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(proj.id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    world.notation_id = Some(notation_id);
}

#[when("the settlor walks the trust questionnaire:")]
async fn walk_questionnaire(world: &mut TrustWorld, step: &Step) {
    let table = step.table.as_ref().expect("expected a data table");
    let nid = world.notation_id();
    let mut last_status = StatusCode::OK;
    let mut last_body = String::new();
    // First row is the header (`value`); skip it.
    for row in table.rows.iter().skip(1) {
        let value = row.first().expect("each row carries one cell").as_str();
        let body = format!("value={}", form_encode(value));
        let resp = world
            .app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/portal/admin/notations/{nid}/step"))
                    .header("authorization", "Bearer dev")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        last_status = resp.status();
        last_body = body_string(resp).await;
    }
    world.final_status = Some(last_status);
    world.last_body = last_body;
}

#[then(regex = r"^the final response status is (\d+)$")]
async fn assert_final_status(world: &mut TrustWorld, code: u16) {
    assert_eq!(
        world
            .final_status
            .expect("no final status captured")
            .as_u16(),
        code,
        "body: {}",
        world.last_body
    );
}

#[then(regex = r#"^the trust notation workflow state is "([^"]+)"$"#)]
async fn assert_notation_state(world: &mut TrustWorld, expected: String) {
    let row = entity::notation::Entity::find_by_id(world.notation_id())
        .one(world.db())
        .await
        .unwrap()
        .expect("notation row");
    assert_eq!(row.state, expected);
}

#[then("the trust notation has a signature request id")]
async fn assert_signature_request_id(world: &mut TrustWorld) {
    let row = entity::notation::Entity::find_by_id(world.notation_id())
        .one(world.db())
        .await
        .unwrap()
        .expect("notation row");
    assert!(
        row.signature_request_id.is_some(),
        "the generalized send path must have stamped a provider envelope id"
    );
}

#[then(regex = r#"^the rendered trust names the trustee "([^"]+)"$"#)]
async fn assert_rendered_trustee(world: &mut TrustWorld, name: String) {
    assert!(
        world.last_body.contains(&name),
        "expected the rendered trust to name {name:?}, got:\n{}",
        world.last_body
    );
}

#[then("the rendered trust states the real-property notarization caveat")]
async fn assert_notarization_caveat(world: &mut TrustWorld) {
    // The funding caveat must survive into the rendered document so a
    // settlor can't mistake a signed trust for funded real property.
    assert!(
        world.last_body.contains("real property"),
        "the rendered trust must mention real property funding:\n{}",
        world.last_body
    );
    assert!(
        world
            .last_body
            .contains("recorded with the county recorder"),
        "the rendered trust must state the recordable-deed caveat:\n{}",
        world.last_body
    );
}

#[then("the trusts__nevada workflow routes:")]
async fn assert_workflow_routes(_world: &mut TrustWorld, step: &Step) {
    let yaml = bundled_spec_yaml(TEMPLATE_CODE).expect("trusts__nevada has a bundled spec");
    let spec = workflow_spec_from_yaml(yaml).expect("trust workflow spec parses");
    let table = step.table.as_ref().expect("scenario has a data table");
    for row in table.rows.iter().skip(1) {
        let from = StateName::from(row.first().expect("from cell").as_str());
        let condition = row.get(1).expect("condition cell").as_str();
        let to = row.get(2).expect("to cell").as_str();
        let actual = spec
            .transitions_from(&from)
            .and_then(|t| t.lookup(condition))
            .unwrap_or_else(|| panic!("no `{condition}` transition out of `{}`", from.as_str()));
        assert_eq!(
            actual.as_str(),
            to,
            "`{}` --{condition}--> expected `{to}`",
            from.as_str()
        );
    }
}

#[tokio::main]
async fn main() {
    TrustWorld::cucumber()
        .run("tests/features/trust_esignature.feature")
        .await;
}
