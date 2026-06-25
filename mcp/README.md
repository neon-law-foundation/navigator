# mcp

[Model Context Protocol](https://modelcontextprotocol.io/) server for Neon Law Navigator. Exposes a JSON-RPC `/mcp`
endpoint that LLM clients ([Gemini Enterprise](https://cloud.google.com/gemini-enterprise), LibreChat, etc.) call to
operate on Neon Law Navigator data. Same database as `web`; a successful `aida_create_person` lands in the same
`persons` table the website reads from.

## Tool registry

Today, by design:

| Name | What it does |
| --- | --- |
| `aida_create_person` | Insert a new person (unique name + email). |
| `aida_show_person` | Fuzzy-find people by case-insensitive name / email substring. |
| `aida_list_jurisdictions` | List every jurisdiction (US states, federal, foreign). |
| `aida_create_notation` | Start a conversational notation from a template; returns the first question. |
| `aida_answer_notation` | Submit one answer; returns next question or `status: "complete"`. |
| `aida_validate_notation` | Lint markdown without persisting; returns `clean` + `violations`. |

The source of truth is [`src/tools/mod.rs`](src/tools/mod.rs) — specifically `list_tools()` (what `tools/list`
advertises) and the `match` arm in `call_tool` (what `tools/call` dispatches).

### The `aida_` prefix is required

Every tool name MUST start with `aida_` — multi-server MCP clients (Gemini Enterprise's Custom MCP Server, LibreChat,
Claude Desktop) surface tools from every connected server in one flat list, and the prefix is what keeps AIDA tools
grouped and free of name collisions. The prefix lives in `tools::REQUIRED_PREFIX` and is enforced by a generic unit test
(`every_tool_name_starts_with_aida_prefix`) that iterates over whatever `list_tools()` returns — so a new tool that
forgets the prefix fails `cargo test -p mcp` without anyone having to remember to update an explicit allow-list.

## Conversational notation: `aida_create_notation` + `aida_answer_notation`

These two tools form one pattern: the LLM asks the user the questionnaire questions in chat; the server owns the state
machine. Per call the server returns either the next question to ask (with `status: "needs_answer"` and a
`next_question` object holding `code`, `prompt`, and `answer_type`) or `status: "complete"` once the questionnaire
reaches END.

1. **Start.** Call `aida_create_notation` with `template_code` (e.g. `onboarding__retainer`). The server creates the
   Notation, starts the questionnaire runtime, and returns the first question.
2. **Loop.** For each `next_question`, ask the user using the server's `prompt` verbatim, then call
   `aida_answer_notation` with `notation_id`, `question_code` (from the most recent `next_question`), and the user's
   `value`. Repeat until `status: "complete"`.
3. **Hand off.** Once complete, the post-intake workflow is the caller's next move; this surface only owns the
   questionnaire half.

Acting principal: when the MCP boundary is enforced (production Google OAuth populates a verified email), the server
uses that email to resolve the respondent and IGNORES any `person_email` argument. In pass-through mode (KIND / local
dev), `person_email` is required on `aida_create_notation`.

## Shape

`mcp` is a **library crate**. There is no `mcp` binary. The production deployment merges [`build_router`] into the `web`
axum router so `/mcp` rides on the same Pod and the same Cloud SQL connection as the public site.

In production the endpoint is `POST https://www.neonlaw.com/mcp`, gated by `web::google_oauth` — every request must
carry a Google OAuth bearer token whose `aud`/`azp` is allowlisted in `GOOGLE_OAUTH_CLIENT_IDS`, where the token has
`email_verified: true` and an email ending in the `GOOGLE_OAUTH_REQUIRED_HD` Workspace domain. The OPA middleware
(`require_policy`) is also in the chain, picking up the synthesized session and applying the same `staff`-role rule that
it uses for `/portal`. See [docs/gemini-enterprise-mcp.md](../docs/gemini-enterprise-mcp.md) for the full Gemini
Enterprise registration runbook.

## Smoke-test against the in-cluster `web`

```bash
# Port-forward web → localhost, then hit /mcp directly. KIND /
# local dev: GOOGLE_OAUTH_CLIENT_IDS is unset, so google_oauth is
# a pass-through and require_auth gates on a Bearer JWT instead.
kubectl -n navigator port-forward svc/web 3001:3001 &
curl -s http://localhost:3001/mcp \
  -H 'authorization: Bearer <test-JWT>' \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
```

The transport is MCP's Streamable HTTP shape — a single `POST /mcp` that accepts JSON-RPC envelopes. Streamable-HTTP MCP
clients (Gemini Enterprise's Custom MCP Server data store, LibreChat, Claude Desktop) all configure it the same way.

## What's next

Tools live under `src/tools/`; adding one is a `pub mod` plus a match arm in `call_tool` and an entry in `list_tools()`.
The `aida_` prefix is mandatory — see "The `aida_` prefix is required" above. Keep the surface narrow — the design bet
is "many small, obviously-safe tools" rather than a general-purpose database adapter.
