# AIDA over A2A — confirmations and errors

How AIDA behaves once a request reaches her over **A2A** — the surface Gemini Enterprise dials at `chat.neonlaw.com`
(and any other A2A client). The agent-card, OAuth, and one-time wiring live in
[`gemini-enterprise-mcp.md`](gemini-enterprise-mcp.md); this doc is the runtime interaction model: how a free-form ask
becomes a tool call, where AIDA pauses to ask **yes/no**, and how a failure's *reason* gets back to the user instead of
a blank non-result.

It answers two questions that came out of real Gemini Enterprise use:

1. When AIDA already has every value she needs, why does she still ask, and can that be a tap instead of a typed reply?
2. When a tool fails (bulk import was the case in point), why did the chat show "an error" with no message — and how is
   the reason propagated now?

## The request lifecycle

A `message/send` with free-form text (no `metadata.skill`) runs an agentic loop in
[`web::a2a::drive_loop`](../web/src/a2a.rs). The rule in three words: **reads run; writes wait.**

```text
user: "send a welcome email to nick@neonlaw.com"
   │
   ▼  router (Vertex AI Gemini Flash) picks the next tool
show_person { email: "nick@neonlaw.com" }      ← read-only: runs inline, no prompt
   │  result fed back into history → router picks again
send_welcome_email { person_id: <uuid> }       ← side-effecting: PAUSES here
   │
   ▼  Task state = input-required
"Authorize this action? AIDA wants to Send Welcome Email for Nick (nick@neonlaw.com)…
 Choose yes to authorize, or no to cancel."   ← message also carries a structured yes/no choice (data Part)
   │  second message/send, same taskId + contextId, structured choice { confirmation: "yes" }
   ▼
send runs → Task state = completed, the send is the artifact
```

Read-only tools ([`tools::READ_ONLY_TOOLS`](../mcp/src/tools/mod.rs)) run unconfirmed, so a lookup→act chain only ever
stops the user once — at the act. Everything else is side-effecting and waits.

## The confirmation gate

When the router picks a side-effecting tool, [`drive_loop`](../web/src/a2a.rs) does **not** run it. It stashes the
resolved call, returns the Task in the non-terminal `input-required` state, and the prompt rides in `status.message`.
The follow-up `message/send` (same `taskId`/`contextId`) routes through
[`resume_after_confirmation`](../web/src/a2a.rs), which enforces the trust boundary and then runs, cancels, or
re-prompts.

The gate is **not** decoration — it is a legal-supervision requirement. A client-facing act AIDA proposes is authorized
by a licensed human (ABA Model Rule 5.3 supervision of a non-lawyer assistant). Two checks run before the call fires:

- **Identity** — only the principal who *started* the task may confirm it.
- **Role** — only a firm-side principal (staff or admin) may authorize a client-facing side-effect. A client-tier
  caller cannot.

Every decision (`proposed` / `authorized` / `declined` / `denied_identity` / `denied_unauthorized`) is emitted as a
`target: "audit"` event — that log, not the in-memory pending store, is the durable record of who authorized what.

### The confirmation is a structured yes/no choice — no free-text command surface

The gate needs only a yes/no, so it does not ask for a free-text prompt. The `input-required` message carries **two**
parts: the one-sentence prompt (human-readable) *and* a structured `data` Part —
[`confirmation_choice_part`](../web/src/a2a.rs) — that declares the answer as a constrained JSON-Schema `enum`/`oneOf`
(`yes` / `no`, each with a label). A constrained `enum` is the universal signal that tells a schema-aware client to
render a one-tap choice rather than a text box.

