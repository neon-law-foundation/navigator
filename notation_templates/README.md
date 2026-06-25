# Notation

This tree holds Neon Law Navigator's markdown notation templates: static legal blueprints whose frontmatter declares a
questionnaire and workflow, and whose body supplies the legal prose. When a Template is bound to a respondent and
Project, it becomes a **Notation** — the running instance whose questions are answered and whose workflow advances to
review, signature, filing, or closeout. The vocabulary (Template, Notation, Questionnaire, Question, Answer) is taught
in [`docs/notation.md`](../docs/notation.md); this README is about how the tree is **organized** and **named**.

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
- **Forum** — the counterparty or sovereign the document is filed with, or `internal` when there is no government
  counterparty. The forum sits **above** the practice area because that is how the firm's filing and workflow
  integrations are organized. It is **mandatory**: a document with no government counterparty is `internal`, not absent.
  `internal` means "no government/sovereign on the other side" — it stays within the client relationship — **not**
  "internal to the firm." The closed list: `internal`, `state`, `clark_county`, `washoe_county`, `secretary_of_state`,
  `department_of_taxation`, `irs`, `uspto`. Counties and agencies live here, not in the `jurisdictions` table.
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
  `filings/` — government compliance filings that are not tied to a single jurisdiction-and-practice-area. Tax filings
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

## Adding a new template

1. Find (or create) the folder that names the jurisdiction, scope, forum, and practice area (or the right operational
   bin).
2. Drop a markdown file named after the document, in `snake_case`
   (`united_states/nevada/internal/trusts_and_estates/trust.md`).
3. Frontmatter: `title`, a stable, unique `code` (e.g. `trusts__nevada`), `respondent_type`, `confidential`, plus the
   `questionnaire:` and `workflow:` state machines.
4. Body: legal prose with `{{question_code}}` placeholders that reference codes declared in `questionnaire:`.
5. Run `cargo run -p cli -- validate notation_templates` until it exits `0`.
