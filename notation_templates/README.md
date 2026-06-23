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

### Substantive law — `<jurisdiction>/<scope>/<forum>/<practice_area>/<name>.md`

Documents that are *about a body of law* live under a jurisdiction, then a scope, then the forum they are filed with,
then a practice area:

```text
notation_templates/
└── united_states/
    ├── federal/
    │   └── <forum>/<practice_area>/<name>.md
    ├── nevada/
    │   └── <forum>/<practice_area>/<name>.md
    ├── california/
    │   └── <forum>/<practice_area>/<name>.md
    └── washington/
        └── <forum>/<practice_area>/<name>.md
```

- **Jurisdiction** — the sovereign. Lowercase `snake_case`: `united_states` (others, e.g. `germany`, are added when the
  firm practices there). Each root is backed by a row in the `jurisdictions` table; a cross-crate reconciliation test
  keeps the path vocabulary and the seeded reference data in sync.
- **Scope** — `federal`, or one of the firm's states of admission: `nevada`, `california`, `washington`. The closed
  list is the firm's actual bar admissions, never a state the firm cannot practice in.
- **Forum** — the counterparty or sovereign the document is filed with, or `private` when there is no government
  counterparty. The forum sits **above** the practice area because that is how the firm's filing and workflow
  integrations are organized. It is **mandatory**: a document with no government counterparty is `private`, not absent.
  The closed list: `private`, `state`, `clark_county`, `washoe_county`, `secretary_of_state`, `department_of_taxation`,
  `irs`, `uspto`. Counties and agencies live here, not in the `jurisdictions` table.
- **Practice area** — the body of law, drawn from the standard MBE/MEE subject list **plus the firm's own practice
  areas** the bar list does not cover (`debt_relief`, `taxation`, `intellectual_property`, `immigration`,
  `landlord_tenant`), so the folder reads the way a lawyer already files law in their head:

  `business_associations`, `civil_procedure`, `conflict_of_laws`, `constitutional_law`, `contracts`,
  `criminal_law_and_procedure`, `debt_relief`, `evidence`, `family_law`, `immigration`, `intellectual_property`,
  `landlord_tenant`, `real_property`, `secured_transactions`, `taxation`, `torts`, `trusts_and_estates`.

### Operational templates — the parallel branch

A retainer letter, a closing letter, a compliance filing, and a consumer-protection demand letter are *not* about a
bar-exam subject — they are how the firm runs an engagement. Forcing them under a bar topic would misrepresent them, so
they live in their own branch:

- `engagements/` — engagement / onboarding letters and intake (retainers, the estate-planning intake, fractional-GC
  onboarding).
- `correspondence/` — client and third-party letters (a generic closing letter and other one-off correspondence).
- `filings/` — government compliance filings that are not tied to a single jurisdiction-and-practice-area. Tax filings
  that *are* (Nevada Modified Business Tax, IRS Form 990) live in the substantive tree under `.../taxation/`, coded by
  their forum (`state`, `irs`).
- `services/` — service-delivery work products (contract review).

### Neon Law — brand-specific templates (trademark-encumbered)

The substantive and operational branches above are **de-branded on purpose**: they name the law (`united_states/...`) or
the function (`engagements/`, `filings/`), never a product. That keeps them safe for a fork to adopt under its own name.

`neon_law/` is the exception — the firm's own brand-specific templates: the named-service retainers and any work product
that carries a Neon Law product mark (Nautilus, Nest, Northstar, Nexus, and the like). It lives at the top level,
parallel to `united_states/`, so the brand is quarantined in one place instead of leaking into the body of law.

These files are shared publicly so you can read and learn from them, **but they are not yours to use as-is.** **"Neon
Law"** is a registered trademark of Shook Law PLLC (U.S. Reg. No. 6,325,650) — see the [Trademarks note in the root
`README.md`](../README.md#trademarks). A fork **must not ship anything under `neon_law/` without first stripping the
marks** and adopting its own name (the `navigator rebrand` white-label seam). The license covers the code, not the name.

Like the operational branch, `neon_law/` carries no jurisdiction segment, so the `N110` jurisdiction-path grammar says
nothing about it.

## Naming convention (enforced by the CLI)

The `navigator validate` command enforces these with the **N-family** notation rules — `N103` (snake_case filename),
`N108` (every template declares a stable `code`), and `N110` (the jurisdiction-path grammar):

1. **Every path segment is lowercase `snake_case`** — no spaces, hyphens, PascalCase, or doubled underscores.
2. **A file under a known jurisdiction must match `<jurisdiction>/<scope>/<forum>/<practice_area>/<name>.md`**, with
   the scope, forum, and practice area each drawn from the closed lists above. An unknown jurisdiction, scope, forum, or
   practice area — or the wrong path depth — is a violation, not a pass; the rule **fails closed** so the convention
   cannot quietly rot.
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

## Folders are created as templates land

There is no pre-committed empty skeleton. A folder is created the moment a template needs it — the closed lists above
(scopes, forums, practice areas) plus this README are the map, so a new template drops into the path its jurisdiction,
scope, forum, and practice area already name instead of inventing one.

## Migration status

This tree is mid-migration. The operational branch and the brand quarantine exist, and the jurisdiction grammar is
enforced for any file placed under `united_states/`. The legacy flat folders (`trust/`, `will/`, `llc/`, `nest/`,
`nonprofit/`, `onboarding/`, `nautilus/`, …) are **grandfathered**: they are still valid notation templates and still
lint under the N-family, but they predate the jurisdiction grammar and are relocated in a follow-up. The substantive
work products are **de-branded** into the jurisdiction tree; only templates that name a Neon Law product (the
brand-named retainers) go to `neon_law/`. The intended destinations:

| Legacy path | Destination |
| --- | --- |
| `trust/nevada.md`, `will/simple.md`, `northstar/*.md` | `united_states/nevada/private/trusts_and_estates/` |
| `nest/*.md`, `annual_report/nevada.md` | `united_states/nevada/state/business_associations/` |
| `dissolution/nevada.md`, `nonprofit/nevada_*.md` | `united_states/nevada/state/business_associations/` |
| `llc/california.md` | `united_states/california/state/business_associations/` |
| `nautilus/*.md` (FDCPA/FCRA letters) | `united_states/federal/private/debt_relief/` |
| `nexus/fractional_gc.md` (work product) | `united_states/nevada/private/business_associations/` |
| `nonprofit/form990_annual_report.md` | `united_states/federal/irs/taxation/` |
| `nv_state_tax_filing/modified_business_tax.md` | `united_states/nevada/state/taxation/` |
| `closing/letter.md` | `correspondence/` |
| `onboarding/estate.md`, `onboarding/retainer.md` (generic) | `engagements/` |
| `services/contract_review.md` | `services/` |
| `onboarding/retainer_*.md` (brand-named) | `neon_law/engagements/` |

The public template gallery serves a curated subset over the `/api/templates/...` route; relocating the gallery's
nonprofit entries to deep paths is part of that same follow-up, since it changes the route shape.

## Adding a new template

1. Find (or create) the folder that names the jurisdiction, scope, forum, and practice area (or the right operational
   bin).
2. Drop a markdown file named after the document, in `snake_case`
   (`united_states/nevada/private/trusts_and_estates/trust.md`).
3. Frontmatter: `title`, a stable, unique `code` (e.g. `trusts__nevada`), `respondent_type`, `confidential`, plus the
   `questionnaire:` and `workflow:` state machines.
4. Body: legal prose with `{{question_code}}` placeholders that reference codes declared in `questionnaire:`.
5. Run `cargo run -p cli -- validate notation_templates` until it exits `0`.
