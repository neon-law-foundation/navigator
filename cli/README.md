# cli

Operator CLI for Navigator (binary name: `navigator`; the crate is still `cli`, so `cargo run -p cli -- …` works
unchanged). Validates markdown templates against the rule engine, imports clean files into the same SeaORM-managed
Postgres `web` reads from, seeds canonical reference data, prints rows, renders an ER diagram for the schema, and — over
a browser-loopback login — drives a **live site's** matter flow against a short-lived bearer token.

## Getting started

```bash
# DB-free subcommand: works on any laptop, no Postgres required.
cargo run -p cli -- validate templates

# DB-touching subcommands take --database-url, falling back to
# the DATABASE_URL environment variable.
export DATABASE_URL=postgres://navigator:navigator@localhost:15432/navigator
cargo run -p cli -- import templates
cargo run -p cli -- list templates
cargo run -p cli -- erd | head

# Or install on your PATH
cargo install --path cli
navigator --help
```

Subcommands split by whether they need a database:

| Subcommand       | Needs DB? | Notes                                                                      |
| ---------------- | --------- | -------------------------------------------------------------------------- |
| `validate`       | no        | N104 runs in structural mode only.                                         |
| `render`         | no        | Validation-gated template → PDF; `--format letter`.                        |
| `format`         | no        | Whitespace + bullet cleanup on one `.md`.                                  |
| `glossary`       | no        | Looks up workspace vocabulary by term.                                     |
| `scaffold`       | no        | Drops template + workflow + feature stubs.                                 |
| `assets build`   | no        | Transcodes source photos into AVIF/WebP/JPEG.                              |
| `assets upload`  | no        | Pushes built variants to the public assets bucket.                         |
| `assets pull`    | no        | Restores `web/public/img/` from the assets bucket for local dev.           |
| `import`         | **yes**   | Writes into `--database-url` Postgres.                                     |
| `list`           | **yes**   | Auto-runs migrate + seed before printing.                                  |
| `erd`            | **yes**   | Introspects `pg_catalog` + `information_schema`.                           |
| `project create` | **yes**   | Needs `--client-email` (a client DRI); `--skip-migrate-and-seed` for prod. |

The live-site commands need no local database — they are an authenticated HTTP client against a deployed `web`:

| Subcommand | Route hit | Notes |
| --- | --- | --- |
| `login` | `GET /auth/cli/start` | Browser-loopback OAuth → `~/.navigator.json` (`0600`). |
| `logout` / `whoami` | (local) | Forget / inspect the stored token; `whoami` does the expiry math locally. |
| `projects list` | `GET /portal/projects.csv` | Rendered as a table, or `--json`. |
| `project open` | `POST /portal/projects` | Open a matter **and** send a retainer in one action; parks at review. |
| `matter open` | `POST /portal/admin/retainers/new` | Open a questionnaire-driven matter; parks at question one. |
| `intake answer` | `GET`/`POST …/step` | Walk the questionnaire (interactive or `--answer`/`--person`). |
| `retainer clause` | `…/clauses` | `add` / `edit` / `list` the per-matter clauses spliced into the retainer. |
| `retainer approve` | `POST …/approve-send` | Renders + parks the PDF at `document_open__retainer_pdf`; no envelope. |
| `retainer send` | `POST …/send` | One real envelope on prod; deliberate human command. `409` until rendered. |
| `notation status` | `GET …/review?format=json` | Workflow state, signature request id, `document_ready`. |
| `notation approve` | `POST …/approve-send` | Render + park the bound packet (formation form or retainer). |
| `notation document` | `GET …/documents/document` | Download the rendered (filled) packet to `--out <path>`. |

## Driving a live site

`navigator login` mints a short-lived (~8h) bearer token the same way `gcloud auth login` does — it opens the browser,
reuses the site's existing OIDC session, and lands the token on a `127.0.0.1` loopback listener. The token is the same
HMAC-signed session blob the browser cookie carries, presented as `Authorization: Bearer`; the server resolves it back
into the caller's session, so every command runs the same handler — and the same `staff_review` gate, role check, and
`authored_by` provenance — the browser does. Sending a retainer for signature stays a deliberate authenticated human
command (`retainer send`); it is never exposed as an LLM-routable tool.

The send is a durable two-step. `retainer approve` fires `approved`, the worker durably renders + persists the retainer
PDF, and the workflow **parks** at `document_open__retainer_pdf` — no envelope yet. `retainer send` then confirms the
PDF is present (`notation status` shows `document_ready:true`) and dispatches exactly one envelope. Splitting the two is
what makes the pipeline safe against a real worker whose render is a separate durable invocation: `send` returns `409`
with a JSON reason — not an opaque 500 — when the PDF isn't ready yet, so the operator retries rather than racing.

