# Government Forms: Vendor, Map, Fill, File

Neon Law Navigator fills official government PDF forms from questionnaire answers and files them through a staff-gated
workflow. The repository path is the storage contract: a blank at
`templates/forms/united_states/nevada/state/nv__llc_formation.pdf` syncs to
`forms/united_states/nevada/state/nv__llc_formation.pdf` in the public assets bucket.

## Pipeline

```text
government website (`origin_url`)
   │
   ▼
templates/forms/<country>/<jurisdiction>/<office>/<code>.pdf
templates/forms/<country>/<jurisdiction>/<office>/<code>.fields.toml
templates/forms/<country>/<jurisdiction>/<office>/<code>.md
   │
   ▼
forms crate registry + field-map resolution
   │
   ▼
web::retainer_walk::render_and_park
   │
   ▼
staff_review → workflows::dispatch_document_open → pdf::fill_acroform → pdf::flatten
   │
   ▼
signature / filing
```

The `.md` file is the catalog card and workflow. It declares the form identity:

```yaml
code: nv__llc_formation
jurisdiction: NV
origin_url: https://www.nvsos.gov/businesses/commercial-recordings/forms-fees/all-business-forms
form: nv__llc_formation
```

`code`, the filename stem, and the `form:` binding match. `origin_url` points at the government page where the blank can
be obtained. Git records the vendored bytes; the URL records provenance.

## Field Maps

Each fillable form carries a sibling `<code>.fields.toml` mapping exact AcroForm `/T` names to answer sources. Real
government field names can be hostile (`undefined`, `City_5`, misspellings baked into the official file), so maps are
data and tests pin them. `forms::resolve` converts questionnaire answers into the field map consumed by
`pdf::fill_acroform`.

Payment-card fields and staff-side acceptance pages stay unmapped. If a government PDF reuses the same widget across
sub-forms, the unsafe field stays unmapped until a staff review path can handle it deliberately.

## The Field-Name = Question-Code Contract

The fill map is not trusted, it is **checked**. Since the question consolidation
([#233](https://github.com/neon-law-foundation/navigator/issues/233)), a notation's questionnaire states are named
`<type>__<role>` — `entity__company`, `person__registered_agent`, `people__managing_members`,
`custom_single_choice__management_structure` — where `<type>` is one of the canonical seeded question codes in
[`Question.yaml`](../store/seeds/Question.yaml). A field map's answer sources are those same states (directly or by
`__role` suffix, exactly as `fieldmap::answer_for` resolves an answer). So a map that fills a real filing must reference
only questions the questionnaire actually asks, of types the workspace actually seeds.

`forms/tests/question_code_contract.rs` is the guard, run offline in `cargo test` with no PDF or network:

- **`every_notation_state_is_a_canonical_question_type`** — every questionnaire state in each vendored form's `.md` has
  a `<type>` that is a canonical question code (`rules::canonical_question_codes()`, the same source of truth the
  notation-template linter uses).
- **`every_mapped_question_resolves_to_a_declared_state`** — every `question` (and `present_in`) in each `.fields.toml`
  resolves to a state that form's notation declares.

A guessed map, a renamed question, or a notation that drifted from its map fails CI here — before a mis-mapped field can
mis-fill a filing.

## Sequenced Follow-Ons

The end state is that the AcroForm `/T` names **are** the question-code paths, retiring the `.fields.toml` indirection:
a human re-authors each government blank's field layer (Acrobat "Prepare Form" / `pdftk`) so `/T` names become
`entity__company.name`, a radio group named `custom_single_choice__management_structure` carries its choices as
on-states, and dotted `people__managing_members.0.address.city` addresses list rows. Named fields give radio exclusivity
and comb fields for free and break loudly on a re-vendor. Until that re-authoring lands, the three NV blanks keep their
`.fields.toml` on the path above, and the guard test pins that layer.

The filled packet is **flattened** before it is persisted. Because a form's fill state (`document_open__*_pdf`) sits
past `staff_review` in every packet's workflow spec, `dispatch_document_open` runs `pdf::flatten` right after
`pdf::fill_acroform`: it paints every value onto the page (text as page content, a checked box as its own appearance
stream), drops the widget annotations, and empties the AcroForm `/Fields`. The result freezes exactly what an attorney
approved — no downstream viewer can re-edit a value on the way to a government office, and a viewer that ignores
`/NeedAppearances` shows the filled values rather than a blank form.

One further follow-on is tracked, not yet built:

- **Blank in GCS, repo stays text-only.** The vendored PDF is binary, re-vendored often, and never diffs — a candidate
  to move out of git and pull through `cloud::StorageService` at fill time, pinned by a sibling `.sha256` so a silent
  re-vendor fails loudly instead of mis-filling. The repo would keep only diffable text (`.md`, the field manifest, the
  sha pin). Trade-off: filling then requires the blank present in the bucket or a warmed cache.

## Runtime Storage

Blank forms are public assets. `navigator forms sync` uploads each registry entry to its `object_path`:

```text
forms/united_states/nevada/state/nv__llc_formation.pdf
```

Filled forms are client documents. They are rendered into the private documents bucket at:

```text
notations/<notation-id>/document.pdf
```

Signed copies and certificates use the same per-notation namespace:

```text
notations/<notation-id>/signed-document.pdf
notations/<notation-id>/certificate-of-completion.pdf
```

## Adding A Form

1. Download the blank from the government `origin_url`.
2. Store it at the bucket-shaped repo path under `templates/forms/`.
3. Add a sibling markdown template with matching `code`, `jurisdiction`, `origin_url`, and `form`.
4. Add a sibling field map if the PDF is fillable.
5. Add the PDF and field map to the `forms` crate registry.
6. Run `cargo test -p forms` and `cargo run -p cli -- validate templates`.
