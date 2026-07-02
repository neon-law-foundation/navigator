# Government Forms: Vendor, Map, Fill, File

Neon Law Navigator fills official government PDF forms from questionnaire answers and files them through a staff-gated
workflow. The blank PDF bytes live **only** in the public GCS assets bucket — these are public government documents, so
a public bucket is correct — and the repository keeps the diffable text: the markdown catalog card, the field layer's
text mirror (a `.fields.toml` map, or the `.fields` manifest of a re-authored blank), and a `.sha256` pin of the
canonical blank. The repository path is still the storage contract: the pin at
`templates/forms/united_states/nevada/state/nv__llc_formation.sha256` pins the bucket object at
`forms/united_states/nevada/state/nv__llc_formation.pdf`.

## Pipeline

```text
government website (`origin_url`)
   │  human downloads / re-authors the blank
   ▼
templates/forms/<country>/<jurisdiction>/<office>/<code>.pdf   (untracked working copy)
   │
   │  navigator forms sync — uploads, writes the sibling .sha256 pin
   ▼
public assets bucket: forms/<country>/<jurisdiction>/<office>/<code>.pdf
   │
   │  repo keeps: <code>.md  <code>.fields.toml | <code>.fields  <code>.sha256
   ▼
forms crate registry (metadata + pin) + field-map resolution
   │
   ▼
web::retainer_walk::acroform_payload
   │  StorageService::get(object_path) → sha256(bytes) == pin, or fail loudly
   ▼
staff_review → workflows::dispatch_document_open → pdf::fill_acroform → pdf::flatten
   │
   ▼
signature / filing
```

The fill path **always pulls**: there are no baked-in bytes and no fallback. A blank missing from the bucket, or one
whose bytes fail the pin, parks the matter with a loud error — `web` never fills against bytes it hasn't pinned. The
verified bytes are staged into the private documents lane at the same key for the worker's `dispatch_document_open`
fill, so what the worker fills is byte-identical to what was verified.

The `.md` file is the catalog card and workflow. It declares the form identity:

```yaml
code: nv__llc_formation
jurisdiction: NV
origin_url: https://www.nvsos.gov/businesses/commercial-recordings/forms-fees/all-business-forms
form: nv__llc_formation
```

`code`, the filename stem, and the `form:` binding match. `origin_url` points at the government page where the blank can
be obtained. The bucket holds the vendored bytes; the `.sha256` pin records exactly which bytes; the URL records
provenance.

## Vendoring — `navigator forms sync` and `navigator forms fields`

`navigator forms sync` closes the loop in both directions, per registry form:

- **With a working copy** at `templates/<object_path>` (untracked — `.gitignore` keeps every `templates/forms/**/*.pdf`
  out of the tree): upload it to the bucket and rewrite the sibling `.sha256` pin to match. Commit the pin (and any map
  change) in the same PR; rebuild so the registry bakes the new pin in.
- **Without one**: pull the bucket object and verify it against the pin. A missing object or a mismatch exits non-zero —
  the same bytes the fill path would refuse.

`navigator forms fields <code>` pulls the blank, verifies its pin, and prints the AcroForm `/T` field names one per line
— the ground truth for authoring a `.fields.toml` or re-authoring the field layer (`/T` name = question code, the
sequenced follow-on below). No guessing: these are the names on the exact bytes the workflows fill.

