# Notation vocabulary

This doc holds the notation-system vocabulary — what the markdown templates produce, how they're filled in, and the
rules that validate them. It is kept in teaching order rather than alphabetically, because Template precedes Notation by
design: you read what a Template *declares* before you read what a Notation *runs*. The rest of the workspace vocabulary
lives in [`glossary.md`](glossary.md).

## Template

A **static blueprint.** A markdown file with a YAML frontmatter block — `title`, `code`, `respondent_type`, and the
`questionnaire:` and `workflow:` specs — plus a body of legal prose with `{{question_code}}` placeholders.

A Template *declares*: which Questions to ask, in what order, what workflow advances the resulting document, who the
respondent is, what the document is titled. It asks nothing on its own. Until a respondent is bound to it (see
[Notation](#notation)), it is inert — a file on disk, useful for linting and preview, but no questions have been asked
and no workflow has run.

Identified by a stable `code` like `nv__llc_formation`, `ca__llc_operating_agreement`, or `onboarding__retainer_nest`.

- Schema: [`store::entity::template`](../store/src/entity/template.rs) Files: [`templates/`](../templates/) — exactly
  two top-level shelves: `forms/<country>/<jurisdiction>/<office>/<code>.md` for government forms, and
  `neon_law/<product>/<document>.md` for Neon Law product work.

> **Storage.** The markdown body lives in [`cloud::StorageService`](../cloud/) like every other artifact: the
  `templates.body` TEXT column is gone; `templates.blob_id` references a [Blob](glossary.md#blob) holding the bytes.
  Read the body via [`store::templates::body`](../store/src/templates.rs); the seed and `navigator import` paths ingest
  it (sha-dedup). The Project's archive folder plays no role — Templates are workspace-scoped code-like assets governed
  by git, not by the per-Project archive.

> **Workspace-shared vs project-scoped.** A Template is workspace-shared (`templates.project_id IS NULL`, the public
  catalog default) or scoped to a single Project. Project-scoped rows are hidden from the public Template list (cli
  `list`, the admin surface) and resolved only under that Project;
  [`store::templates::resolve`](../store/src/templates.rs) prefers the caller's Project, falling back to the shared row.
  Two partial unique indexes on `code` enforce the rule. The shared index keeps workspace-shared codes globally unique
  (`ca__llc_operating_agreement`, `onboarding__retainer`); the per-Project index on `project_id` and `code` lets each
  Project reuse short codes (`amendment`, `consent`) without colliding with another Project's.

> **Jurisdiction.** Every Template declares a `jurisdiction:` code that resolves to
  [`store/seeds/Jurisdiction.yaml`](../store/seeds/Jurisdiction.yaml). Government form templates also encode the
  jurisdiction in their `code`: `NV` maps to `nv__...`, `US` maps to `us__...`, and the markdown filename stem must
  match that code. The government provenance URL is `origin_url`, not a checked-in checksum or revision field; git
  tracks the vendored bytes.

## Notation

A Template **come to life.** One running instance of a Template, bound to a specific [Person](glossary.md#person) — the
respondent — a [Project](glossary.md#project), and optionally an [Entity](glossary.md#entity), carrying a workflow
`state` such as `draft`, `staff_review`, or `signed`.

> **Client English.** A Notation in the context of its Project is what clients call an
  **[Engagement](glossary.md#engagement--retainer)** (or a **Retainer**, when the bound Template is a retainer). The
  schema noun is `Notation`; the marketing noun is Engagement.

The Questions the Template declared are *asked* here; the [Answers](#answer) the respondent gives are stored against
this Notation; the workflow runs against this Notation. Where a Template is static, a Notation has a lifetime — born
when a matter opens, closed when its workflow terminates. **In our legal practice, the unit of work is a Notation:**
opening a new matter creates one; walking a client through engagement, intake, and signing advances it through its
states.

- Schema: [`store::entity::notation`](../store/src/entity/notation.rs) Lives in: `notations` table

> **Note — two meanings.** "Notation" is also the umbrella name for Neon Law Navigator's markdown notation format (the
  file format Templates are written in). Templates *are* notations in that sense; each row in the `notations` table is
  one running instance of one. The format name is older than the schema; both meanings stuck. Disambiguate by context:
  capitalized and referring to a row or a matter, it's the runtime instance; referring to the file format, it's the
  lowercase "markdown notation."

## Questionnaire

The ordered list of [Questions](#question) a Template **declares** it will ask. Lives entirely in the template's
frontmatter under `questionnaire:`. Not a separate table — the questionnaire is what you get when you read a Template's
frontmatter and resolve each entry against the `questions` table.

When a [Notation](#notation) runs, *those* are the prompts the respondent sees and the [Answers](#answer) get attached
to. **The Template declares the questionnaire; the Notation asks it.**

> **Status — declared and walked.** The questionnaire state machine is structurally validated by the [`N104` rule
  implementation](../rules/src/f104.rs) **and** executed step-by-step by
  [`web::retainer_walk`](../web/src/retainer_walk.rs): one question per request, one [Answer](#answer) per advance, one
  [Notation Event](glossary.md#notation-event) per transition. The walker shares its runtime surface with the [Workflow
  Runtime](glossary.md#workflow-runtime) — both implement `workflows::StateMachineRuntime`, keyed by `MachineKind` and
  `notation_id` — so a single Restate virtual object per Notation hosts both timelines on one logical journal. See
  [`docs/retainer_intake.md`](retainer_intake.md) for the end-to-end walkthrough.

### Conversational notation (MCP)

The same questionnaire state machine is also driven from outside the HTML form via two MCP tools: `aida_create_notation`
(start the Notation, get the first question) and `aida_answer_notation` (submit one answer, get the next question or
"complete"). The LLM client is the UI; the server owns the state. Both the form and the MCP tools call the same
[`workflows::notation_session`](../workflows/src/notation_session.rs) service, so changes to the walking logic touch
exactly one codepath. See [`mcp/README.md`](../mcp/README.md) for the client-side loop.

### Language access

Each Person carries a `preferred_language` (BCP-47, default `en`). `notation_session::load_question` renders each prompt
in that language from the `question_translations` table — attorney-reviewed localized copy keyed by `question_id` and
`locale` — falling back to the English base prompt when no translation exists. Because both the HTML form and the MCP /
A2A tools resolve the prompt through that one service, intake is multilingual on every surface at once. Translation is
reviewed copy, not runtime machine translation: the `staff_review` gate and all legal copy stay attorney-reviewed in
each language. Spanish (`es`) ships seeded for the retainer questions.

## Question

One prompt presented to a respondent during Template traversal. Identified by a stable `code` (e.g. `client_name`,
`organizer_state`). Has an `answer_type` — `string`, `int`, `bool`, `choice`, etc. — that the form layer uses to render
the right input. When a questionnaire state uses the typed grammar `<type>__<role>`, its `<type>` prefix is a [Question
Type](glossary.md#question-type) from `store::question_registry` (record / reference / custom, singular / plural) — the
closed vocabulary `N113`–`N117` and the render/form-fill evaluator all share. Use glossary-backed states and dotted
fields for durable nouns: `person__client` with `{{person__client.name}}`, not `custom_text__client_name`.

- Schema: [`store::entity::question`](../store/src/entity/question.rs) Lives in: `questions` table Seed:
  [`store/seeds/Question.yaml`](../store/seeds/Question.yaml)

## Answer

One respondent's answer to one Question. Deduplicated by `(question, person, value)`, so re-submitting the same value is
a no-op.

- Schema: [`store::entity::answer`](../store/src/entity/answer.rs) Lives in: `answers` table

## Rule

A validation check applied to markdown notations by the [`rules`](../rules/) crate. Three families:

- **M-family** — generic Markdown hygiene (headings, list spacing, code-fence languages, link targets). **N-family** —
  Neon Law Navigator notation template shape (required keys, question-code resolution, template/workflow
  well-formedness).
- **S101** — the 120-character line-length limit. Applies to every `.md` file in the workspace.

The `cli validate` subcommand runs the relevant subset per file.
