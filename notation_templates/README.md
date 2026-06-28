# Notation

This tree holds Neon Law Navigator's markdown notation templates: static legal blueprints whose frontmatter declares a
questionnaire and workflow, and whose body supplies the legal prose. When a Template is bound to a respondent and
Project, it becomes a **Notation** — the running instance whose questions are answered and whose workflow advances to
review, signature, filing, or closing. The vocabulary is taught in [`docs/notation.md`](../docs/notation.md); this
README is about how the tree is organized and named.

Every template has YAML frontmatter with `title`, `code`, `jurisdiction`, `respondent_type`, `confidential`, and the
`questionnaire:` / `workflow:` state machines. The body is legal prose with `{{question_code}}` placeholders.

## Two shelves

The tree has exactly two top-level shelves:

```text
notation_templates/
├── forms/
└── neon_law/
```

`forms/` holds government form-backed templates. Its paths mirror the public assets bucket. If the blank PDF is stored
at `gs://<assets-bucket>/forms/united_states/nevada/state/nv__llc_formation.pdf`, the local canonical copy lives at:

```text
notation_templates/forms/united_states/nevada/state/nv__llc_formation.pdf
notation_templates/forms/united_states/nevada/state/nv__llc_formation.fields.toml
notation_templates/forms/united_states/nevada/state/nv__llc_formation.md
```

The markdown file is the catalog card and workflow. Its `code` is the form identity:

```yaml
title: Nevada LLC Formation
code: nv__llc_formation
jurisdiction: NV
origin_url: https://www.nvsos.gov/businesses/commercial-recordings/forms-fees/all-business-forms
respondent_type: person_and_entity
confidential: false
form: nv__llc_formation
```

`origin_url` is the government page where the blank can be obtained. Git records the exact bytes we vendored; the URL
records where those bytes came from.

`neon_law/` holds firm-authored product templates and trademarked Neon Law work product. Each product gets its own
folder, and shared firm documents live under `shared/`:

```text
notation_templates/neon_law/
├── nautilus/retainer.md
├── nest/retainer.md
├── nexus/retainer.md
├── northstar/retainer.md
└── shared/closing_letter.md
```

These files are public so you can read and learn from them, but the marks are reserved. **"Neon Law"** is a registered
trademark of Shook Law PLLC (U.S. Reg. No. 6,325,650); see the [Trademarks note in the root
`README.md`](../README.md#trademarks). A fork must rebrand `neon_law/` through the white-label seam before shipping it.

## Naming convention

The `navigator validate` command enforces these with the N-family notation rules:

1. **Only `forms/` and `neon_law/` are valid top-level shelves.**
2. **Every template declares `jurisdiction:`**, using a code from `store/seeds/Jurisdiction.yaml` such as `NV`, `CA`, or
   `US`.
3. **Form codes are jurisdiction-first**: `nv__llc_formation`, `us__form_990`. The filename stem, `code`, and `form`
   binding match.
4. **Product codes are product-first**: `nest__retainer`, `northstar__closing_letter`, or the existing workflow code
   while a compatibility migration is still in flight.
5. **Every path segment is lowercase `snake_case`**.

Run it before committing:

```bash
cargo run -p cli --quiet -- validate notation_templates
```

This `README.md` is linted like every other workspace README:

```bash
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes notation_templates/README.md
```

## Adding a form template

1. Put the blank PDF under the bucket-shaped local path:
   `notation_templates/forms/<country>/<jurisdiction>/<office>/<code>.pdf`.
2. Add a sibling `<code>.fields.toml` when the form is fillable.
3. Add a sibling `<code>.md` whose `code` matches the filename stem and whose `origin_url` is the government source.
4. Add the PDF to `forms/src/lib.rs` so the binary embeds the same bytes the repo carries.
5. Run `cargo run -p cli -- validate notation_templates` and the `forms` crate tests.
