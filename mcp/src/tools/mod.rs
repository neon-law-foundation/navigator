//! Tool registry for the MCP server.
//!
//! Adding a tool is two lines: a `pub mod` here and a `match` arm in
//! [`call_tool`]. Each tool module owns its JSON Schema (returned by
//! `descriptor`) and its handler (`call`).
//!
//! Tool names are namespaced under `aida_` so clients that surface
//! multiple MCP servers (Gemini Enterprise, `LibreChat`) can group
//! Neon Law Navigator's tools cleanly in their UI.

use serde_json::Value;

use crate::principal::Principal;
use crate::server::McpState;

pub mod aida_bulk_import;
pub mod aida_send_welcome_email;
pub mod aida_spawn_legal_council;
pub mod answer_notation;
pub mod create_notation;
pub mod create_person;
pub mod create_project;
pub mod link_person_project;
pub mod list_entities;
pub mod list_jurisdictions;
pub mod list_projects;
pub mod list_tools;
pub mod show_person;
pub mod validate_notation;

/// Returns the list of tool descriptors `tools/list` advertises.
#[must_use]
pub fn list_tools() -> Vec<Value> {
    vec![
        create_person::descriptor(),
        show_person::descriptor(),
        list_jurisdictions::descriptor(),
        list_entities::descriptor(),
        create_notation::descriptor(),
        answer_notation::descriptor(),
        validate_notation::descriptor(),
        create_project::descriptor(),
        list_projects::descriptor(),
        link_person_project::descriptor(),
        list_tools::descriptor(),
        aida_bulk_import::descriptor(),
        aida_spawn_legal_council::descriptor(),
        aida_send_welcome_email::descriptor(),
    ]
}

/// Required prefix for every MCP tool name we advertise. Multi-server
/// MCP clients (Gemini Enterprise, `LibreChat`) surface tools from
/// every connected server in one list — namespacing Neon Law Navigator's tools
/// keeps them grouped and avoids name collisions. Enforced by
/// `every_tool_name_starts_with_aida_prefix` in this module's tests.
pub const REQUIRED_PREFIX: &str = "aida_";

/// Tools that only read. These run without a human confirmation step
/// on the A2A surface. Everything NOT listed here is treated as
/// side-effecting — it writes a row, sends mail, or mutates Drive — and
/// the A2A confirmation gate pauses for explicit user approval before
/// it runs (the `input-required` task state). Defaulting to "needs
/// confirmation" is deliberate: a newly-added tool is gated until
/// someone consciously marks it read-only here, so we never ship a
/// silent side-effect. Kept in lockstep with [`list_tools`] by
/// `read_only_set_only_names_real_tools`.
const READ_ONLY_TOOLS: &[&str] = &[
    "aida_show_person",
    "aida_list_jurisdictions",
    "aida_list_entities",
    "aida_validate_notation",
    "aida_list_projects",
    "aida_list_tools",
    "aida_spawn_legal_council",
];

/// Whether a tool mutates state — writes a row, sends an email, changes
/// Drive — and therefore needs an explicit confirmation step before the
/// A2A surface runs it. Accepts either the prefixed MCP name
/// (`aida_create_person`) or the unprefixed A2A skill id
/// (`create_person`). Tools not listed in [`READ_ONLY_TOOLS`] default to
/// side-effecting, so the safe answer (gate it) is the default for
/// anything new or unrecognized.
#[must_use]
pub fn is_side_effecting(tool_name: &str) -> bool {
    let prefixed = if tool_name.starts_with(REQUIRED_PREFIX) {
        tool_name.to_string()
    } else {
        format!("{REQUIRED_PREFIX}{tool_name}")
    };
    !READ_ONLY_TOOLS.contains(&prefixed.as_str())
}