[`extract_confirmation`](../web/src/a2a.rs) reads the chosen value from the structured `data` Part first (a
`{"confirmation":"yes"}` object or a bare `"yes"`). If the client doesn't wrap the choice in a `data` Part but instead
echoes the chosen token back as plain text (Gemini Enterprise's shape), that exact token is accepted too, so the gate
behaves identically regardless of envelope — no external client behavior to verify. Only the **exact** tokens `yes` /
`no` authorize or decline; a free-form sentence matches neither and re-prompts, so there is no natural-language command
surface: the action needs only a `yes`, and only a `yes` is read.

The engineering council reviewed this. The findings, and the line between what we control and what we do not:

- **We cannot remove the gate for client-facing acts.** Sending email, assembling and routing a document to DocuSign,
  and other outbound or irreversible actions must keep exactly one human authorization. That is the supervision line the
  legal council drew; loosening it is a legal decision, not engineering. The change here removes the *typed text*, not
  the *authorization* — a staff principal still consciously chooses yes.
- **What we control: the number of gates.** The gate today treats *every* non-read tool identically via
  [`tools::is_side_effecting`](../mcp/src/tools/mod.rs). But the strict Rule-5.3 requirement is about **client-facing**
  acts. Writes that only touch the firm's own records and send nothing outward — `create_person`, `create_project`,
  `link_person_project`, `create_notation` — are closer to "confirm the data we already have." Splitting the
  classification into *internal write* (light confirmation, or a once-per-session trust window) versus *client-facing
  act* (always the hard gate) is the highest-leverage way to reduce prompts. **This is a proposal pending legal
  sign-off, not yet implemented** — it changes the supervision boundary and must go through the legal council first.
- **What we do not control: whether the client renders a button.** A2A 0.3 has no standardized "quick-reply button"
  primitive, so whether Gemini Enterprise draws a tap or a text box for an `input-required` status is the client's call.
  This does **not** affect correctness: a button-tap sends the choice as a `data` Part and a text box sends the typed
  `yes`/`no` token, and `extract_confirmation` accepts both. Either way the approver supplies only a yes/no — never a
  free-form prompt — so there is no live-client behavior left to verify before relying on this in production.

The consensus action: keep the gate, advertise the structured yes/no choice, accept only the exact `yes`/`no` token in
either envelope, and pursue the internal-vs-client-facing split as a legal-council item.

## Error propagation

A tool result is two parts (see [`tool_result_to_parts`](../web/src/a2a.rs)):

1. a **text** Part from `content[0].text` — what a chat UI renders to the user;
2. a **data** Part from `structuredContent` — for programmatic A2A clients.

Gemini Enterprise renders the **text** Part and effectively drops the structured one. So any failure reason that lives
*only* in `structuredContent` is invisible — the user sees a bland line and reads it as "an error with no message."

### Bulk import: the case that surfaced this

[`import::apply`](../import/src/apply.rs) returns `Ok(report)` even when structural validation rejects the payload (then
`organizations`/`people` are empty) or an individual row fails — the reasons live in `report.diagnostics` and each
`RowOutcome.detail`. The tool used to render only the tally:

```text
Bulk import: 0 created, 0 updated, 0 unchanged, 0 failed.
```

That is the silent non-result the user hit. The fix folds the reasons into the **text** Part via
[`ImportReport::problem_lines`](../import/src/apply.rs), so [`aida_bulk_import`](../mcp/src/tools/aida_bulk_import.rs)
now returns:

```text
Bulk import: 0 created, 0 updated, 0 unchanged, 0 failed.

Problems:
• version (error): unsupported contract version 2 (this engine speaks 1)
• organization `njp` failed: unknown jurisdiction code `ZZ`
• person `abigail`: organization `njp` was not created; link skipped
```

Because the A2A bridge reads `content[0].text`, this reaches Gemini Enterprise with no A2A-specific code. The structured
`diagnostics`/`detail` fields are still present for programmatic clients. `problem_lines` lives on the report (not the
tool) so the `cli import-contacts` path and the future `web` upload route surface the same text.

### The general rule

Put the *why* in `content[0].text`. A tool whose failure reason exists only in `structuredContent` will read as a
message-less non-result on any text-only A2A client. The Gemini Enterprise MCP-server description already tells the
planner to "show the user the error and ask whether to retry" (see
[`gemini-enterprise-mcp.md`](gemini-enterprise-mcp.md)) — that only works if the error text is actually in the result.

## The Foundation workshop runs on this surface

The Foundation's Nebula workshop, *Using the Neon Law Navigator to Rapidly Solve Legal Outcomes*
([`/foundation/nebula/workshops/use-the-navigator`](../web/content/workshops/navigator/README.md)), is the canonical
end-user entry into exactly this A2A path. Lawyers add AIDA through Gemini's "Add AIDA" connector — no install, no CLI —
and every "tool call" is a Gemini prompt routed through AIDA's tools over A2A. Two behaviors from this doc are the ones
a workshop attendee feels directly:

- **The confirmation gate is the workshop's trust story.** "The deed is not signed until you, the attorney, explicitly
  advance the workflow" is the same `input-required` pause described above — AIDA proposes, the lawyer authorizes.
- **Error text is the workshop's debugging story.** When a notation or import fails in class, the reason now shows in
  the chat, so an attendee can self-correct instead of seeing a blank failure.

## Cross-references — docs and the tests that ground them

Each behavior described above is grounded by a test or a BDD feature, so the doc and the executable spec stay in step:

- **Reads run inline, writes pause for yes/no** — the confirmation gate is exercised end-to-end by
  `web/src/a2a.rs::rpc_welcome_email_pauses_for_confirmation_then_sends_on_yes` (a stub router drives the real
  `show_person` → `send_welcome_email` chain against a real DB, no live LLM).
- **Welcome-email lookup→confirm→send, as a story** —
  [`aida_welcome_chain.feature`](../features/tests/features/aida_welcome_chain.feature).
- **Bulk-import reasons reach the text Part** — the MCP tests `validation_reject_explains_why_in_the_text_content` and
  `failed_row_reason_reaches_the_text_content` in [`aida_bulk_import.rs`](../mcp/src/tools/aida_bulk_import.rs), plus
  `unknown_jurisdiction_fails_only_its_row` in [`import/tests/apply.rs`](../import/tests/apply.rs).
- **Bulk-import contract and validation** — [`bulk-contact-import.md`](bulk-contact-import.md), grounded by
  [`bulk_import_engagement.feature`](../features/tests/features/bulk_import_engagement.feature).
- **Welcome-email audit trail across surfaces** —
  [`admin_send_welcome.feature`](../features/tests/features/admin_send_welcome.feature).
- **Workshop end-to-end over the AIDA connector** — the [workshop README](../web/content/workshops/navigator/README.md),
  grounded by its [feature](../features/tests/features/workshop_navigator_walkthrough.feature).
- **Agent-card / OAuth / one-time setup** — [`gemini-enterprise-mcp.md`](gemini-enterprise-mcp.md), grounded by the
  card tests in `web/src/a2a.rs`.
