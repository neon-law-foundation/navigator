# Editing a legal workflow

This is the practical guide to **changing a workflow that already exists** — asking another question, rewriting a
template body, adding a staff-review or signature or filing step, or wiring a fee. For authoring a brand-new matter type
from scratch, start with [agent workflows](agent-workflows.md) and [notation authoring](notation-authoring.md); this doc
is about evolving what is already shipped.

The guiding idea, proven by the end-to-end journey suite in [`features/`](../features/): **the questionnaire and the
workflow composition are the tested contract; the template body is replaceable.** A stub template (see the [Nevada
entity-formation template](../notation_templates/united_states/nevada/state/business_associations/entity_formation.md))
ships a real, tested flow with placeholder prose, and the prose is filled in later without touching the flow.

## The four artifacts of one workflow

A single workflow `code` (e.g. `onboarding__nest`) is defined in four places that must stay in lockstep:

1. **The template markdown** — `notation_templates/<category>/<snake_case_name>.md`. Its YAML frontmatter carries the
   `questionnaire:` and `workflow:` blocks plus `title` / `code` / `respondent_type`; the body after the frontmatter is
   the document prose.
2. **The standalone spec** — `workflows/specs/<code>.yaml`. The same `questionnaire:` + `workflow:` blocks, no body.
   This is the form `cli scaffold` generates and what the runtime resolves by code.
3. **The seed registration** — `store/src/seed.rs`: an `include_str!` constant in `mod canonical` and a row in the
   `seed_templates` loop, so a fresh cluster carries the template. Adding one bumps the `templates_inserted` count the
   seed tests assert.
4. **The bundled-spec registration** — `workflows/src/specs.rs`: an entry in `BUNDLED_SPEC_YAML`, so
   `bundled_spec_yaml(code)` (and therefore the walker and every journey) can find the spec.

Two tests keep these honest, both DB-free and fast:

- `workflows/tests/spec_coherence.rs` — the standalone YAML and the template frontmatter must parse to the **same**
  spec. Edit one block, edit the other identically.
- `workflows/tests/workflow_integrity.rs` — every template's machine has `BEGIN` and `END`, `END` is reachable, every
  transition target is a real state, and every workflow state's prefix resolves to a `StepKind`.

> A template that has a `BUNDLED_SPEC_YAML` entry but no `seed_templates` row parses fine yet cannot be *opened* —
> `start_notation` resolves the template from the database. Several shipped products (the Nautilus letters, the estate
> plan) hit exactly this gap; if a notation won't open, check that the code is seeded, not just bundled.

## Asking more questions

The `questionnaire:` block is a linear state machine of question codes walked one answer per request.

1. Add the state to the `questionnaire:` block in **both** the template frontmatter and the standalone spec, keeping the
   `_` chain intact (`BEGIN: { _: first }`, … `last: { _: END }`).
2. The prefix of every questionnaire state must be a **seeded question code**. Reuse an existing code from
   [`store/seeds/Question.yaml`](../store/seeds/Question.yaml) when one fits — the walker renders the prompt from that
   row, and rule `N104` validates the code exists. If you need a new code, add a record to `Question.yaml` (and a row to
   `QuestionTranslation.yaml` for every non-English locale, so the questionnaire still reads in the client's language —
   see [`i18n.md`](i18n.md)).
3. Reference the answer in the body as `{{question_code}}`; it is substituted at render time.

A questionnaire that reuses only seeded codes needs no other change. New codes are the only reason to touch the seed.

## Updating the template body

The body is plain markdown rendered two ways: to HTML for the on-screen preview and to a PDF through the Typst compiler
(`pdf::render`). Two Typst gotchas the existing templates avoid:

- **No `#` headings.** `#` starts code mode in Typst markup; use bold runs and prose, as the trust and Nest bodies do.
- **Escape `$`.** A bare `$` opens math mode; write `\$5,000` so it renders as a literal dollar in both the HTML and
  the PDF.

Signature placeholders are role-scoped and carry a dot — `{{client.signature}}`, `{{firm.signature}}`,
`{{client.date}}`. They expand to anchored Typst blocks plus the e-signature manifest; data placeholders (no dot) are
substituted first, so the two never collide. Turning a stub into the real document is *only* a body edit — the
questionnaire and workflow, and every journey that exercises them, are untouched.

## Changing the workflow composition

The `workflow:` block is a state machine whose state-name **prefix** selects the actor and side effect via
`workflows::step::step_kind_for`. The vocabulary you compose from:

| Prefix | `StepKind` | What it means |
| --- | --- | --- |
| `staff_review` | `StaffReview` | a licensed attorney approves before the flow advances |
| `client_review` | `ClientReview` | the client signs off on an attorney-reviewed draft |
| `document_open__*` | `DocumentOpen` | render + persist a PDF (the signal carries a `DocumentPayload`) |
| `document_intake__*` | `DocumentIntake` | ingest an uploaded document (carries an `IntakePayload`) |
| `sent_for_signature__*` | `System` | wait for the e-signature ceremony |
| `firm_signature__*` | `FirmSignature` | the firm signs — on the closing letter, this closes the matter |
| `mailroom_send` / `certified_mail__*` / `e_filing__*` / `filing__*` | submission kinds | record a `filings` row |

The prefix is the reusable step; the discriminator after `__` names the instance in this template. Prefer
`document_open__articles_pdf` or `mailroom_send__debt_validation` over a new bespoke state. A new prefix means a new
engine capability, not just a new legal product.

Rules to hold when editing:

- **Add the prefix to `step_kind_for` first** if it is genuinely new, or `workflow_integrity` fails with "unrouted".
- **`staff_review` gates every government submission** (`N106` + `workflows::staff_review_precedes_submission`): no
  `filing__*` / `mailroom_send` / `e_filing__*` state may be reachable without first crossing a bare `staff_review`.
