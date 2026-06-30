# Notations

This tree holds Neon Law Navigator's **notations** — the executable form of the firm's legal work. A notation is one
markdown file that carries three things at once: the **template** (the legal prose the client signs), the
**questionnaire** that gathers the answers that fill it in, and the **workflow** that advances the document from intake
through attorney review to signature, filing, or closing. Templates, questionnaires, and workflows are not three
separate files — they are three faces of one notation.

When a notation is bound to a respondent and a Project it comes to life as a running **Notation** (capital N): the live
matter whose questions get answered and whose workflow advances. That runtime vocabulary is taught in
[`docs/notation.md`](../docs/notation.md); this page is about how the notation tree is organized, named, and checked.

Every notation has YAML frontmatter with `title`, `code`, `jurisdiction`, `respondent_type`, `confidential`, and the
`questionnaire:` / `workflow:` state machines. The body is legal prose with `{{question_code}}` placeholders. Every key
is explained, in plain English and for attorneys, in [`docs/frontmatter.md`](../docs/frontmatter.md).

## Two shelves

The tree has exactly two top-level shelves:

```text
templates/
├── forms/
└── neon_law/
```

`forms/` holds government form-backed templates. Its paths mirror the public assets bucket. If the blank PDF is stored
at `gs://<assets-bucket>/forms/united_states/nevada/state/nv__llc_formation.pdf`, the local canonical copy lives at:

```text
templates/forms/united_states/nevada/state/nv__llc_formation.pdf
templates/forms/united_states/nevada/state/nv__llc_formation.fields.toml
templates/forms/united_states/nevada/state/nv__llc_formation.md
```

The markdown file is the catalog card and workflow. Its `code` is the form identity:

```yaml
title: Nevada LLC Formation
code: nv__llc_formation
jurisdiction: NV
origin_url: https://www.nvsos.gov/businesses/commercial-recordings/forms-fees/all-business-forms
respondent_type: person_and_entity
confidential: false
output: form
form: nv__llc_formation
```

`origin_url` is the government page where the blank can be obtained. Git records the exact bytes we vendored; the URL
records where those bytes came from.

`neon_law/` holds firm-authored product templates and trademarked Neon Law work product. Each product gets its own
folder, and shared firm documents live under `shared/`:

```text
templates/neon_law/
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
cargo run -p cli --quiet -- validate templates
```

This `README.md` is linted like every other workspace README (the validator classifies each file automatically, so there
is no mode flag to pass):

```bash
cargo run -p cli --quiet -- validate --no-default-excludes templates/README.md
```

## Authoring with live feedback — the LSP

You do not have to run `validate` by hand to find a problem. The same rule engine ships as a small language server,
`navigator-lsp`, that any editor (VS Code, Zed, Neovim, Helix, Emacs) can attach to `*.md`. As you type a notation it
underlines what is wrong, in place:

- a **red** underline is a blocking error — a missing `title`, an unknown `respondent_type`, a `workflow` with no
  `staff_review`, a notation that declares only one of `questionnaire:` / `workflow:`;
- a **yellow** underline is a non-blocking advisory — most often a workflow step that is allowed but not built yet.

Hover any underline for the rule and the fix. The server runs entirely on your machine and sends nothing anywhere — the
same confidentiality the `confidential:` key is there to protect. The frontmatter keys it checks are documented for
attorneys in [`docs/frontmatter.md`](../docs/frontmatter.md); editor setup is in
[`docs/lsp/README.md`](../docs/lsp/README.md).

## Adding a form template

1. Put the blank PDF under the bucket-shaped local path:
   `templates/forms/<country>/<jurisdiction>/<office>/<code>.pdf`.
2. Add a sibling `<code>.fields.toml` when the form is fillable.
3. Add a sibling `<code>.md` whose `code` matches the filename stem and whose `origin_url` is the government source.
4. Add the PDF to `forms/src/lib.rs` so the binary embeds the same bytes the repo carries.
5. Run `cargo run -p cli -- validate templates` and the `forms` crate tests.