/// Whether `tool_name` (prefixed or unprefixed) names a real tool in the
/// catalog. Callers that gate side-effecting tools use this so an
/// *unknown* skill still falls through to the `Unknown` error rather than
/// being reported as an authorization failure.
#[must_use]
pub fn is_known_tool(tool_name: &str) -> bool {
    let prefixed = if tool_name.starts_with(REQUIRED_PREFIX) {
        tool_name.to_string()
    } else {
        format!("{REQUIRED_PREFIX}{tool_name}")
    };
    list_tools()
        .iter()
        .any(|d| d.get("name").and_then(Value::as_str) == Some(prefixed.as_str()))
}

/// Dispatch a `tools/call`. Returns the MCP `result` payload (the
/// thing that ends up under `Response::result`), or a structured
/// error the dispatcher will repackage as an MCP tool error.
///
/// `principal` is the authenticated email behind the call (populated
/// by an upstream auth layer; see [`crate::Principal`]). Tools that
/// mutate data trust it over any caller-supplied `email`-style
/// argument.
pub async fn call_tool(
    state: &McpState,
    principal: Option<&Principal>,
    name: &str,
    arguments: &Value,
) -> Result<Value, ToolError> {
    let db = &state.db;
    let runtime = state.questionnaire_runtime.as_ref();
    // Per-tool authorization, enforced for EVERY dispatch path (the MCP
    // server, the A2A router loop, and the A2A direct-skill path). A
    // side-effecting tool invoked by an *authenticated* non-staff caller
    // is refused here, so authz never depends solely on the endpoint's
    // OPA gate or the LLM confirmation flow.
    require_tool_authz(db, principal, name).await?;
    match name {
        "aida_create_person" => create_person::call(db, arguments).await,
        "aida_show_person" => show_person::call(db, arguments).await,
        "aida_list_jurisdictions" => list_jurisdictions::call(db, arguments).await,
        "aida_list_entities" => list_entities::call(db, arguments).await,
        "aida_create_notation" => {
            create_notation::call(db, runtime, state.storage.as_ref(), principal, arguments).await
        }
        "aida_answer_notation" => {
            answer_notation::call(db, runtime, state.storage.as_ref(), arguments).await
        }
        "aida_validate_notation" => validate_notation::call(arguments).await,
        "aida_create_project" => create_project::call(db, arguments).await,
        "aida_list_projects" => list_projects::call(db, arguments).await,
        "aida_link_person_project" => link_person_project::call(db, arguments).await,
        "aida_bulk_import" => aida_bulk_import::call(db, principal, arguments).await,
        "aida_list_tools" => list_tools::call(db, arguments).await,
        "aida_spawn_legal_council" => aida_spawn_legal_council::call(arguments).await,
        "aida_send_welcome_email" => aida_send_welcome_email::call(state, arguments).await,
        other => Err(ToolError::Unknown(other.to_string())),
    }
}

/// Defense-in-depth tier check for side-effecting tools. An
/// *authenticated* caller (a [`Principal`] is present) must resolve to a
/// staff/admin `persons` row to run a side-effecting tool. An
/// unauthenticated caller (`None`) is allowed through: that is the
/// KIND/local-dev path where no auth layer ran and MCP has no identity,
/// and in production the OAuth layer always injects a principal *and*
/// the endpoint is OPA-staff-gated. Read-only tools are never gated.
///
/// This closes the gap where any allowlisted token was treated as staff:
/// a validated-but-non-staff identity (e.g. a Google token whose email
/// maps to a client) can no longer invoke a write tool.
async fn require_tool_authz(
    db: &store::Db,
    principal: Option<&Principal>,
    tool_name: &str,
) -> Result<(), ToolError> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use store::entity::person;

    if !is_side_effecting(tool_name) {
        return Ok(());
    }
    let Some(email) = principal.map(|p| p.email.trim()).filter(|e| !e.is_empty()) else {
        return Ok(());
    };
    let is_staff = person::Entity::find()
        .filter(person::Column::Email.eq(email))
        .one(db)
        .await?
        .is_some_and(|p| p.role.is_staff_tier());
    if is_staff {
        Ok(())
    } else {
        Err(ToolError::Forbidden(format!(
            "{email} is not staff or admin; '{tool_name}' is a privileged operation"
        )))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("unknown tool: {0}")]
    Unknown(String),
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("not found: {0}")]
    NotFound(String),
    /// The authenticated principal lacks the tier this tool requires
    /// (e.g. a bulk write reserved for staff/admin). The model can't
    /// fix this by retrying with different arguments.
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// The write would violate a UNIQUE constraint. Surfaced to the
    /// model as a tool-call failure with `conflict:` so it can correct
    /// the input rather than treat the error as a transient backend
    /// problem to retry. Wraps the original `DbErr` for log fidelity.
    #[error("conflict: {0}")]
    Conflict(sea_orm::DbErr),
    #[error("database error: {0}")]
    Database(sea_orm::DbErr),
    /// Catch-all for internal failures the model can't fix by
    /// retrying with different arguments — workflow-runtime
    /// errors, missing seed data, spec parse failures.
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<sea_orm::DbErr> for ToolError {
    fn from(err: sea_orm::DbErr) -> Self {
        if store::is_unique_violation(&err) {
            ToolError::Conflict(err)
        } else {
            ToolError::Database(err)
        }
    }
}