`navigator forms re-author <code>` (#256 item 1) retires a form's `.fields.toml`: it pulls the blank, verifies its pin,
and transforms the field layer so the `/T` names *are* questionnaire state paths — the map's recorded judgment drives
every rename (several hostile names collapsing onto one state merge into one multi-widget field), every checkbox-pair →
radio merge (`custom_single_choice__management_structure`, choice values as on-states), and every pre-printed literal
(`NRS 86` becomes static content); every field the map never covered lands in the reserved `unmapped__` namespace, so
"unmapped" is a decision the guard checks, not a comment. The transform is deterministic and refuses loudly on anything
it cannot cleanly express. It writes the working copy plus the sorted `.fields` manifest; visual QA of a sample fill,
`navigator forms sync`, and deleting the consumed `.fields.toml` remain the human steps.

All three subcommands target `NAVIGATOR_ASSETS_BUCKET` (or `--bucket`) and honor the `NAVIGATOR_STORAGE_ENDPOINT`
emulator override, so the same commands vendor into KIND's fake-gcs `navigator` bucket. Before its first filing, a fresh
environment downloads each blank from its `origin_url` to the working-copy path and runs the sync. An offline mode (a
warmed local cache) is deliberately not built.

## Field Maps and Manifests

A fillable form carries exactly one sibling text mirror of its field layer. A **re-authored** form (today:
`nv__llc_formation`) carries a `.fields` manifest — its blank's actual `/T` names, one per line — and fills with no map
at all: `forms::resolve_reauthored` reads each name as its own data path (`entity__company.name`,
`people__managing_members.0.title`, a bare `custom_single_choice__*` radio state), skipping the `unmapped__` namespace.
A **mapped** form carries a `<code>.fields.toml` mapping exact AcroForm `/T` names to answer sources. Real government
field names can be hostile (`undefined`, `City_5`, misspellings baked into the official file), so maps are data and
tests pin them. `forms::resolve` converts questionnaire answers into the field map consumed by `pdf::fill_acroform`.

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
- **`every_reauthored_field_name_is_a_declared_state_path_or_unmapped`** — the same assertion moved onto the names
  of the bytes we file: every `.fields` manifest entry either carries a declared questionnaire state or sits in the
  `unmapped__` namespace.

A guessed map, a renamed question, or a notation that drifted from its map fails CI here — before a mis-mapped field can
mis-fill a filing.

## The Guard / Verify Split

CI stays offline; the network truth is checked at vendor time:

- **`cargo test` (offline)** — the question-code contract above, the pin-shape guard (`forms/tests/vendored_forms.rs`),
  and the fill round-trip (`forms/tests/fill_round_trip.rs`), which stages a synthetic blank built from each form's own
  field-layer mirror (`.fields.toml` rules, or `.fields` manifest names plus the notation's `choices:` for radio groups)
  in a `fake-gcs-server` container and runs the full production pipeline against the `cloud::StorageService` seam: pull
  → verify pin → resolve → fill → flatten. The web, CLI, and journey e2e suites stage the same synthetic blanks
  (`web::test_support::stage_blank_forms`) under their own pins, so the formation flows exercise the pull-and-verify
  gate end to end.
- **`navigator forms sync` (network)** — asserts the bucket's actual bytes match the repo pins; `navigator forms fields`
  reads the field names off those exact bytes. A re-vendor that changes the bytes without updating the pin fails here,
  and the fill path refuses the same bytes in production.

## Sequenced Follow-Ons

The end state is that every fillable blank's AcroForm `/T` names **are** the question-code paths. `nv__llc_formation` is
there — re-authored by `navigator forms re-author`, its `.fields.toml` retired for the `.fields` manifest.
`nv__profit_corp_formation` and `nv__business_trust_formation` keep their `.fields.toml` on the path above until each
takes the same pass (the profit-corp map still carries `value_map` slot-label rules the planner refuses by design;
re-express them as the person row's own `title` part first, as the LLC map did). Re-authoring happens on the working
copy of each blank; `navigator forms sync` then vendors it up and records the new pin.

The filled packet is **flattened** before it is persisted. Because a form's fill state (`document_open__*_pdf`) sits
past `staff_review` in every packet's workflow spec, `dispatch_document_open` runs `pdf::flatten` right after
`pdf::fill_acroform`: it paints every value onto the page (text as page content, a checked box as its own appearance
stream), drops the widget annotations (dereferencing the indirect `/Annots` arrays the NV packets use), and empties the
AcroForm `/Fields`. The result freezes exactly what an attorney approved — no downstream viewer can re-edit a value on
the way to a government office, and a viewer that ignores `/NeedAppearances` shows the filled values rather than a blank
form. Overlay text is written in `WinAnsiEncoding` (declared on the overlay font), so accented names render correctly
everywhere; a character outside that encoding fails the flatten loudly instead of filing a garbled glyph.

## Runtime Storage

Blank forms are public assets. `navigator forms sync` vendors each registry entry to its `object_path` in the public
assets bucket:

```text
forms/united_states/nevada/state/nv__llc_formation.pdf
```

At fill time `web` pulls the blank from that bucket (`cloud::assets_from_env` — `NAVIGATOR_ASSETS_BUCKET`, falling back
to `NAVIGATOR_STORAGE_BUCKET` in the single-bucket KIND/dev topology), verifies the pin, and stages the verified bytes
at the same key in the private documents lane for the worker.

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

1. Download the blank from the government `origin_url` to the bucket-shaped repo path under `templates/forms/` (it
   stays untracked).
2. Run `navigator forms sync` — it uploads the blank and writes the sibling `.sha256` pin (tracked).
3. Add a sibling markdown template with matching `code`, `jurisdiction`, `origin_url`, and `form`.
4. Add a sibling field map if the PDF is fillable — `navigator forms fields <code>` prints the real `/T` names.
5. Add the form's metadata (with its `include_str!` pin) and field map to the `forms` crate registry.
6. Run `cargo test -p forms` and `cargo run -p cli -- validate templates`.
