# forms

Vendored government forms — the bundled registry behind `notation_templates/forms/`.

Every official form Neon Law Navigator fills and files is vendored from its canonical source (the issuing authority's
own domain, e.g. `nvsos.gov`) and pinned in
[`notation_templates/forms/FORMS.toml`](../notation_templates/forms/FORMS.toml) by printed revision and SHA-256. This
crate bundles the ledger and the PDF bytes into the binary so every consumer — the workflow walker building an AcroForm
document payload, the web download routes, the `cli forms sync` uploader — reads the same bytes the repo committed, with
no network or bucket dependency.

The acquisition discipline (canonical source only, no Wayback or mirrors, canonical example on disk before any field
map, one commit per refresh) lives in the `vendor-gov-forms` skill. The guard test (`tests/vendored_forms.rs`)
recomputes each `sha256` from the bundled bytes and cross-checks the on-disk file, so the ledger, the bundle, and the
working tree cannot drift apart silently.

## API

- `forms::registry()` — every vendored form: ledger metadata + `&'static [u8]` PDF bytes. `forms::get(form_code)` — one
  form by its stable `form_code` (e.g. `nv_sos__llc_formation`).
