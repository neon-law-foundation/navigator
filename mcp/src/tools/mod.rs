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
/// side-effecting — it writes a row, sends mail, or commits to a matter
/// repo — and the A2A confirmation gate pauses for explicit user approval before
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

/// Whether a tool mutates state — writes a row, sends an email, commits
/// to a matter repo — and therefore needs an explicit confirmation step before the
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
    let surreal = &state.surreal;
    let runtime = state.questionnaire_runtime.as_ref();
    // Per-tool authorization, enforced for EVERY dispatch path (the MCP
    // server, the A2A router loop, and the A2A direct-skill path). A
    // side-effecting tool invoked by an *authenticated* non-lawyer caller
    // is refused here, so authz never depends solely on the endpoint's
    // embedded Rego policy gate or the LLM confirmation flow.
    require_tool_authz(surreal, principal, name).await?;
    match name {
        "aida_create_person" => create_person::call(surreal, arguments).await,
        "aida_show_person" => show_person::call(surreal, arguments).await,
        "aida_list_jurisdictions" => list_jurisdictions::call(surreal, arguments).await,
        "aida_list_entities" => list_entities::call(surreal, arguments).await,
        "aida_create_notation" => {
            create_notation::call(
                surreal,
                runtime,
                state.storage.as_ref(),
                principal,
                arguments,
            )
            .await
        }
        "aida_answer_notation" => {
            answer_notation::call(surreal, runtime, state.storage.as_ref(), arguments).await
        }
        "aida_validate_notation" => validate_notation::call(arguments).await,
        "aida_create_project" => create_project::call(surreal, principal, arguments).await,
        "aida_list_projects" => list_projects::call(surreal, arguments).await,
        "aida_link_person_project" => link_person_project::call(surreal, arguments).await,
        "aida_bulk_import" => aida_bulk_import::call(surreal, principal, arguments).await,
        "aida_list_tools" => list_tools::call(arguments).await,
        "aida_spawn_legal_council" => aida_spawn_legal_council::call(arguments).await,
        "aida_send_welcome_email" => aida_send_welcome_email::call(state, arguments).await,
        other => Err(ToolError::Unknown(other.to_string())),
    }
}