- **`END` must stay reachable** from `BEGIN`, and every branch target must be a declared state.

Feature files should prove the composition ("this template wires these reusable steps in this order/branching shape").
Rust tests should prove the step mechanics (`StepKind` routing, payload decoding, dispatch side effects, and replay-safe
durability).

### Two ways a workflow is driven

**Walker-driven (signed templates).** The admin walker at `/portal/admin/notations/:id/step` auto-drives this exact
shape on questionnaire completion:

```text
intake_submitted → intake_persisted__<respondent> → <doc>_rendered → staff_review →
approved → document_open__<doc>_pdf → pdf_persisted → sent_for_signature__pending
```

The retainer, Nest, and Nexus templates follow it, so the walker renders the PDF and fires the signature seam with no
per-template code. If you want a template walker-driven, match this shape — you may append a tail, e.g. Nest's
`filing__nv_sos` step after `signature_received`.

**Worker-driven (everything else).** Branching or differently-shaped machines (the estate plan, the Nautilus letters,
annual reports) are driven by signalling the runtime directly — in dev/tests through `workflows::DispatchingRuntime`, in
prod through the `workflows-service` worker. The journey runners drive these with `worker().signal(...)`.

## Wiring a fee

The flat matter-close fee lives in one place: `flat_fee_cents(template_code)` in
[`web/src/retainer_walk.rs`](../web/src/retainer_walk.rs). When the firm signs the closing letter,
`raise_matter_close_fee` resolves the matter's originating product to its fee and raises an invoice through the
`billing` seam (idempotent on the project id). To give a product a close fee, add its code → cents to that map. Other
cadences (Nexus monthly, Nautilus subscription) are separate billing concerns, not the close fee.

## Verifying a change

Run the cheap structural tests first, then the journey that exercises the flow end to end:

```bash
cargo test -p workflows --test workflow_integrity --test spec_coherence
cargo test -p features --test <journey>          # e.g. nest_formation, northstar_estate
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes notation_templates/<category>/<snake_case_name>.md
```

**Green means green.** Cucumber's `.run()` is non-fatal: a failing *or skipped* scenario still exits `0`. Read the
runner's summary line (`N steps (N passed)`) and scan the output for `Step failed` and `Step skipped` — a drifted step
matcher silently skips its assertion. The journey is not done until its runner is genuinely green.

## Where the journeys live

[`features/`](../features/) carries one end-to-end journey per product and surface, plus grouped composition specs for
notation templates that share a workflow shape. Shared mechanics — the seeded app, the admin walker over HTTP, the
worker-shaped runtime, the client Person — live in [`features/src/journey.rs`](../features/src/journey.rs). When you
change a workflow, the matching journey or composition scenario is both the proof it still works and the worked example
of how to drive it.

## Mutating a notation at runtime

The sections above are *authoring time* — they shape the four artifacts of a workflow before any client exists. This
section is the *runtime* sibling: how one notation is filled, edited, and sent **after** it is created. The thesis is
that a notation is **fillable from two sides and editable in the small, then signed** — and that this is additive to the
signed-template walker, not a per-product fork. It works unchanged for the retainer, estate, Nest, and Nexus templates.

**Two-sided answers.** An `answers` row carries who supplied it: `source` (`staff` | `client`) and
`authored_by_person_id` (the typist). The bound Person stays the *respondent* (`answers.person_id`) regardless of who
typed it, so a staff-entered and a client-entered answer to the same question share a respondent but differ in
authorship. `notation_session::answer_step` takes an `AnswerAuthor`; the admin walker records `staff`, the client
surface records `client`. (Both are never-null, low-cardinality dimensions the nightly Parquet snapshot groups by — see
`archives`.)

**Question audience.** `questions.audience` (`staff` | `client` | `both`, default `both`) marks which side sees a
question. It is data, not code — set it in `store/seeds/Question.yaml`, never branch per product.

**The client magic link.** `GET/POST /portal/projects/:id/intake/:notation_id` (`web::intake`) is the demand-side mirror
of the admin walker: the client answers the client-facing questions themselves, one per step, pre-filled with anything
staff entered on their behalf and editable. It is gated by the same cookie-session + project ACL as every other
`/portal/*` page — no second token scheme. The runtime pointer stays staff's progress; client answers write straight to
the `answers` table via `notation_session::record_client_answer` (latest-per-code wins at render), so a client edit
lands without disturbing staff's walk. "Send the client their intake link" on the walker emails the URL.

**Custom clauses.** `notation_clauses` (`web::clauses` admin editor at `/portal/admin/notations/:id/clauses`) holds
per-matter prose without forking the template. The assembled document splices the clauses at the body's
`{{custom_clauses}}` marker (`store::notation_clauses::splice`), in order. A dedicated table keeps each clause one
analyzable row rather than burying it in a body override.

**The review gate (non-negotiable).** Any custom content — a clause **or** a client-entered answer — forces the notation
back through `staff_review` before signature. `drive_post_questionnaire_workflow` parks at `staff_review` instead of
auto-approving; the attorney's "approve and send" (`approve_send_post` → `assemble_and_send`) renders the document
**once**, persists it, and sends *that exact PDF*. The bytes the attorney approved are the bytes that get signed. The
invariant is locked structurally by `workflows::guardrail::staff_review_precedes_signature` (every engagement template
is tested), and behaviourally by the `mutable_intake_docusign` journey.

The end-to-end proof — staff open a matter, send the client a link, the client finishes the rest, staff add a clause,
the attorney reviews, and the exact document goes to DocuSign client-then-firm — is
[`features/tests/mutable_intake_docusign.rs`](../features/tests/mutable_intake_docusign.rs).
