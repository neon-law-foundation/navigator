# notation_templates

These are the blueprints the firm uses to produce your legal documents. Each file here is a **Notation Template**: a
static markdown document that, once assigned to a person or entity, produces a **Notation** — the filled-in instance an
attorney reviews, signs, and files. The vocabulary (Template, Notation, Questionnaire, Question, Answer) is taught in
[`docs/notation.md`](../docs/notation.md); this README is about how the tree is **organized** and **named**.

Every file is markdown with a YAML frontmatter block carrying `title`, `code`, `respondent_type`, `confidential`, and
the `questionnaire:` / `workflow:` state machines. The body is the legal prose with `{{question_code}}` placeholders.

## Why the folders are shaped this way

A notation template is law, and law is organized by **where it applies** before **what it is about**. So the path
codifies the jurisdiction in the mark on file itself — you can read a template's reach from its location without opening
it. The tree has two branches.

### Substantive law — `<jurisdiction>/<scope>/<bar_exam_topic>/<name>.md`

Documents that are *about a body of law* live under a jurisdiction, then a scope, then a bar-exam topic:

```text
notation_templates/
└── united_states/
    ├── federal/
    │   └── <bar_exam_topic>/...
    ├── nevada/
    │   └── <bar_exam_topic>/...
    ├── california/
    │   └── <bar_exam_topic>/...
    └── washington/
        └── <bar_exam_topic>/<name>.md
```

- **Jurisdiction** — the sovereign. Lowercase `snake_case`: `united_states` (others, e.g. `germany`, are added when
  the firm practices there).
- **Scope** — `federal`, or one of the firm's states of admission: `nevada`, `california`, `washington`. The closed
  list is the firm's actual bar admissions, never a state the firm cannot practice in.
- **Bar exam topic** — the body of law, drawn from the standard MBE/MEE subject list so the folder reads the way a
  lawyer already files law in their head:

  `business_associations`, `civil_procedure`, `conflict_of_laws`, `constitutional_law`, `contracts`,
  `criminal_law_and_procedure`, `evidence`, `family_law`, `real_property`, `secured_transactions`, `torts`,
  `trusts_and_estates`.

### Operational templates — the parallel branch

A retainer letter, a closing letter, a compliance filing, and a consumer-protection demand letter are *not* about a
bar-exam subject — they are how the firm runs an engagement. Forcing them under a bar topic would misrepresent them, so
they live in their own branch:

- `engagements/` — engagement / onboarding letters and intake (retainers, the estate-planning intake, fractional-GC
  onboarding).
- `correspondence/` — client and third-party letters (closing letters, the consumer-debt-defense letter set).
- `filings/` — government compliance and tax filings (Nevada Modified Business Tax, IRS Form 990).
- `services/` — service-delivery work products (contract review).

## Naming convention (enforced by the CLI)

The `navigator validate` command enforces these with the **N-family** notation rules — `N103` (snake_case filename),
`N108` (every template declares a stable `code`), and `N110` (the jurisdiction-path grammar):

1. **Every path segment is lowercase `snake_case`** — no spaces, hyphens, PascalCase, or doubled underscores.
2. **A file under a known jurisdiction must match `<jurisdiction>/<scope>/<bar_exam_topic>/<name>.md`**, with the
   scope and topic drawn from the closed lists above. An unknown jurisdiction, scope, or topic is a violation, not a
   pass — the rule **fails closed** so the convention cannot quietly rot.
3. **A template is uniquely identified by its `code`** — the questionnaire/workflow key. `code` values are unique
   across the whole tree; `navigator validate` reports any duplicate.

Run it before committing:

```bash
cargo run -p cli --quiet -- validate notation_templates
```

This `README.md` is linted like every other workspace README (M-family + `S101` only, N-family skipped):

```bash
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes notation_templates/README.md
```

## The `.gitkeep` skeleton

The full jurisdiction × scope × bar-exam-topic skeleton is committed up front, with a `.gitkeep` in each topic folder
that has no template yet. The skeleton is the map: a new template drops into the folder that already names its
jurisdiction, scope, and topic, instead of inventing a path.

## Migration status

This tree is mid-migration. The substantive skeleton and the operational branch exist and are enforced for any file
placed inside them. The legacy flat folders (`trust/`, `will/`, `llc/`, `nest/`, `nonprofit/`, `onboarding/`,
`nautilus/`, …) are **grandfathered**: they are still valid notation templates and still lint under the N-family, but
they predate the jurisdiction grammar and are relocated in a follow-up. The intended destinations:

| Legacy path | Destination |
| --- | --- |
| `trust/nevada.md`, `will/simple.md`, `northstar/*.md` | `united_states/nevada/trusts_and_estates/` |
| `onboarding/estate.md` | `engagements/` |
| `llc/california.md` | `united_states/california/business_associations/` |
| `nest/*.md`, `annual_report/nevada.md` | `united_states/nevada/business_associations/` |
| `dissolution/nevada.md`, `nonprofit/nevada_*.md` | `united_states/nevada/business_associations/` |
| `nonprofit/form990_annual_report.md` | `filings/federal/` |
| `nv_state_tax_filing/modified_business_tax.md` | `filings/nevada/` |
| `nautilus/*.md`, `closing/letter.md` | `correspondence/` |
| `onboarding/retainer*.md`, `nexus/fractional_gc.md` | `engagements/` |
| `services/contract_review.md` | `services/` |

The public template gallery serves a curated subset over the `/api/templates/...` route; relocating the gallery's three
nonprofit entries to deep paths is part of that same follow-up, since it changes the route shape.

## Adding a new template

1. Find the folder that already names the jurisdiction, scope, and topic (or the right operational bin).
2. Drop a markdown file named after the document, in `snake_case` (`united_states/nevada/trusts_and_estates/trust.md`).
3. Frontmatter: `title`, a stable, unique `code` (e.g. `trusts__nevada`), `respondent_type`, `confidential`, plus the
   `questionnaire:` and `workflow:` state machines.
4. Body: legal prose with `{{question_code}}` placeholders that reference codes declared in `questionnaire:`.
5. Run `cargo run -p cli -- validate notation_templates` until it exits `0`.