/// Defense-in-depth tier check for side-effecting tools. An
/// *authenticated* caller (a [`Principal`] is present) must resolve to a
/// lawyer/admin `persons` row to run a side-effecting tool. An
/// unauthenticated caller (`None`) is allowed through: that is the
/// KIND/local-dev path where no auth layer ran and MCP has no identity,
/// and in production the OAuth layer always injects a principal *and*
/// the endpoint is embedded Rego policy-lawyer-gated. Read-only tools are never gated.
///
/// This closes the gap where any allowlisted token was treated as lawyer:
/// a validated-but-non-lawyer identity (e.g. a Google token whose email
/// maps to a client) can no longer invoke a write tool.
async fn require_tool_authz(
    surreal: &store::surreal::SurrealDb,
    principal: Option<&Principal>,
    tool_name: &str,
) -> Result<(), ToolError> {
    if !is_side_effecting(tool_name) {
        return Ok(());
    }
    let Some(email) = principal.map(|p| p.email.trim()).filter(|e| !e.is_empty()) else {
        return Ok(());
    };
    // Case-insensitive, like every other email lookup: a lawyer row stored
    // as `Attorney@Example.com` must still authorize a caller whose IdP
    // presents `attorney@example.com`.
    let is_lawyer = store::persons::find_by_email_ci(surreal, email)
        .await?
        .is_some_and(|p| p.role.is_lawyer_tier());
    if is_lawyer {
        Ok(())
    } else {
        Err(ToolError::Forbidden(format!(
            "{email} is not lawyer or admin; '{tool_name}' is a privileged operation"
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
    /// (e.g. a bulk write reserved for lawyer/admin). The model can't
    /// fix this by retrying with different arguments.
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// The write would violate a UNIQUE constraint. Surfaced to the
    /// model as a tool-call failure with `conflict:` so it can correct
    /// the input rather than treat the error as a transient backend
    /// problem to retry. Carries the engine's own message for log
    /// fidelity — it comes from either store.
    #[error("conflict: {0}")]
    Conflict(String),
    /// The store refused a write for a reason the model cannot correct
    /// by retrying with different arguments, and which no module
    /// classified into a narrower variant. Carries the engine's own
    /// message for log fidelity.
    #[error("database error: {0}")]
    Database(String),
    /// Catch-all for internal failures the model can't fix by
    /// retrying with different arguments — workflow-runtime
    /// errors, missing seed data, spec parse failures.
    #[error("internal error: {0}")]
    Internal(String),
}

impl ToolError {
    /// The variant name alone, for logs and spans.
    ///
    /// A `ToolError`'s `Display` text can embed the caller's email or an
    /// argument value (`forbidden: {email} is not lawyer or admin`), and
    /// telemetry carries identifiers and outcomes, never content. Log
    /// sites use this instead of `%err` so a tool failure is still
    /// classifiable without putting a mailbox in the log.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            ToolError::Unknown(_) => "unknown",
            ToolError::InvalidArguments(_) => "invalid_arguments",
            ToolError::NotFound(_) => "not_found",
            ToolError::Forbidden(_) => "forbidden",
            ToolError::Conflict(_) => "conflict",
            ToolError::Database(_) => "database",
            ToolError::Internal(_) => "internal",
        }
    }
}

impl From<store::jurisdictions::JurisdictionError> for ToolError {
    fn from(err: store::jurisdictions::JurisdictionError) -> Self {
        use store::jurisdictions::JurisdictionError as E;
        match err {
            // The unique code index is caller-correctable: the model can
            // retry with a different code.
            E::CodeTaken => ToolError::Conflict(err.to_string()),
            E::Db(_) | E::WriteReturnedNothing => ToolError::Internal(err.to_string()),
        }
    }
}

impl From<store::entities::EntityError> for ToolError {
    fn from(err: store::entities::EntityError) -> Self {
        use store::entities::EntityError as E;
        match err {
            // The firm's own row cannot be forked, and a model that tried
            // can act on that — it is a caller-correctable conflict, not a
            // fault.
            E::FirmAnchorTaken => ToolError::Conflict(err.to_string()),
            E::Db(_) | E::WriteReturnedNothing => ToolError::Internal(err.to_string()),
        }
    }
}

impl From<store::entity_roles::EntityRoleError> for ToolError {
    fn from(err: store::entity_roles::EntityRoleError) -> Self {
        ToolError::Internal(err.to_string())
    }
}

impl From<store::entity_types::EntityTypeError> for ToolError {
    fn from(err: store::entity_types::EntityTypeError) -> Self {
        use store::entity_types::EntityTypeError as E;
        match err {
            // The unique name index is caller-correctable: the model can
            // retry with a different name.
            E::NameTaken => ToolError::Conflict(err.to_string()),
            E::Db(_) | E::WriteReturnedNothing => ToolError::Internal(err.to_string()),
        }
    }
}

impl From<store::persons::PersonError> for ToolError {
    fn from(err: store::persons::PersonError) -> Self {
        use store::persons::PersonError as E;
        match err {
            // The two unique indexes are caller-correctable: the model
            // can retry with a different mailbox or identity.
            E::EmailTaken | E::OidcSubjectTaken => ToolError::Conflict(err.to_string()),
            E::Db(_) | E::WriteReturnedNothing => ToolError::Internal(err.to_string()),
        }
    }
}

impl From<store::people_commands::PeopleCommandError> for ToolError {
    fn from(err: store::people_commands::PeopleCommandError) -> Self {
        use store::people_commands::PeopleCommandError as E;
        match err {
            E::Invalid(m) => ToolError::InvalidArguments(m.to_string()),
            E::EmailConflict => ToolError::Conflict("that email is already in use".into()),
            E::NotFound => ToolError::NotFound("person not found".into()),
            E::Blocked(m) => ToolError::Forbidden(m.to_string()),
            E::SendFailed => ToolError::Internal("welcome email send failed".into()),
            E::Db(e) => ToolError::from(e),
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

    use store::test_support::mem_surreal;
    async fn state() -> McpState {
        let surreal = mem_surreal().await;
        let runtime: Arc<dyn workflows::StateMachineRuntime> = Arc::new(InMemoryRuntime::new());
        McpState::new(surreal, runtime)
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
    async fn require_tool_authz_blocks_non_lawyer_yet_allows_anonymous_and_read_only() {
        use super::require_tool_authz;
        use crate::principal::Principal;

        let s = state().await;
        store::persons::create(
            &s.surreal,
            &store::persons::NewPerson::with_role(
                "Client",
                "client@example.com",
                store::persons::Role::Client,
            ),
        )
        .await
        .unwrap();
        let client = Principal::new("client@example.com");

        // Anonymous (dev / no auth layer) is allowed even for writes.
        assert!(require_tool_authz(&s.surreal, None, "aida_create_project")
            .await
            .is_ok());
        // A read-only tool is never gated.
        assert!(
            require_tool_authz(&s.surreal, Some(&client), "aida_show_person")
                .await
                .is_ok()
        );
        // A side-effecting tool by an authenticated client-tier caller is
        // refused — the core of the fix.
        assert!(matches!(
            require_tool_authz(&s.surreal, Some(&client), "aida_create_project").await,
            Err(ToolError::Forbidden(_))
        ));
        // An authenticated caller with no `persons` row is also refused.
        let ghost = Principal::new("ghost@example.com");
        assert!(matches!(
            require_tool_authz(&s.surreal, Some(&ghost), "aida_create_project").await,
            Err(ToolError::Forbidden(_))
        ));
    }

    #[tokio::test]
    async fn require_tool_authz_matches_lawyer_email_case_insensitively() {
        use super::require_tool_authz;
        use crate::principal::Principal;

        // A lawyer row stored with mixed-case email. The gate matches the
        // stored `email_lower` field, so a differently-cased principal is
        // authorized instead of being rejected before the tool's own
        // resolver runs.
        let s = state().await;
        store::persons::create(
            &s.surreal,
            &store::persons::NewPerson::with_role(
                "Attorney",
                "Attorney@Example.com",
                store::persons::Role::Lawyer,
            ),
        )
        .await
        .unwrap();

        let caller = Principal::new("attorney@example.com");
        assert!(
            require_tool_authz(&s.surreal, Some(&caller), "aida_create_project")
                .await
                .is_ok(),
            "a mixed-case lawyer row must authorize a lower-case caller through the dispatched gate"
        );
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
