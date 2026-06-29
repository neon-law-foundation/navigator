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
workflows::dispatch_document_open → pdf::fill_acroform
   │
   ▼
staff_review → signature / filing
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
