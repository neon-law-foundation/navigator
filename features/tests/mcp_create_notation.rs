//! Cucumber runner for `features/mcp_create_notation.feature`.
//!
//! Drives the embedded MCP server end-to-end via `oneshot`,
//! exercising the conversational notation flow that the
//! LLM-driven `aida_create_notation` + `aida_answer_notation`
//! tools expose.

#![allow(clippy::unused_async)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cucumber::{given, then, when, World};
use features::{app_state, body_string, fs_storage, in_memory_db};
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use serde_json::{json, Value};
use store::{entity, seed, Db};
use tower::ServiceExt;
use uuid::Uuid;
use web::{policy::PolicyClient, SessionStore};
use workflows::{InMemoryRuntime, MachineKind, StateMachineRuntime, StateName};

#[derive(Default, World)]
#[world(init = Self::default)]
struct McpWorld {
    app: Option<axum::Router>,
    db: Option<Db>,
    runtime: Option<Arc<InMemoryRuntime>>,
    notation_id: Option<Uuid>,
    /// JSON-RPC `id` counter so each call gets a fresh request id.
    next_rpc_id: u64,
    /// Most recent MCP `result` payload (with or without `isError`).
    last_result: Option<Value>,
}

impl std::fmt::Debug for McpWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpWorld")
            .field("notation_id", &self.notation_id)
            .field("last_result", &self.last_result)
            .finish_non_exhaustive()
    }
}

impl McpWorld {
    fn app(&self) -> axum::Router {
        self.app.as_ref().expect("app not built").clone()
    }
    fn db(&self) -> &Db {
        self.db.as_ref().expect("db not built")
    }
    fn runtime(&self) -> &Arc<InMemoryRuntime> {
        self.runtime.as_ref().expect("runtime not built")
    }
    fn last_result(&self) -> &Value {
        self.last_result.as_ref().expect("no MCP result captured")
    }

    fn fresh_rpc_id(&mut self) -> u64 {
        self.next_rpc_id += 1;
        self.next_rpc_id
    }

    /// Send one JSON-RPC `tools/call` and capture the `result`
    /// payload (regardless of `isError`).
    async fn call_tool(&mut self, name: &str, arguments: Value) {
        let rpc_id = self.fresh_rpc_id();
        let body = json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            }
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
        self.last_result = Some(envelope["result"].clone());
    }
}

#[given("a fresh Neon Law Navigator app with the canonical templates seeded")]
async fn build_app(world: &mut McpWorld) {
    let db = in_memory_db().await;
    let storage = fs_storage("mcp-create-notation").await;
    seed::seed_canonical(&db, &storage)
        .await
        .expect("seed canonical");
    let runtime = Arc::new(InMemoryRuntime::new());
    let state = app_state(
        db.clone(),
        runtime.clone(),
        storage,
        PolicyClient::passthrough(),
        None,
        SessionStore::new("test-session-key-not-for-production"),
    );
    let router = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    world.app = Some(router);
    world.db = Some(db);
    world.runtime = Some(runtime);
}

#[given(regex = r#"^a seeded person "([^"]+)" with email "([^"]+)"$"#)]
async fn seed_person(world: &mut McpWorld, name: String, email: String) {
    entity::person::ActiveModel {
        name: ActiveValue::Set(name),
        email: ActiveValue::Set(email),
        role: ActiveValue::Set(entity::person::Role::Client),
        ..Default::default()
    }
    .insert(world.db())
    .await
    .unwrap();
}

#[when(regex = r#"^the LLM calls aida_create_notation for "([^"]+)" as "([^"]+)"$"#)]
async fn create_notation(world: &mut McpWorld, template_code: String, email: String) {
    // A matter always opens against a pre-existing entity; seed one for
    // the engagement to open against.
    let entity_id = store::test_support::seed_entity(world.db()).await;
    world
        .call_tool(
            "aida_create_notation",
            json!({
                "template_code": template_code,
                "person_email": email,
                "entity_id": entity_id,
            }),
        )
        .await;
    if let Some(id_val) = world.last_result()["structuredContent"]["notation_id"].as_str() {
        world.notation_id = Some(Uuid::parse_str(id_val).unwrap());
    }
}

#[when(regex = r#"^the LLM calls aida_answer_notation with code "([^"]+)" value "([^"]+)"$"#)]
async fn answer_notation(world: &mut McpWorld, code: String, value: String) {
    let id = world.notation_id.expect("no notation_id captured");
    world
        .call_tool(
            "aida_answer_notation",
            json!({
                "notation_id": id,
                "question_code": code,
                "value": value,
            }),
        )
        .await;
}

#[then(regex = r#"^the MCP response status is "([^"]+)"$"#)]
async fn assert_status(world: &mut McpWorld, expected: String) {
    let actual = world.last_result()["structuredContent"]["status"]
        .as_str()
        .expect("structuredContent.status missing");
    assert_eq!(actual, expected.as_str(), "{}", world.last_result());
}

#[then(regex = r#"^the MCP next question is "([^"]+)"$"#)]
async fn assert_next_question(world: &mut McpWorld, expected: String) {
    let actual = world.last_result()["structuredContent"]["next_question"]["code"]
        .as_str()
        .expect("structuredContent.next_question.code missing");
    assert_eq!(actual, expected.as_str(), "{}", world.last_result());
}

#[then("the notation has reached the questionnaire END state")]
async fn assert_end(world: &mut McpWorld) {
    let id = world.notation_id.expect("no notation_id captured");
    let current = StateMachineRuntime::current_state(
        world.runtime().as_ref(),
        MachineKind::Questionnaire,
        id,
    )
    .await
    .expect("runtime should know this notation");
    assert_eq!(current, StateName::end(), "runtime state");
    // And the notation row exists under the seeded person.
    let row = entity::notation::Entity::find_by_id(id)
        .one(world.db())
        .await
        .unwrap()
        .expect("notation row");
    assert_eq!(row.template_id.to_string().len(), 36);
}

#[then(regex = r#"^the MCP tool error mentions "([^"]+)"$"#)]
async fn assert_tool_error(world: &mut McpWorld, needle: String) {
    let result = world.last_result();
    assert_eq!(
        result["isError"], true,
        "expected tool error result, got {result}",
    );
    let text = result["content"][0]["text"]
        .as_str()
        .expect("error result missing text");
    assert!(
        text.contains(needle.as_str()),
        "error `{text}` does not mention `{needle}`",
    );
}

#[tokio::main]
async fn main() {
    McpWorld::run("tests/features/mcp_create_notation.feature").await;
}
