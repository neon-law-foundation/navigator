//! Cucumber runner for `features/workshop_navigator_walkthrough.feature`.
//!
//! Grounds the workshop README's prose ("Using the Navigator to
//! Rapidly Solve Legal Outcomes") in real Navigator behavior. Every
//! scenario maps directly onto a Bloom-tagged learning objective in
//! the README — if a scenario breaks, the page is stale.
//!
//! The attorney is the actor in every `When` step; Navigator is the
//! instrument. Scorpio's trust claim (from the engineer council
//! review) is asserted at the bottom: the notation's `state` is
//! `draft` until the attorney explicitly advances the workflow.

#![allow(clippy::unused_async)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cucumber::{given, then, when, World};
use features::{app_state, body_string, fs_storage, in_memory_db};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    Statement,
};
use serde_json::{json, Value};
use store::entity::{notation, person, project, template};
use store::Db;
use tower::ServiceExt;
use uuid::Uuid;
use web::{policy::PolicyClient, SessionStore};
use workflows::InMemoryRuntime;

/// Stable code for the workshop's deed-of-sale template. Used by the
/// `aida_create_notation` tool to look up the template row inserted
/// in the Background.
const DEED_TEMPLATE_CODE: &str = "real_estate__deed_of_sale";

/// The deed body the workshop README walks the attorney through
/// writing — full template-with-frontmatter shape (`start_notation`
/// needs a spec block to parse, even if the questionnaire is
/// empty). Raw string so the YAML indentation survives verbatim;
/// the previous string-continuation form lost leading whitespace
/// and the parser rejected the workflow as missing `BEGIN`.
const DEED_BODY: &str = r#"---
title: Deed of Sale
respondent_type: person
code: real_estate__deed_of_sale
questionnaire:
  BEGIN:
    _: END
  END: {}
workflow:
  BEGIN:
    _: draft
  draft:
    _: staff_review
  staff_review:
    _: notarization_pending
  notarization_pending:
    _: notarized
  notarized:
    _: signed
  signed:
    _: END
  END: {}
---

# Deed of Sale

This Deed is made between {{client_name}} ("Buyer") and the named Seller for the property described
herein. Choice of law: Nevada. Buyer's signature must be acknowledged by a Nevada notary public per NRS
Chapter 240 § 161.
"#;

#[derive(Default, World)]
#[world(init = Self::default)]
struct WorkshopWorld {
    app: Option<axum::Router>,
    db: Option<Db>,
    storage: Option<Arc<dyn cloud::StorageService>>,
    attorney_email: Option<String>,
    project_id: Option<Uuid>,
    notation_id: Option<Uuid>,
    /// JSON-RPC `id` counter so each call gets a fresh request id.
    next_rpc_id: u64,
}

impl std::fmt::Debug for WorkshopWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkshopWorld")
            .field("attorney_email", &self.attorney_email)
            .field("project_id", &self.project_id)
            .field("notation_id", &self.notation_id)
            .finish_non_exhaustive()
    }
}

impl WorkshopWorld {
    fn app(&self) -> axum::Router {
        self.app.as_ref().expect("app not built").clone()
    }
    fn db(&self) -> &Db {
        self.db.as_ref().expect("db not built")
    }
    fn storage(&self) -> &Arc<dyn cloud::StorageService> {
        self.storage.as_ref().expect("storage not built")
    }
    fn fresh_rpc_id(&mut self) -> u64 {
        self.next_rpc_id += 1;
        self.next_rpc_id
    }

    /// Send one MCP `tools/call` and return the `result` payload.
    /// Asserts HTTP 200 + no JSON-RPC `error` member; tool-level
    /// errors are surfaced through `result.isError` which callers
    /// inspect.
    async fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        let rpc_id = self.fresh_rpc_id();
        let body = json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        let req = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("authorization", "Bearer dev")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = self.app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "MCP HTTP status");
        let raw = body_string(resp).await;
        let envelope: Value = serde_json::from_str(&raw).expect("MCP response is JSON");
        assert!(
            envelope.get("error").is_none(),
            "expected `result`, got JSON-RPC `error`: {envelope}",
        );
        envelope["result"].clone()
    }
}