/// Decode a tool's raw JSON `arguments` into its typed `Args`, mapping
/// any deserialization failure to [`ToolError::InvalidArguments`]. Every
/// tool shares this so the bad-input error convention stays identical
/// across the catalog and each handler reduces to
/// `let args: Args = super::decode_args(arguments)?;`.
pub(crate) fn decode_args<T: serde::de::DeserializeOwned>(
    arguments: &Value,
) -> Result<T, ToolError> {
    serde_json::from_value(arguments.clone())
        .map_err(|e| ToolError::InvalidArguments(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{call_tool, list_tools, ToolError, REQUIRED_PREFIX};
    use crate::server::McpState;
    use serde_json::json;
    use std::sync::Arc;
    use workflows::InMemoryRuntime;

    async fn state() -> McpState {
        let db = store::test_support::pg().await;
        let runtime: Arc<dyn workflows::StateMachineRuntime> = Arc::new(InMemoryRuntime::new());
        McpState::new(db, runtime)
    }

    /// Generic invariant: every tool descriptor returned by
    /// [`list_tools`] must use the [`REQUIRED_PREFIX`] namespace. This
    /// runs over *whatever* `list_tools` returns, so a future tool
    /// that forgets the prefix fails this test without anyone having
    /// to remember to update the explicit set below.
    #[test]
    fn every_tool_name_starts_with_aida_prefix() {
        let tools = list_tools();
        assert!(
            !tools.is_empty(),
            "list_tools must advertise at least one tool"
        );
        for tool in &tools {
            let name = tool["name"]
                .as_str()
                .unwrap_or_else(|| panic!("tool descriptor has no string `name`: {tool}"));
            assert!(
                name.starts_with(REQUIRED_PREFIX),
                "every tool must be namespaced under `{REQUIRED_PREFIX}`, got `{name}`",
            );
            assert!(
                name.len() > REQUIRED_PREFIX.len(),
                "tool name `{name}` is only the prefix with no suffix",
            );
        }
    }

    /// Explicit registry: the tools we ship today. Pairs with
    /// [`every_tool_name_starts_with_aida_prefix`] — that one enforces
    /// the convention, this one pins the *contents* so a tool can't
    /// be silently removed.
    #[test]
    fn list_tools_advertises_the_expected_registry() {
        let tools = list_tools();
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"aida_create_person"));
        assert!(names.contains(&"aida_show_person"));
        assert!(names.contains(&"aida_list_jurisdictions"));
        assert!(names.contains(&"aida_list_entities"));
        assert!(names.contains(&"aida_create_notation"));
        assert!(names.contains(&"aida_answer_notation"));
        assert!(names.contains(&"aida_validate_notation"));
        assert!(names.contains(&"aida_create_project"));
        assert!(names.contains(&"aida_list_projects"));
        assert!(names.contains(&"aida_link_person_project"));
        assert!(names.contains(&"aida_bulk_import"));
        assert!(names.contains(&"aida_list_tools"));
        assert!(names.contains(&"aida_spawn_legal_council"));
        assert!(names.contains(&"aida_send_welcome_email"));
    }

    #[test]
    fn read_only_tools_are_not_side_effecting() {
        // The read-only allowlist must classify as no-confirmation, by
        // both their prefixed MCP name and unprefixed A2A skill id.
        for name in super::READ_ONLY_TOOLS {
            assert!(
                !super::is_side_effecting(name),
                "`{name}` is on the read-only allowlist but classified side-effecting"
            );
            let unprefixed = name.strip_prefix(REQUIRED_PREFIX).unwrap();
            assert!(
                !super::is_side_effecting(unprefixed),
                "`{unprefixed}` (unprefixed) should match the read-only allowlist"
            );
        }
    }

    #[test]
    fn writers_are_side_effecting_and_default_is_safe() {
        // Known writers must be gated...
        for name in [
            "aida_create_person",
            "aida_send_welcome_email",
            "aida_create_project",
            "aida_create_notation",
            "aida_bulk_import",
        ] {
            assert!(super::is_side_effecting(name), "`{name}` must be gated");
        }
        // ...and unprefixed forms classify the same.
        assert!(super::is_side_effecting("create_person"));
        assert!(super::is_side_effecting("send_welcome_email"));
        // An unknown tool defaults to side-effecting — the safe default.
        assert!(super::is_side_effecting("aida_some_future_writer"));
        assert!(super::is_side_effecting("totally_unknown"));
    }

    #[test]
    fn read_only_set_only_names_real_tools() {
        // Guard against the allowlist drifting from the catalog: every
        // entry must be a tool we actually advertise.
        let tools = list_tools();
        let real: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        for name in super::READ_ONLY_TOOLS {
            assert!(
                real.contains(name),
                "READ_ONLY_TOOLS lists `{name}`, which is not in list_tools()"
            );
        }
    }

    #[tokio::test]
    async fn call_tool_with_unknown_name_returns_unknown_error() {
        let s = state().await;
        let err = call_tool(&s, None, "does_not_exist", &json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Unknown(name) if name == "does_not_exist"));
    }

    #[tokio::test]
    async fn require_tool_authz_blocks_non_staff_yet_allows_anonymous_and_read_only() {
        use super::require_tool_authz;
        use crate::principal::Principal;
        use sea_orm::{ActiveModelTrait, ActiveValue::Set};
        use store::entity::person;

        let s = state().await;
        person::ActiveModel {
            name: Set("Client".into()),
            email: Set("client@example.com".into()),
            oidc_subject: Set(None),
            role: Set(person::Role::Client),
            ..Default::default()
        }
        .insert(&s.db)
        .await
        .unwrap();
        let client = Principal::new("client@example.com");

        // Anonymous (dev / no auth layer) is allowed even for writes.
        assert!(require_tool_authz(&s.db, None, "aida_create_project")
            .await
            .is_ok());
        // A read-only tool is never gated.
        assert!(require_tool_authz(&s.db, Some(&client), "aida_show_person")
            .await
            .is_ok());
        // A side-effecting tool by an authenticated client-tier caller is
        // refused — the core of the fix.
        assert!(matches!(
            require_tool_authz(&s.db, Some(&client), "aida_create_project").await,
            Err(ToolError::Forbidden(_))
        ));
        // An authenticated caller with no `persons` row is also refused.
        let ghost = Principal::new("ghost@example.com");
        assert!(matches!(
            require_tool_authz(&s.db, Some(&ghost), "aida_create_project").await,
            Err(ToolError::Forbidden(_))
        ));
    }

    #[tokio::test]
    async fn call_tool_dispatches_aida_validate_notation() {
        let s = state().await;
        let result = call_tool(
            &s,
            None,
            "aida_validate_notation",
            &json!({ "contents": "# H\n", "markdown_only": true }),
        )
        .await
        .unwrap();
        assert_eq!(result["structuredContent"]["clean"], true);
    }
}
