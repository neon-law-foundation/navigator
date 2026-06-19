# Authoring notations

This is the how-to companion to [`notation.md`](notation.md). That doc defines the *vocabulary* (Template, Notation,
Questionnaire, Question, Answer, Rule); this one is the *procedure* — how you write a notation, what the toolchain
enforces, what runs after a client finishes intake, and what is still on the roadmap. If a word here is unfamiliar,
[`notation.md`](notation.md) is the source of truth for what it means.

## What a notation is, in one paragraph

A **Template** is a static blueprint: one markdown file with YAML frontmatter, checked into `templates/`. A **Notation**
is that Template come to life — one running instance bound to a [Person](glossary.md#person) (the respondent), exactly
one [Project](glossary.md#project), and optionally an [Entity](glossary.md#entity) — advancing through two state
machines the Template declares. In client English a Notation-in-a-Project is the **Engagement** (or **Retainer**). The
Template *declares*; Restate *runs*. Everything below is about writing good Templates and growing what their workflows
can do.

## Anatomy of a template file

Every template lives at `templates/<category>/<snake_case_name>.md` and has two parts: YAML frontmatter (the contract)
and a markdown body (the document, with `{{question_code}}` placeholders). Here is the shipped retainer's frontmatter
(the real file wraps this block in `---` fences, then the prose body follows):

```yaml
title: Retainer Agreement
respondent_type: person_and_entity
code: onboarding__retainer
confidential: true
questionnaire:            # the intake Q&A — what we ask the client
  BEGIN:                { _: client_name }
  client_name:          { _: client_email }
  client_email:         { _: project_name }
  project_name:         { _: product_description }
  product_description:  { _: END }
  END: {}
workflow:                 # what happens after intake — render, review, sign
  BEGIN:                       { intake_submitted: intake_persisted__client }
  intake_persisted__client:    { retainer_rendered: staff_review }
  staff_review:                { approved: document_open__retainer_pdf, rejected: END }
  document_open__retainer_pdf: { pdf_persisted: sent_for_signature__pending }
  sent_for_signature__pending: { signature_received: END }
  END: {}
```

The body below the frontmatter is plain prose carrying the same `{{code}}` placeholders. At render time each is replaced
with the client's answer — `{{client_name}}` becomes the actual name, `{{project_name}}` the matter.

Frontmatter fields:

- `title` — the human document title (F101 requires it non-empty).
- `respondent_type` — one of `person`, `entity`, `person_and_entity` (F102).
- `code` — the stable, unique identifier (`onboarding__retainer`, `trusts__nevada`); how every surface refers to it.
- `confidential` — an explicit `true`/`false` decision, never defaulted (F105).
- `questionnaire:` — the intake state machine: `BEGIN → question_code → … → END`. Each step's `_:` is the "answered"
  transition. State names are `<question_code>__<discriminator>`; the prefix before `__` must be a real Question `code`.
- `workflow:` — the post-intake state machine: render, staff review, signature, filing. Transitions fire on named
  signals (`approved`, `pdf_persisted`, `signature_received`).

Two machines, one journal: questionnaire and workflow are hosted on a single Restate virtual object keyed by the
notation's id, so their signals serialize and can never interleave. State is **append-only** — every transition writes a
`notation_events` row, and the current state is the latest row's `to_state`. Nothing is ever updated in place, so the
full history of a matter is replayable for audit.

## How to create one — the five-step recipe

New legal matters follow a fixed order (see the `create-legal-workflow` skill for the long form). Feature-first, so the
behavior is specified before the prose exists:

1. **Write the `.feature` first.** Describe the matter as a BDD scenario in `features/` using only Person / Entity
   role nouns from [`glossary.md`](glossary.md). The feature is the spec; the template satisfies it.
2. **Write the template + questionnaire.** Create `templates/<category>/<snake_case_name>.md` with the frontmatter
   above. Declare the `questionnaire:` walk and the `workflow:` states. Body prose uses `{{question_code}}`
   placeholders.
3. **Seed the questions.** Add each new question `code` to `store/seeds/Question.yaml` (prompt, `question_type`,
   help text). The questionnaire's state prefixes must resolve to these codes or F104 fails.
4. **Declare the workflow YAML.** Compose the post-intake flow from the shared step library (below) — never a one-off
   handler. Reuse `staff_review`, signature, and document steps so the flow stays auditable.
5. **Wire the durable handlers.** Bind new workflow steps onto the existing `workflows-service` worker. Never stand up a
   per-workflow pod — one worker hosts every flow.

A template is not legally usable until an attorney has reviewed the body copy. The `staff_review` state is mandatory
(F106) precisely so a licensed human is always in the loop before anything is sent or filed.

## The validation contract

Three rule families guard every template, enforced identically in your editor, in `cli validate`, and in CI — because
all three call the same `rules` crate. A template that is clean on your laptop is clean in the merge gate.

- **F-family (frontmatter shape, structural).** F101 title present; F102 valid `respondent_type`; F103 snake_case
  filename; F104 both machines declare `BEGIN`, reach `END`, and every state prefix resolves to a real Question code;
  F105 `confidential` is an explicit bool; F106 the `workflow:` has a bare `staff_review` state (the suffix form
  `staff_review__for_grantor` does **not** satisfy it — the human-review gate must be unconditional); F108 `code` is the
  stable Template identifier. F-family rules are diagnostic-only: a human must resolve them, the tool will not
  auto-rewrite legal structure.
- **M-family (markdown hygiene, ~50 rules).** Headings, lists, fences, tables, spacing. Most carry a safe autofix.
- **S101 (line length).** 120 Unicode scalars per line, every `.md`. Frontmatter is linted too; folded YAML scalars let
  a long value wrap and still pass.

Run it before committing any `.md` change:

```bash
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes <path>
```

## Authoring in markdown with the LSP

`navigator-lsp` is a single Rust binary speaking LSP over stdio. It shares the exact rules engine the CLI uses, so the
editor and CI can never disagree. Supported editors ship copy-paste configs under [`lsp/`](../lsp) docs: VS Code,
Neovim, Helix, Emacs, Zed. The authoring loop for a non-engineer legal author:

1. **Type.** Open `templates/will/simple.md` in your editor. Write legal prose and frontmatter — no proprietary tool, no
   markup beyond markdown.
2. **Live diagnostics.** On every keystroke the LSP lints the buffer and shows squiggles: F101 if `title:` is missing,
   F104 if the questionnaire/workflow shape is broken, S101 past 120 chars, M-rules on shape. The CLI can add DB-backed
   question-code checks when invoked with `--database-url`. Hover any squiggle for a plain-English explanation of the
   rule.
3. **Fix-all on save.** `source.fixAll` rewrites every mechanical issue — tabs, trailing whitespace, blank-line spacing,
   heading spacing — automatically. What remains is the *semantic* work only a human can do (an unmade `confidential`
   decision, a workflow that never reaches `END`).
4. **Open a PR.** The clean `.md` is committed as a plain-text diff. CI runs the identical engine. An attorney reviews
   readable prose; the linter has already signed off on structure.

### Why markdown + frontmatter + git, not a proprietary tool

- **Ergonomics.** One free binary attaches to whatever editor the author already knows. Fix-on-save removes the entire
  class of formatting fiddling; hover tooltips teach the rules in context, lowering the floor for a non-engineer.
- **Correctness.** A single rules engine is the authority — editor, CLI, and CI cannot diverge. Invariants that matter
  legally (every workflow has a `staff_review` gate, `confidential` is an explicit choice, every workflow code resolves)
  are machine-enforced *before merge*, not left to reviewer vigilance.
- **Auditability.** The template is plain text under git: every change is an attributable, reviewable diff, gated by PR.
  The rules themselves are versioned Rust with snapshot tests. A proprietary document-automation tool hides the document
  in an opaque format with no line-level diff and no enforceable structural contract.

## What runs after intake — the step library

Once the questionnaire reaches `END`, the workflow machine takes over. Steps are resolved from a state-name prefix to a
`StepKind` and an actor class (System / Staff / Respondent) in `workflows/src/step.rs`. Honest status of what is wired
today:

| Step | Status | Notes |
| --- | --- | --- |
| `email_send__<slug>` | Implemented | Durable SendGrid send via two `ctx.run` journals; only `welcome` renders today. |
| `intake_persisted__*` | Implemented | Pass-through wait state recorded on the journal. |
| `staff_review` | State-only | Mandatory gate; dev auto-approves. No prod review UI wired to the worker. |
| `client_review` | State-only | Respondent approves attorney-reviewed drafts on the Phase A review surface. |
| `document_intake__<slug>` | Implemented | Worker files a provided artifact (text/file/link) via `ingest_bytes`. |
| `extract__*` | Seam | Northstar: estate inputs mined from the transcript by Ada/Gemini; advanced on completion. |
| `analysis__*` | Seam | Contract review: web (Vertex Gemini) flags playbook deviations; System wait state. |
| `document_drafts__*` | Implemented | Northstar: web renders drafts into review_documents rows (System wait state). |
| `document_open__retainer_pdf` | Implemented | Worker-dispatched: render + storage persist wrapped in `ctx.run`. |
| `sent_for_signature__pending` | Implemented | Wait state; e-signature webhook signals `signature_received` → END. |
| `notarization`, `_signature` | State-only | Trust/will signing states; a human act, no worker side effect. |
| `firm_signature` | State-only | Firm (staff) signs the closing letter ending a matter; a human act, no side effect. |
| `mailroom_send` | Implemented | Worker records a `filings` row in `ctx.run`; reached only after `staff_review`. |
| `certified_mail`, `e_filing`, `filing__*` | Implemented | Worker submission steps; record `filings` post-review. |
| `onchain__*` | Scaffolded | Node attestation → durable `attestations` row; `null` attestor keeps it `pending`. |
| `mailroom_receive` | State-only | Inbound mail logged by the SendGrid webhook, not a workflow step. |
| `witnesses` | State-only | Respondent's witnesses sign (will); resolves to the Signature step kind. |

Durability is Restate's: each side effect is wrapped in `ctx.run`, so a replay reuses the cached result instead of
re-emailing or double-inserting. In prod the worker dials Restate Cloud; in KIND it dials the in-cluster Operator. The
"State-only" rows are the contract for steps with no worker side effect yet. The drift-guard test
`workflows::step::tests::drift_guard_every_step_prefix_is_documented` fails if `step_kind_for` gains a prefix
(`STEP_PREFIXES`) this table never mentions, so the status here cannot silently rot.

The `onchain__*` row is "Scaffolded": the step kind, the dispatch arm, and the durable `attestations` table are
implemented and tested, but the on-chain write itself is deferred. The chain is isolated behind the
`workflows::attest::Attestor` trait exactly as GCS is isolated behind `cloud::StorageService` — selecting Solana (or a
second chain) is a new `impl Attestor`, never a workflow edit. The default `NullAttestor` records no transaction, so the
row stays `pending` and no live retainer can claim an on-chain record that does not exist. The step is therefore not yet
wired into the binding `onboarding__retainer_node` workflow; that one-line YAML edge lands together with the
`SolanaAttestor` (whose open questions — firm key custody, the client wallet, public-chain confidentiality of the hash,
and finality — are decisions, not code). See `workflows::attest` and the Neon Law Node product page.

### Adding a reusable step — the recipe

A "reusable step" is one `StepKind` that many notations bind to by naming a `<prefix>__<slug>` state — `email_send__*`,
`document_open__*`, `document_intake__*`. Two reference implementations show the shape; the next one is a single
registry entry, not a second dispatch match.

- **Signature** is the *seam* reference: `web::signature::SignatureProvider` is a trait with a stub for KIND/tests and a
  concrete `DocuSignSignatureProvider` for prod, selected from `AppState`. Reach for a trait when the step calls an
  external system that has more than one real implementation you swap at runtime.
- **Document-intake** is the *registry* reference: `document_intake__<slug>` files a provided artifact (a transcript, an
  executed PDF, an ID scan) into the matter through `store::documents::ingest_bytes`. It has exactly one implementation,
  so it is a plain dispatch fn behind one `StepKind`, not a trait.

The step layer routes through one registry, `workflows::dispatch_step`, keyed by `StepKind`. Both callers — the
`workflows-service` worker (`notation_service::workflow_signal`, which wraps the call in `ctx.run`) and the in-process
dev/BDD runtime (`DispatchingRuntime::maybe_dispatch`, which calls it inline) — share that one arm, so a new step is
added once, not twice. To add a step kind with a worker side effect:

1. **Name the prefix + kind.** Add a `StepKind` variant and its `(prefix, StepKind)` row to `STEP_PREFIXES` in
   `workflows/src/step.rs`, plus the actor class in `StepKind::actor`. Document it in the status table above (the
   drift-guard test enforces this).
2. **Write the dispatch fn + payload.** Add `<kind>.rs` with a serde payload (internally tagged on `kind`, like
   `DocumentPayload` / `IntakeArtifact`) and an `async fn dispatch_<kind>(deps…, payload) -> Result<_, _>` that performs
   the *one* side effect and returns — no `ctx.run`, no journaling; durability is the caller's.
3. **Register one arm.** Add the `StepKind` to `dispatches_side_effect` and one match arm to `dispatch_step` in
   `workflows/src/dispatch.rs`, decoding the payload from the signal `value` and calling your dispatch fn with the
   `StepDeps` providers (`email`, `storage`, optional `db`). The worker and the in-process runtime pick it up for free.
4. **Thread the payload from the trigger.** The surface that fires the transition into the step (a `web` handler) builds
   the payload, JSON-serializes it, and passes it as the signal `value`. The artifact for an intake step is
   phone-friendly: a text paste, a file, or a link — never "scan a PDF".

Keep the `ctx.run` boundary in the worker, never inside `dispatch_step`: a registry that journaled its own side effect
would reintroduce the duplicate-effect bug on replay.

## Documents and PDFs

**What we have.** A dedicated `pdf` crate renders a Typst document to PDF bytes in pure Rust (no shell-out), in the firm
typeface Noto Serif, with a redaction helper. The retainer flow substitutes `{{placeholder}}` tokens from the notation's
answers in `web`, then threads the result to the **worker** as a `DocumentPayload` on the `approved` signal; the
`document_open__retainer_pdf` step calls `pdf::render` and persists the bytes through the `cloud::StorageService` seam
(`FsStorage` in dev, GCS in prod) at `notations/<id>/retainer.pdf`, wrapped in `ctx.run` for replay-idempotent
durability. `web` reads the PDF back from storage to hand to the signature provider. This is one-directional: template →
fresh PDF.

**Filling fillable government PDFs — done.** `pdf::fill_acroform(blank_pdf, fields)` opens an existing fillable PDF (a
Nevada SoS articles form, an IRS Form 990) via `lopdf`, walks its AcroForm `/Fields`, sets each `/V`, and sets
`/NeedAppearances` so a viewer regenerates the field appearances — a read-modify-write path distinct from the Typst
`render` path. Blank forms live as templates in the `cloud::StorageService` seam (`forms/<slug>.pdf`); a
`document_open__<form>` sub-slug dispatches the fill through the same worker step as the retainer PDF (via
`DocumentPayload::Acroform`). The output is **attorney-review-ready, never auto-filed**: the workflow spec parks it at
`staff_review` before any filing step, enforced by `workflows::staff_review_gates_filing` (a spec-graph check + test).
Two loud-failure guardrails — XFA-based forms (Adobe's XML form layer, unsupported by any Rust crate) are detected and
rejected rather than silently emitting a blank, and a field name that matches no form field errors rather than being
silently dropped. Hierarchical (kids / dotted `/T`) field names remain out of scope.

## External integrations

- **Email — implemented.** `email_send__*` → SendGrid via `reqwest`, durable, with the message id captured for the event
  webhook join. This is the one integration wired end-to-end.
- **E-signature — implemented (the production dead-end is closed).** The `SignatureProvider` trait now ships a concrete
  `DocuSignSignatureProvider` (DocuSign eSignature REST via `reqwest`, `.env`-driven; the stub stays for KIND / tests).
  At send time the provider's `envelopeId` is persisted on `notations.signature_request_id`. The inbound webhook at
  `/webhook/esignature/:secret` (`web::esignature_webhook`) **verifies an HMAC-SHA256 signature over the raw body before
  parsing it** — fail-closed when `DOCUSIGN_HMAC_KEY` is configured (a prod invariant) — then resolves the envelope id
  back to its notation and signals `signature_received` → END. Only a `completed` event advances state; other events ack
  with 200. The engagement terms are attorney-reviewed at `staff_review` *before* the document is sent, so signature
  receipt is a ministerial transition with no second human gate. Covered by a `.feature` (happy + forgery) and an
  end-to-end integration test through the real provider against a mocked endpoint.
- **Google Drive per-project sync — removed.** The per-Project archive is the append-only git repo served from `web`
  (see [`git-project-repos.md`](git-project-repos.md)); the `projects.drive_folder_id` column, the `DriveSync` Restate
  workflow, the `aida_drive_*` MCP tools, and the web/CLI sync surfaces have all been dropped. The `cloud::drive` OAuth
  door (the `cli drive login` / `cli drive ls` installed-app flow) is kept for ad-hoc browsing, but Drive is no longer a
  document-ingest surface.

## Roadmap

Ordered by value, each item independently shippable. Reliability fixes are split out from the features they ride with so
neither blocks the other.

**Recently shipped.** The full ten-template catalog is now bundled into the canonical seed, so a fresh cluster carries
every template (LLC, trust, will, annual report, dissolution, three nonprofit forms, NV MBT) without an import pass.
**The signature loop is closed** — `DocuSignSignatureProvider` plus the HMAC-verified `/webhook/esignature/:secret`
route advance a signed retainer to END (see External integrations above).

1. ~~**Close the signature loop.**~~ **Shipped.** A real `DocuSignSignatureProvider` plus an inbound webhook that
   verifies the provider's HMAC signature over the raw body before signaling `signature_received`; the provider
   request-id is persisted on the notation for correlation. This ended the production dead-end at
   `sent_for_signature__pending`.
2. ~~**Make Drive sync Restate-durable (reliability).**~~ **Removed.** The per-project Drive sync (the `DriveSync`
   workflow, the `drive_folder_id` column, the `aida_drive_*` tools) has been dropped in favour of the append-only
   per-Project git repo as the document surface. Drive is no longer an ingest path.
3. ~~**Add Drive write-back (feature).**~~ **Dropped** with the per-project Drive sync above — the per-Project git repo
   is the document system of record now, not a Drive folder.
4. ~~**AcroForm form-filling.**~~ **Shipped.** `pdf::fill_acroform(blank_pdf, fields)` (lopdf) fills a fillable
   government form; a `document_open__<form>` sub-slug dispatches it through the worker step, with blank forms held in
   `cloud::StorageService`. Output is **attorney-review-ready, never auto-filed** — the spec-graph guardrail
   `staff_review_gates_filing` proves no fill→file path skips `staff_review`. XFA forms and unmatched field names fail
   loudly rather than emitting a silent blank.
5. ~~**Promote the planned filing/mail steps to real handlers.**~~ **Shipped.** `mailroom_send`, `certified_mail`,
   `e_filing`, and `filing__*` are worker-dispatched steps that record a durable `filings` row (the firm's proof of what
   was filed) in `ctx.run`; compliance flows (e.g. the Nevada annual report) run end-to-end to END instead of parking.
   `staff_review_precedes_submission` proves — on every bundled spec — that no submission side effect fires before the
   review gate. (`notarization` stays a human act; `mailroom_receive` is inbound.)
6. ~~**Make language access explicit in intake.**~~ **Shipped.** `persons.preferred_language` (BCP-47, default `en`)
   plus a `question_translations` table of attorney-reviewed localized prompts; `notation_session` renders every
   questionnaire prompt in the person's language (web form + AIDA MCP/A2A surfaces, one convergence point), falling back
   to the English base when a translation is absent. Spanish ships seeded for the retainer questions. Translation is
   reviewed copy, not runtime machine translation — the `staff_review` gate and legal copy stay attorney-reviewed in
   each language. The questionnaire *prompt* is the only localized surface here: the **template body** — the binding
   document a client signs — stays English-only regardless of the client's language. See the English-first rule in
   [`../CLAUDE.md`](../CLAUDE.md).
7. ~~**Template storage and scoping.**~~ **Shipped.** Template bodies moved from the inline `templates.body` TEXT
   column to blob storage (`templates.blob_id` → a Blob via `cloud::StorageService`); `templates.project_id` plus two
   partial unique indexes add project-scoped templates alongside the shared catalog, resolved by
   `store::templates::resolve` (prefer Project, fall back to shared). The seed + `navigator import` paths ingest bodies
   into blobs; render paths read them back via `store::templates::body`. See [`notation.md`](notation.md).

## Why this matters — access to justice

The whole point of the notation system is to make routine legal work cheap, fast, and repeatable without removing the
attorney. Each design choice traces back to that mission ([`mission.md`](../web/content/marketing/mission.md)):

- **One template, many matters.** A lawyer encodes a matter once; every future client walks the same validated
  questionnaire. The marginal cost of the next LLC, trust, or annual report trends toward zero, which is what lets the
  firm serve people a billable-hour model prices out.
- **Faster resolution, lower cost.** A guided questionnaire plus automatic document generation collapses what used to be
  multiple back-and-forth meetings into a single self-serve intake the client finishes in minutes — answered in their
  own words through AIDA, on whichever surface they already use.
- **The human stays in the loop.** `staff_review` is mandatory by rule, not by convention. Automation does the
  repetitive assembly; a licensed attorney signs off on the substance. Faster *and* accountable, not faster *instead of*
  accountable.
- **Auditable by construction.** Append-only `notation_events` means every matter has a complete, replayable history —
  the transparency a public-interest practice owes the people it serves, and the record that lets one attorney safely
  oversee far more matters than a paper process ever could.

Speed here is not a convenience feature; it is the access-to-justice mechanism. Every minute and dollar the notation
system removes from a routine matter is one that a person who could not otherwise afford a lawyer gets to keep.
