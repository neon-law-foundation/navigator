# forms

Vendored government forms — the bundled registry behind `notation_templates/forms/`.

Every official form Neon Law Navigator fills is stored under the same path it uses in the public assets bucket. For
example:

```text
notation_templates/forms/united_states/nevada/state/nv__llc_formation.pdf
```

syncs to:

```text
gs://<assets-bucket>/forms/united_states/nevada/state/nv__llc_formation.pdf
```

The sibling markdown template is the catalog card (`code`, `jurisdiction`, `origin_url`, questionnaire, workflow), and
the sibling `.fields.toml` maps questionnaire answers onto the PDF's AcroForm field names.

## API

- `forms::registry()` — every bundled form: metadata + `&'static [u8]` PDF bytes.
- `forms::get(code)` — one form by its stable code, e.g. `nv__llc_formation`.
- `forms::field_map(code)` — the sibling field map, when the form is fillable.
