# Government forms: vendor → map → fill → file

Navigator fills official government PDF forms from questionnaire answers and files them with the issuing authority. This
document is the end-to-end map of that pipeline — what each layer owns, where the data lives, and which guard test pins
each seam. The first three forms are the Nevada Secretary of State formation packets (LLC under NRS 86, profit
corporation under NRS 78, business trust under NRS 88A); the same pipeline is built to absorb thousands more.

## The pipeline at a glance

```text
nvsos.gov (canonical source)
   │  browser download — the vendor-gov-forms skill
   ▼
notation_templates/forms/<authority>/<form_code>-<revision>.pdf   ← canonical example, committed
notation_templates/forms/FORMS.toml                               ← provenance ledger (sha256, revision, source_url)
notation_templates/forms/<authority>/<form_code>.fields.toml      ← field map, derived from a dump of the bytes
   │  include_bytes! / include_str!
   ▼
forms crate (registry + fieldmap resolution)
   │  template frontmatter:  form: <form_code>
   ▼
web::retainer_walk::render_and_park                      ← answers → field map → Acroform payload
   │  DocumentPayload::Acroform { blank_form_key, fields, storage_key }
   ▼
workflows::dispatch_document_open → pdf::fill_acroform   ← fills the official packet
   │  staff_review: the attorney reviews the FILLED packet
   ▼
filing__nv_sos                                           ← staff files (SilverFlume), records the
                                                           confirmation as a durable `filings` row
```

## Vendoring: canonical source, on disk, no guessing

Every form's exact bytes are committed under `notation_templates/forms/` and pinned in
[`notation_templates/forms/FORMS.toml`](../notation_templates/forms/FORMS.toml) by authority, printed revision
date, canonical source URL, retrieval date, and SHA-256. The acquisition discipline — issuing authority's own
domain only, never a mirror or the
Wayback Machine; one commit per acquisition or refresh so `git log` is the verifiable timestamp ledger — lives in the
`vendor-gov-forms` skill. Field maps and templates are authored only from a dump of the on-disk bytes: real government
field names include `undefined`, `City_5`, and `Name of Registered Agenl` (a typo printed in the official form), so
nothing is guessed.

Guards: `forms/tests/vendored_forms.rs` recomputes every sha256 from the bundled bytes and the working-tree file;
`forms/tests/fill_real_packets.rs` fills the real packets end-to-end in CI.

## Field maps: answers → the form's own fields

Each form carries a `<form_code>.fields.toml` mapping its AcroForm `/T` names to answer sources: a `question` code, a
`literal`, a checkbox `checked_when`/`on_state` pair, a `value_map` (choice answer → printed title), or `row` + `part`
into a `people_list` answer (a JSON array of person rows — name, title, mailing address — captured by one reusable
question widget). A `present_in`/`row_present` gate keeps slot labels like "Trustee" from printing beside an empty name
line. `forms::resolve` turns (map, answers) into the `fill_acroform` input; missing answers skip their fields,
structural defects error loudly.

Deliberately unmapped, in every form: payment-card fields (payment data never flows through the questionnaire), the
Registered Agent Acceptance page (a staff-side artifact), and any field whose widgets are shared across sub-forms — the
LLC packet's Initial-List slot-1 `City`/`State` widgets also render on the ePayment Checklist, so filling them would
print an officer's city onto the payment form.

Guards: `forms/tests/fill_real_packets.rs` asserts every mapped name exists in the vendored bytes and round-trips sample
answers; `forms/tests/template_bindings.rs` asserts every template `form:` binding resolves.

## Filling: the template declares, the worker fills

A template that fills a government form declares it in frontmatter — `form: nv_sos__llc_formation` — persisted to
`templates.form_code` by the seed loader. At staff approve, `render_and_park` resolves the field map against the
respondent's answers, ensures the blank bytes exist in documents storage (an idempotent put of the bundled bytes,
identical in `FsStorage`, KIND, and prod), and hands the worker a `DocumentPayload::Acroform`. The worker fills via
`pdf::fill_acroform`, which handles text fields, checkboxes and radio groups with arbitrary on-states, and duplicate kid
widgets — and refuses loudly (`UnmatchedField`, `InvalidChoice`, `XfaUnsupported`) rather than ever producing a silently
blank or mis-checked form. Templates without a binding render through Typst exactly as before.