#[given("a fresh Navigator app with a deed-of-sale template")]
async fn build_app_with_deed_template(world: &mut WorkshopWorld) {
    let db = in_memory_db().await;
    let storage = fs_storage("workshop-navigator-walkthrough").await;
    // Insert the workshop's deed-of-sale template. Kept tiny on
    // purpose — the workshop has the lawyer write a similarly small
    // template by hand; this row mirrors that prose so the test
    // grounds what the README claims. The body lives in a blob.
    let blob_id = store::blobs::ingest(&db, &storage, DEED_BODY.as_bytes(), "text/markdown")
        .await
        .expect("ingest deed body");
    template::ActiveModel {
        code: ActiveValue::Set(DEED_TEMPLATE_CODE.into()),
        title: ActiveValue::Set("Deed of Sale".into()),
        respondent_type: ActiveValue::Set("person".into()),
        project_id: ActiveValue::Set(None),
        blob_id: ActiveValue::Set(Some(blob_id)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert deed template");

    let runtime = Arc::new(InMemoryRuntime::new());
    let state = app_state(
        db.clone(),
        runtime,
        storage.clone(),
        PolicyClient::passthrough(),
        None,
        SessionStore::new("test-session-key-not-for-production"),
    );
    let router = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    world.app = Some(router);
    world.db = Some(db);
    world.storage = Some(storage);
}

#[given(regex = r#"^the workshop attorney "([^"]+)" is registered with email "([^"]+)"$"#)]
async fn seed_attorney(world: &mut WorkshopWorld, name: String, email: String) {
    person::ActiveModel {
        name: ActiveValue::Set(name),
        email: ActiveValue::Set(email.clone()),
        role: ActiveValue::Set(store::entity::person::Role::Client),
        ..Default::default()
    }
    .insert(world.db())
    .await
    .expect("insert person");
    world.attorney_email = Some(email);
}

#[then(regex = r#"^the schema has a "([^"]+)" table$"#)]
async fn schema_has_table(world: &mut WorkshopWorld, table: String) {
    let db = world.db();
    let stmt = Statement::from_string(
        db.get_database_backend(),
        format!(
            "SELECT EXISTS ( \
             SELECT 1 FROM information_schema.tables \
             WHERE table_schema = current_schema() \
             AND table_name = '{table}' \
             ) AS present"
        ),
    );
    let row = db
        .query_one(stmt)
        .await
        .expect("schema-introspection query")
        .expect("at least one row");
    let present: bool = row.try_get("", "present").unwrap_or(false);
    assert!(
        present,
        "expected table {table:?} to exist (every Navigator noun must be a real schema entity)",
    );
}

#[when(regex = r#"^the attorney creates a Project named "([^"]+)"$"#)]
async fn attorney_creates_project(world: &mut WorkshopWorld, name: String) {
    let entity_id = store::test_support::seed_entity(world.db()).await;
    let result = world
        .call_tool(
            "aida_create_project",
            json!({ "name": name, "entity_id": entity_id }),
        )
        .await;
    assert_ne!(
        result.get("isError"),
        Some(&Value::Bool(true)),
        "create_project should succeed, got: {result}",
    );
    let id_str = result["structuredContent"]["id"]
        .as_str()
        .expect("structuredContent.id missing");
    world.project_id = Some(Uuid::parse_str(id_str).expect("project id is a UUID"));
}

#[then(regex = r#"^a project named "([^"]+)" exists in the database$"#)]
async fn project_exists_named(world: &mut WorkshopWorld, name: String) {
    let id = world.project_id.expect("no project id captured");
    let row = project::Entity::find_by_id(id)
        .one(world.db())
        .await
        .expect("project lookup")
        .expect("project row");
    assert_eq!(row.name, name, "project name");
}

#[then(regex = r#"^the project status is "([^"]+)"$"#)]
async fn project_status_is(world: &mut WorkshopWorld, expected: String) {
    let id = world.project_id.expect("no project id captured");
    let row = project::Entity::find_by_id(id)
        .one(world.db())
        .await
        .expect("project lookup")
        .expect("project row");
    assert_eq!(row.status, expected, "project status");
}

#[when("the attorney binds the deed template as a notation")]
async fn attorney_binds_notation(world: &mut WorkshopWorld) {
    let email = world.attorney_email.clone().expect("no attorney email");
    let entity_id = store::test_support::seed_entity(world.db()).await;
    let result = world
        .call_tool(
            "aida_create_notation",
            json!({
                "template_code": DEED_TEMPLATE_CODE,
                "person_email": email,
                "entity_id": entity_id,
            }),
        )
        .await;
    assert_ne!(
        result.get("isError"),
        Some(&Value::Bool(true)),
        "create_notation should succeed, got: {result}",
    );
    let id_str = result["structuredContent"]["notation_id"]
        .as_str()
        .expect("structuredContent.notation_id missing");
    world.notation_id = Some(Uuid::parse_str(id_str).expect("notation id is a UUID"));
}

#[then("a notation row exists linking the deed template to Virgo")]
async fn notation_links_template_to_attorney(world: &mut WorkshopWorld) {
    let id = world.notation_id.expect("no notation id captured");
    let row = notation::Entity::find_by_id(id)
        .one(world.db())
        .await
        .expect("notation lookup")
        .expect("notation row");
    let person_row = person::Entity::find_by_id(row.person_id)
        .one(world.db())
        .await
        .expect("person lookup")
        .expect("person row");
    assert_eq!(person_row.name, "Virgo", "notation respondent");
    let template_row = template::Entity::find_by_id(row.template_id)
        .one(world.db())
        .await
        .expect("template lookup")
        .expect("template row");
    assert_eq!(
        template_row.code, DEED_TEMPLATE_CODE,
        "notation template code",
    );
}

#[then(regex = r#"^the deed template body carries the "([^"]+)" placeholder$"#)]
async fn deed_template_body_carries_placeholder(world: &mut WorkshopWorld, needle: String) {
    let row = template::Entity::find()
        .filter(template::Column::Code.eq(DEED_TEMPLATE_CODE))
        .one(world.db())
        .await
        .expect("template lookup")
        .expect("deed template row");
    let body = store::templates::body(world.db(), world.storage(), &row)
        .await
        .expect("deed body in storage");
    assert!(
        body.contains(&needle),
        "deed template body must contain {needle:?}; got body: {body:?}",
    );
}

#[then(regex = r#"^the notation state is "([^"]+)"$"#)]
async fn notation_state_is(world: &mut WorkshopWorld, expected: String) {
    let id = world.notation_id.expect("no notation id captured");
    let row = notation::Entity::find_by_id(id)
        .one(world.db())
        .await
        .expect("notation lookup")
        .expect("notation row");
    assert_eq!(row.state, expected, "notation state");
}

#[then(regex = r#"^the notation state is not "([^"]+)"$"#)]
async fn notation_state_is_not(world: &mut WorkshopWorld, forbidden: String) {
    let id = world.notation_id.expect("no notation id captured");
    let row = notation::Entity::find_by_id(id)
        .one(world.db())
        .await
        .expect("notation lookup")
        .expect("notation row");
    assert_ne!(
        row.state, forbidden,
        "Scorpio's load-bearing trust claim: the deed must not be {forbidden:?} until the attorney advances the workflow"
    );
}

#[tokio::main]
async fn main() {
    WorkshopWorld::run("tests/features/workshop_navigator_walkthrough.feature").await;
}
