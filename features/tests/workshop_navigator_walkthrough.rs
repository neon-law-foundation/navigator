//! Cucumber runner for `features/workshop_navigator_walkthrough.feature`.
//!
//! Grounds the workshop README's prose ("Using the Neon Law Navigator to
//! Rapidly Solve Legal Outcomes") in real Neon Law Navigator behavior. Every
//! scenario maps directly onto a Bloom-tagged learning objective in
//! the README — if a scenario breaks, the page is stale.
//!
//! The attorney is the actor in every `When` step; Neon Law Navigator is the
//! instrument. Scorpio's trust claim (from the engineer council
//! review) is asserted at the bottom: the notation's `state` is
//! `draft` until the attorney explicitly advances the workflow.

#![allow(clippy::unused_async)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cucumber::{given, then, when, World};
use features::{app_state, body_string, fs_storage};
use portal::{policy::PolicyClient, SessionStore};
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;
use workflows::InMemoryRuntime;

/// Stable code for the workshop's deed-of-sale template. Used by the
/// `aida_create_notation` tool to look up the template row inserted
/// in the Background.
const DEED_TEMPLATE_CODE: &str = "real_estate__deed_of_sale";

#[derive(Default, World)]
#[world(init = Self::default)]
struct WorkshopWorld {
    app: Option<axum::Router>,
    storage: Option<Arc<dyn cloud::StorageService>>,
    /// The stock local attorney persona whose firm-side participation scopes
    /// the seeded Henderson matter.
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
            .header(
                "authorization",
                portal::test_support::lawyer_bearer_header(),
            )
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

#[given("a fresh dev Navigator app with the Henderson workshop seed")]
async fn build_app_with_henderson_seed(world: &mut WorkshopWorld) {
    let surreal = features::shared_surreal().await;
    let storage = fs_storage("workshop-navigator-walkthrough").await;
    store::seed::seed_environment(
        &surreal,
        &storage,
        store::DeploymentEnvironment::Dev,
        store::seed::BrandSeed::Neon,
    )
    .await
    .expect("seed the disposable Henderson workshop portfolio");
    let henderson = store::projects::find_by_name(&surreal, "Henderson Bungalow Purchase")
        .await
        .expect("query Henderson matter")
        .expect("dev seed opens the Henderson matter");
    let lawyer = store::persons::find_by_email_ci(&surreal, "lawyer@neonlaw.com")
        .await
        .expect("query local lawyer persona")
        .expect("dev seed registers the local lawyer persona");

    let runtime = Arc::new(InMemoryRuntime::new());
    let state = app_state(
        runtime,
        storage.clone(),
        PolicyClient::passthrough(),
        None,
        SessionStore::new("test-session-key-not-for-production"),
    )
    .await;
    let router = features::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR));
    world.app = Some(router);
    world.storage = Some(storage);
    world.project_id = Some(henderson.id);
    world.attorney_email = Some(lawyer.email);
}

#[then(regex = r#"^the schema defines a "([^"]+)" table$"#)]
async fn schema_defines_table(_world: &mut WorkshopWorld, table: String) {
    let tables = store::schema::introspect(&features::shared_surreal().await)
        .await
        .expect("introspect the schema");
    assert!(
        tables.contains_key(&table),
        "expected table {table:?} to be defined (every Neon Law Navigator noun must be a real \
         schema entity); the schema defines: {:?}",
        tables.keys().collect::<Vec<_>>(),
    );
}

#[then(regex = r#"^a project named "([^"]+)" exists in the database$"#)]
async fn project_exists_named(world: &mut WorkshopWorld, name: String) {
    let id = world.project_id.expect("no project id captured");
    let row = store::projects::find_by_id(&features::shared_surreal().await, id)
        .await
        .expect("project lookup")
        .expect("project row");
    assert_eq!(row.name, name, "project name");
}

#[then(regex = r#"^the project status is "([^"]+)"$"#)]
async fn project_status_is(world: &mut WorkshopWorld, expected: String) {
    let id = world.project_id.expect("no project id captured");
    let row = store::projects::find_by_id(&features::shared_surreal().await, id)
        .await
        .expect("project lookup")
        .expect("project row");
    assert_eq!(row.status, expected, "project status");
}

#[when("the attorney binds the deed template as a notation")]
async fn attorney_binds_notation(world: &mut WorkshopWorld) {
    // The notation hangs on the seeded matter; its respondent is the
    // matter's client DRI (the seeded client account), not the lawyer presenter.
    let project_id = world
        .project_id
        .expect("the attorney opens the Project before binding a notation");
    let result = world
        .call_tool(
            "aida_create_notation",
            json!({
                "template_code": DEED_TEMPLATE_CODE,
                "project_id": project_id,
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

#[then("a notation row exists linking the deed template to the client")]
async fn notation_links_template_to_attorney(world: &mut WorkshopWorld) {
    let id = world.notation_id.expect("no notation id captured");
    let row = store::notations::find_by_id(&features::shared_surreal().await, id)
        .await
        .expect("notation lookup")
        .expect("notation row");
    let person_row = store::persons::find_by_id(&features::shared_surreal().await, row.person_id)
        .await
        .expect("person lookup")
        .expect("person row");
    assert_eq!(
        person_row.email, "client@neonlaw.com",
        "notation respondent"
    );
    let template_row =
        store::templates::find_by_id(&features::shared_surreal().await, row.template_id)
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
    let surreal = features::shared_surreal().await;
    // Through the notation's pinned `template_id`, the way the sibling step
    // does. The Henderson deed is a *project-scoped* version, so resolving
    // the bare code against the shared catalog finds nothing — and the body
    // under test is the one this notation actually bound.
    let id = world.notation_id.expect("no notation id captured");
    let notation_row = store::notations::find_by_id(&surreal, id)
        .await
        .expect("notation lookup")
        .expect("notation row");
    let row = store::templates::find_by_id(&surreal, notation_row.template_id)
        .await
        .expect("template lookup")
        .expect("deed template row");
    assert_eq!(row.code, DEED_TEMPLATE_CODE, "the bound deed template");
    let body = store::templates::body(&surreal, world.storage(), &row)
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
    let row = store::notations::find_by_id(&features::shared_surreal().await, id)
        .await
        .expect("notation lookup")
        .expect("notation row");
    assert_eq!(row.state, expected, "notation state");
}

#[then(regex = r#"^the notation state is not "([^"]+)"$"#)]
async fn notation_state_is_not(world: &mut WorkshopWorld, forbidden: String) {
    let id = world.notation_id.expect("no notation id captured");
    let row = store::notations::find_by_id(&features::shared_surreal().await, id)
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
    WorkshopWorld::cucumber()
        // Every scenario seeds the same SurrealDB portfolio, so running them
        // concurrently can bind a shared Project row to another scenario's
        // Entity record.
        .max_concurrent_scenarios(1)
        .run_and_exit("tests/features/workshop_navigator_walkthrough.feature")
        .await;
}