```bash
navigator login --host www.neonlaw.com           # browser → ~8h token, stored 0600 at ~/.navigator.json
navigator whoami                                  # "nick@neonlaw.com (admin) — expires in 7h52m"
navigator projects list                           # table (or --json)
navigator project open --name "Shook estate" \
  --template onboarding__retainer \
  --client-name "Nick Shook" --client-email nick@shook.family \
  --scope "Flat-fee estate planning"              # prints the notation id + review URL
navigator retainer approve <notation-id>          # renders + parks the PDF (no envelope)
navigator notation status <notation-id>           # state + signature request id + document_ready
navigator retainer send <notation-id>             # dispatches one real envelope (409 until document_ready)
navigator logout
```

`--host` is optional after a single `login` (the sole stored host is used); pass it to pick between prod, staging, and a
local `http://localhost:8080` KIND run, each keyed separately in the credential file.

## Forming an LLC from the CLI

A person can form a Nevada LLC end to end without opening a browser. `matter open` starts a questionnaire-driven
`onboarding__*` matter (distinct from `project open`, which opens a matter *and* sends a retainer); `intake answer` then
walks the questionnaire one question at a time over the same `/portal/admin/notations/:id/step` route the browser POSTs.
The CLI reads each question's prompt, `answer_type`, and (for a `radio`) its choices from that route's `?format=json`
branch — it never scrapes HTML — and posts a `people_list` answer as the widget's `p{row}_{part}` fields.

In interactive mode `intake answer` shows one prompt per question — a `radio` lists its choices, and a `people_list` is
entered row by row (a blank name ends the rows).

```bash
navigator login http://localhost:8080
navigator matter open --template onboarding__nest --client-email libra@example.com
navigator intake answer <notation-id>
navigator notation status <notation-id>
navigator notation approve <notation-id>
navigator notation document <notation-id> --out /tmp/llc.pdf
```

To script it (no prompts), answer non-interactively — scalar answers in the order the questionnaire asks, and one
`--person` per `people_list` row:

```bash
navigator intake answer <notation-id> \
  --answer "Libra" --answer "libra@example.com" --answer "Bright Star Ventures" \
  --answer "Neon Law Registered Agent" --answer "members" \
  --person 'name=Libra,street=1 Main St,city=Las Vegas,state=NV,zip=89101,country=USA' \
  --answer "2026-07-01"
```

A clean staff-entered walk auto-renders the packet on the last answer and drives the matter to the signature wait, so
`notation approve` is an idempotent confirmation rather than a separate render step; `notation document` then downloads
the same per-notation PDF the review surface shows. The whole round-trip is proven against an in-process `web` app in
`tests/llc_formation_e2e.rs`.

## Photography assets

`assets build` resizes + re-encodes the curated source photos (manifest: `views::assets::GALLERY`) into responsive AVIF,
WebP, and JPEG width variants under `web/public/img/<slug>/`. `assets upload` then pushes that tree to the public assets
bucket (`--bucket`, default `NAVIGATOR_ASSETS_BUCKET`) through the `cloud` crate's `StorageService`, stamping a bounded
`Cache-Control` (~1 week, never `immutable`). `assets pull` is the inverse — it downloads the published variants from
the bucket back into `web/public/img/` so a fresh clone (or any developer without the source JPEGs) can serve the photos
locally.

```bash
# Curate the gallery (needs the source JPEGs):
cargo run -p cli -- assets build    # /tmp sources → web/public/img
cargo run -p cli -- assets upload   # web/public/img → gs://<project>-assets/img

# Restore photos on a fresh clone (no source JPEGs needed):
cargo run -p cli -- assets pull     # gs://<project>-assets/img → web/public/img
```

> **First-run note.** `web/public/img/` is gitignored — the variants ship from the bucket in production, never from
> git or the Docker image. A fresh clone therefore has **empty photo slots** until you populate them. Run
> `assets pull` to download the already-published variants (no source JPEGs, no re-encode), or `assets build` if you
> have the sources. This is intentional and matches how workshop/marketing assets are handled; everything else under
> `web/public` (Bootstrap, brand SVGs) is tracked and renders immediately. With `NAVIGATOR_ASSET_BASE_URL` unset the
> page markup resolves photos against `/public`, so once the directory is populated the KIND dev loop serves them with
> zero configuration. Full pipeline: [`docs/assets.md`](../docs/assets.md).

## What's next

`cli`'s shipped binary depends on `rules` and `store` — no `web` dep, so it stays small and starts instantly. (`web`,
`workflows`, and `pdf` are **dev-dependencies** only, for the in-process end-to-end test in `tests/llc_formation_e2e.rs`
that drives the binary against a real app on a loopback port; they never link into the shipped binary.) Integration
tests under `tests/` drive the compiled binary end-to-end via `assert_cmd` / `CARGO_BIN_EXE_navigator` against per-test
Postgres schemas spun up via `store::test_support`. To add a subcommand, extend the `Command` enum in `src/main.rs` and
wire it to a module.
