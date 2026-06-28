---
name: create-legal-workflow
description: >
  The standard recipe for adding a new legal workflow to Neon Law Navigator — feature spec first, then template + questionnaire,
  then a Restate-durable workflow composed from a shared step library (staff review, signature, certified mail,
  e-filing, county / Nevada SoS / Department of Taxation filings). Trigger when adding any new legal matter type (LLC
  formation, trust formation, will, retainer variant, dissolution, annual report, tax filing), extending an existing
  template's workflow with new states, or introducing a new step prefix (`filing__nv_sos`, `e_filing__*`,
  `certified_mail`, `document_open`). Also trigger before reaching for a one-off router handler — legal flows belong in
  the template + workflow YAML so they are auditable and reusable.
---

# Creating a legal workflow in Neon Law Navigator

Every legal workflow in this workspace follows the same five-step recipe. **We always start with the feature.** A
feature spec names the actors, the matter, and the happy path; only after that is settled do we design the template, the
questionnaire, and the durable workflow. Skipping the feature spec produces workflows that the team can't reason about
and that the BDD suite can't anchor to.

The unit of work is a [Notation](../../../docs/glossary.md#notation) — a [Template](../../../docs/glossary.md#template)
bound to a [Person](../../../docs/glossary.md#person) (and optionally an [Entity](../../../docs/glossary.md#entity))
with a current [State](../../../docs/glossary.md#state). The Template declares the questionnaire and the workflow;
[Restate](../../../docs/glossary.md#restate) runs the workflow. **The Template declares; Restate runs.**

## The five steps, in order

### 1. Feature spec — `features/tests/features/<matter>.feature`

Write the Cucumber `.feature` file first. Frame the actors and the authorization scheme using glossary nouns — **never
invent new role words**:

| Glossary noun        | Use for                                                 |
|----------------------|---------------------------------------------------------|
| Person               | Every human — clients, attorneys, paralegals, staff.    |
| Person–Project Role  | `attorney`, `paralegal`, `client` on a matter.          |
| Entity               | LLC, Trust, Corporation, Foundation — the "business."   |
| Person–Entity Role   | `manager`, `member`, `beneficiary`, `trustee`.          |
| Jurisdiction         | `NV`, `CA`, `WA`, county codes, federal.                |
| Project              | The matter itself; Notations and Persons hang off it. Every Project corresponds to one folder in the NeonLaw shared drive (`projects.drive_folder_id`). |

A feature should describe **who** can do **what** to **which** matter, plus the happy path through the workflow.
Authorization is `(Person role) × (Person–Project Role) × (route)` and is enforced by OPA — see the `opa-policy` skill —
so feature scenarios that exercise admin routes must name a Person with the right role.

Template to copy from:
[`features/tests/features/retainer_intake.feature`](../../../features/tests/features/retainer_intake.feature).

### 2. Template + questionnaire — `notation_templates/<category>/<snake_case_name>.md`

The Template is the static blueprint. One markdown file with YAML frontmatter and a body of legal prose. Required
frontmatter keys:

```yaml
title: <human-readable name>
code: <category>__<specifier>    # stable id, snake_case, double underscore
respondent_type: person | entity | person_and_entity
confidential: true | false
questionnaire:                   # state machine of question codes
  BEGIN:
    _: first_question_code       # `_` is "respondent answered"
  first_question_code:
    _: next_question_code
  END: {}
workflow:                        # state machine of workflow states
  BEGIN:
    intake_submitted: <first_state>
  END: {}
```

Body uses `{{question_code}}` placeholders that are substituted with the respondent's answers. The template is **inert
until a Notation binds a respondent to it.**

**The template body is English-only.** The notation a client signs is the binding artifact, so it is always English —
only the questionnaire *prompt* may carry an attorney-reviewed localized variant (the `question_translations` table) so
a client understands the question. See [`CLAUDE.md`](../../../CLAUDE.md#human-language-english-first).

Working examples — copy the closest one:

- [`notation_templates/onboarding/retainer.md`](../../../notation_templates/onboarding/retainer.md)
  — person + entity, four-question intake, five-state workflow.
- [`notation_templates/llc/california.md`](../../../notation_templates/llc/california.md)
  — entity-only, three-question, three-state workflow.
- [`notation_templates/trust/nevada.md`](../../../notation_templates/trust/nevada.md) —
  entity-only trust formation.

The template's job is to turn validated answers into a **candidate document**. The candidate is what the workflow
advances through staff review, signature, and filing — so the template body must compile into a complete legal document
with only the answers as input.

### 3. Questions — `store/seeds/Question.yaml`

Every `question_code` referenced in a template's `questionnaire:` must exist in
[`store/seeds/Question.yaml`](../../../store/seeds/Question.yaml) with an `answer_type` (`string`, `text`, `int`,
`bool`, `choice`). The form layer in [`views/src/pages/admin/retainers.rs`](../../../views/src/pages/admin/retainers.rs)
picks the input widget from `answer_type`. Reuse existing question codes across templates wherever the prompt is
identical — `client_name`, `client_email`, `principal_office`, `member_list` are already shared.

### 4. Workflow YAML — compose from the shared step library

A workflow state machine has named States, transitions keyed by event, and `BEGIN` / `END`. **State names use
`<prefix>__<discriminator>`** so [`workflows::step::step_kind_for`](../../../workflows/src/step.rs) can dispatch the
right actor class per state.

#### Existing step prefixes (use these first)

| Prefix             | Actor      | What it means                          |
|--------------------|------------|----------------------------------------|
| `BEGIN` / `END`    | System     | Runtime-driven boundary.               |
| `staff_review`     | Staff      | Operator approves or rejects.          |
| `notarization`     | Respondent | Client signs (or refuses).             |
| `mailroom_send`    | Staff      | Outbound physical mail logged.         |
| `mailroom_receive` | Staff      | Inbound physical mail logged.          |

Where they live:

- `staff_review` is driven by the admin UI in `web::admin`; the staff
  review form posts a `signal(notation_id, condition)` where `condition` is `approved` or `rejected`.
- `notarization` runs through the signature seam — see
  [`docs/retainer_intake.md`](../../../docs/retainer_intake.md#the-signature-seam). Today the implementation is
  `StubSignatureProvider`; a real DocuSign or Dropbox Sign adapter implements the same trait.
- `mailroom_send` and `mailroom_receive` record a
  [Letter](../../../docs/glossary.md#letter) row scoped to a [Mailroom](../../../docs/glossary.md#mailroom).

#### New step prefixes you may need

The user-facing legal vocabulary needs more step kinds than the runtime currently knows about. The canonical ones:

| Prefix (proposed) | Actor  | Example discriminator         |
|-------------------|--------|-------------------------------|
| `e_filing`        | System | `e_filing__nv_sos`            |
| `e_filing`        | System | `e_filing__nv_tax`            |
| `e_filing`        | System | `e_filing__washoe_county`     |
| `e_filing`        | System | `e_filing__clark_county`      |
| `certified_mail`  | Staff  | `certified_mail__nv_sos`      |
| `certified_mail`  | Staff  | `certified_mail__irs`         |
| `document_open`   | System | `document_open__retainer`     |
| `document_open`   | System | `document_open__articles`     |
| `filing_paper`    | Staff  | `filing_paper__washoe_county` |

External-system notes:

- `e_filing__*` posts to a per-jurisdiction e-filing API. Each gets
  its own adapter trait, modeled on [`web::signature::SignatureProvider`](../../../web/src/signature.rs).
- `certified_mail__*` is USPS Certified Mail; staff logs the
  tracking number against the [Letter](../../../docs/glossary.md#letter) row.
- `document_open__*` renders the template body into a
  [Blob](../../../docs/glossary.md#blob) via [`cloud::StorageService`](../../../cloud/) and links it via a
  [Document](../../../docs/glossary.md#document) row.
- `filing_paper__*` is the physical filing window. Often pairs with
  `certified_mail__return_receipt`.

To add one:

1. Extend the `StepKind` enum in
   [`workflows/src/step.rs`](../../../workflows/src/step.rs) and add the prefix to `step_kind_for`.
2. Map the kind to its `ActorClass` in `StepKind::actor()`.
3. If the step touches an external system, add a one-method async
   trait next to [`web::signature::SignatureProvider`](../../../web/src/signature.rs) and a stub implementation.
   Production swaps in the real adapter by replacing the `Arc<dyn Trait>` in [`web::AppState`](../../../web/src/lib.rs).
4. Add a handler branch in
   [`workflows-service/src/notation_service.rs`](../../../workflows-service/src/notation_service.rs) that wraps the side
   effect in `ctx.run("name-of-effect", …)`.

**Reuse before extension.** A "file with the Nevada Secretary of State by paper" step is `mailroom_send__nv_sos` today;
a `filing_paper` prefix is worth adding only when paper filings need behaviour that diverges from generic outbound mail
(tracked receipt, court acknowledgement, jurisdictional confirmation number).

### 5. Durable steps — Restate handlers in `workflows-service`

Each step is one durable side effect. The handler reads the spec yaml and current state from Restate's keyed state,
computes `next_state`, and records the transition by writing a row to `notation_events` inside
`ctx.run("append-event", …)` so a replay reuses the cached row id instead of double-writing. Pattern lives in
[`workflows-service/src/notation_service.rs`](../../../workflows-service/src/notation_service.rs); the journal helpers
are in [`workflows-service/src/journal.rs`](../../../workflows-service/src/journal.rs).

**Every side effect must be wrapped in `ctx.run` with a stable name.** Without it, retries re-run the effect and produce
duplicate rows or duplicate filings. The stable name is how Restate matches a journal entry to a `ctx.run` site across
handler versions — rename it and replay loses the cache hit. See
[`docs/glossary.md#ctxrun`](../../../docs/glossary.md#ctxrun).

## Authorization

Every admin route is gated by OPA. The middleware posts `{path, method, session: {person_id, roles}}` to OPA, which
returns `true` or `false`. Add the route's allow-rule to the Rego in
[`k8s/base/opa/opa.yaml`](../../../k8s/base/opa/opa.yaml) before shipping a new admin handler. See the `opa-policy`
skill for the full pattern.

Workflow-state visibility flows the same way. A respondent sees only their own Notations; a staff member sees the queue
for each `staff_review__*` state. Pull the gating into the Rego, not the handler.

## Testing — same commit as the implementation

A new workflow ships with all four layers of test coverage. None are optional.

| Layer             | File pattern                                    |
|-------------------|-------------------------------------------------|
| Spec shape        | `workflows/tests/<matter>_spec.rs`              |
| Handler unit      | `workflows-service/src/<matter>_service.rs`     |
| BDD scenarios     | `features/tests/features/<matter>.feature`      |
| Browser e2e       | `web/tests/browser_e2e.rs`                      |

What each layer pins:

- **Spec shape** — YAML parses; `BEGIN → … → END` is reachable on
  `InMemoryRuntime`.
- **Handler unit** — per-state `next_state` and per-step idempotence. **BDD scenarios** — the feature spec from step 1,
  now executable. **Browser e2e** — full HTTP path via fantoccini + chromedriver; one scenario per matter.

Pre-commit gates (must pass):

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# Plus the markdown lint if you touched any .md (templates, docs, this skill):
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes notation_templates/
```

**Branch → PR → auto-merge — never commit on `main`.** Per [`CLAUDE.md`](../../../CLAUDE.md) Commit discipline, do the
workflow on a topic branch (`git switch -c <topic>`), push and open a PR (`git push -u origin <topic>` →
`gh pr create`), then enable auto-merge (`gh pr merge --auto --squash`). `ci.yml` runs on the PR and GitHub
squash-merges it once every required check is green — never commit to `main`, never merge by hand.

## Ship the work — merge, then roll the cluster

A legal workflow that lives only on the operator's laptop is half-built. Once your PR auto-merges into `main`, the
**daily tag flow** ([`deploy.yml`](../../../.github/workflows/deploy.yml)) builds both images and publishes them to
**ghcr.io** tagged `YY.MM.DD` — you no longer build images locally. To put the new workflow in front of clients, the
prod-deploy flow (`navigator ship --tag YY.MM.DD`) rolls the GKE cluster onto that **published `YY.MM.DD` image** from
ghcr.io. New workflows are useless until the cluster pulls the image that contains them.

## Things to avoid

- **No ad-hoc admin handlers for legal flows.** If it advances a
  Notation, it belongs in the workflow YAML and the matching step prefix. One-off handlers bypass the journal and break
  replay.
- **No new role words outside the glossary.** Authorization uses
  Person + Person–Project Role + Person–Entity Role + Jurisdiction. Need a new role? Add it to the seeds and the
  glossary first.
- **No skipping the feature file.** Templates and workflows that
  weren't preceded by a feature spec accumulate states no scenario exercises — the BDD suite is how we know the matter
  is end-to-end-reachable.
- **No side effects outside `ctx.run`.** Plain `await` on an HTTP
  call or a DB insert inside a handler runs once per replay; that's the duplicate-filing failure mode.
- **No new programming languages.** Templates are Markdown + YAML;
  everything else is Rust. See [`CLAUDE.md`](../../../CLAUDE.md) — there is no JS / Python / Go hiding in `tests/` or
  `scripts/`.
- **No non-English template bodies.** English-only; see step 2 and
  [`CLAUDE.md`](../../../CLAUDE.md#human-language-english-first).