The attorney reviews the **filled packet** at `staff_review` before anything is signed or filed — the parked PDF is the
artifact that gets signed and filed, byte for byte.

## Filing: staff-gated, durable

`filing__nv_sos` is a staff act: the attorney files on SilverFlume (nvsos.gov has no public filing API) and the workflow
records a durable `filings` row with the office and confirmation reference, gated by `workflows::guardrail` so no
submission state is reachable without `staff_review`. The journeys in `features/tests/features/nest_formation.feature`
and `entity_formation.feature` walk all three formations from intake to a recorded filing.

## Serving: blanks for logged-in readers

`GET /portal/forms` lists the vendored catalog and `GET /portal/forms/<form_code>.pdf` serves the canonical blank to any
authenticated person (OPA's `/portal/forms` rule). `navigator forms sync` pushes the same bytes to the public assets
bucket (`NAVIGATOR_ASSETS_BUCKET`) at each ledger `object_path` for serving outside the binary. Filled packets are
client documents and persist only to the private documents bucket.

## Forming from the CLI

The whole pipeline is drivable from the `navigator` CLI — a person can form a Nevada entity without opening a browser.
The CLI stays a thin client: it POSTs to the same `/portal` routes the web walker uses and reads question metadata from
their machine-readable branches, so the `staff_review` gate, role check, and `authored_by` provenance all hold.

```bash
navigator login http://localhost:8080
navigator matter open --template onboarding__nest --client-email libra@example.com   # → notation id
navigator intake answer <notation-id>             # walk the questionnaire (interactive, or --answer/--person)
navigator notation status <notation-id>           # workflow state + document_ready
navigator notation approve <notation-id>          # render + park the filled packet (idempotent once rendered)
navigator notation document <notation-id> --out /tmp/llc.pdf   # download the FILLED official SoS packet
```

`matter open` opens the questionnaire-driven matter through `POST /portal/admin/retainers/new` (the sibling command
`project open`, by contrast, also sends a retainer). `intake answer` walks each question over the
`/portal/admin/notations/:id/step` route, reading the current question's prompt, `answer_type`, and `radio` choices from
that route's `?format=json` branch (the choices come from the canonical `Question.yaml` via
`store::seed::question_choices`, since they have no column on the `questions` table), and posting a `people_list` answer
as the widget's `p{row}_{part}` fields. A clean staff-entered walk auto-renders the packet on the last answer, so
`notation approve` is an idempotent confirmation; `notation document` downloads the same per-notation
`notations/<id>/document.pdf` the review surface shows, via the participation-gated `…/documents/document` route.
`cli/tests/llc_formation_e2e.rs` proves the binary round-trip against an in-process app and asserts the downloaded bytes
carry the answers (`NAME OF ENTITY`, `managers_b`, `Name3`) — the same assertions `features/tests/nest_formation.rs`
makes, now through the CLI surface. See [`cli/README.md`](../cli/README.md) for the full command reference.

## Adding the next form

1. Vendor it with the `vendor-gov-forms` skill (canonical source, ledger entry, own commit).
2. Dump its fields from the on-disk bytes; author `<form_code>.fields.toml` (and new question seeds only if no
   existing code fits — reuse first).
3. Add the `include_bytes!`/`include_str!` rows to the `forms` crate; the guard tests pick the form up automatically.
4. Write the feature spec, then the template with `form:` + questionnaire + workflow, mirror the spec YAML, and add
   it to the bundled catalogs (`workflows::specs`, `store::seed`).
5. If the form is flat (no AcroForm), stop: the overlay fill path is designed but deliberately unbuilt until the
   first flat form arrives.
