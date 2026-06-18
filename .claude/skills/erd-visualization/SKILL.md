---
name: erd-visualization
description: >
  Render the workspace ERD as SVG (the canonical `docs/erd.svg`) using the navigator CLI's deterministic, pure-Rust
  renderer. Trigger when prepping a slide, screenshot, design-doc, or external review — or when someone asks "can you
  show me the database structure?" Also trigger when a migration has just landed: regenerate both `docs/erd.md`
  (mermaid) and `docs/erd.svg` (rendered) so picture and schema stay in sync; the `cli::tests::erd_svg` integration test
  will otherwise fail.
---

# ERD visualization recipe

The workspace ships two ERD artifacts and both come from the same introspection in `cli/src/erd.rs`:

- [`docs/erd.md`](../../../docs/erd.md) — a Mermaid `erDiagram` block. GitHub renders Mermaid natively in markdown, so
  this covers the "view in the repo" case. Also contains the canonical `psql` + `awk` pipeline for regenerating from any
  Postgres.
- [`docs/erd.svg`](../../../docs/erd.svg) — a standalone SVG rendered by `cli erd --format svg`. The renderer is
  **deterministic by construction**: alphabetical BTreeMap iteration, integer-only arithmetic, no timestamps, no random
  IDs. Same schema in → byte-identical SVG out.

Use the SVG for anything that doesn't render Mermaid natively (slides, design docs, screenshots, links shared outside
the repo).

## The recipe — one command

```bash
set -a && source .devx/env && set +a   # DATABASE_URL for the KIND Postgres
cargo run -p cli -- erd --format svg > docs/erd.svg
```

The CLI introspects `pg_catalog`, lays out tables in a deterministic grid, and emits SVG XML directly. The output is
byte-stable across runs against the same schema — checked into the repo, guarded by a test ([see
below](#idempotency-test)).

### `--format` choices

- `--format mermaid` (default) — emit the Mermaid `erDiagram` block on stdout. Equivalent to what the SQL+awk pipeline
  in `docs/erd.md` produces.
- `--format svg` — emit a standalone SVG document on stdout.

Both formats introspect the same schema; only the rendering differs.

## Refresh after a migration

When a schema-changing migration lands, both files need a refresh:

```bash
set -a && source .devx/env && set +a

# 1. Mermaid block in docs/erd.md (also produces the committed file)
cargo run -p cli -- erd --format mermaid   # paste into docs/erd.md

# 2. SVG
cargo run -p cli -- erd --format svg > docs/erd.svg
```

Verify against production Cloud SQL too — the deploy cron rolls migrations on Mon-Thu 06:00 UTC, so local and prod
should match within a day of any change:

```bash
gcloud auth application-default login   # one-time per session
cloud-sql-proxy --auto-iam-authn --port 15433 \
    YOUR_PROJECT_ID:us-west4:navigator-pg &

DATABASE_URL="postgres://${USER}@your-domain.example@127.0.0.1:15433/navigator?sslmode=disable" \
  cargo run -p cli -- erd --format mermaid > /tmp/prod_mermaid.txt

diff <(cargo run -p cli -- erd --format mermaid) /tmp/prod_mermaid.txt
# (no output = identical)
kill %1
```

See [[postgres-in-kind]] for the local connection story and [[cloud-rest-endpoints]] for `cloud-sql-proxy
--auto-iam-authn`.

## Opening the SVG

```bash
firefox docs/erd.svg     # or google-chrome
```

Or set Firefox as the default SVG handler once:

```bash
xdg-mime default firefox.desktop image/svg+xml
xdg-open docs/erd.svg
```

The Rust-rendered SVG uses native SVG `<text>` elements (not `<foreignObject>`), so it also opens correctly in GNOME
Image Viewer, `feh`, and any other simple SVG viewer — unlike Mermaid's SVG output, which puts text in `<foreignObject>`
and is invisible in many image viewers.

## Idempotency test

[`cli/tests/erd_svg.rs`](../../../cli/tests/erd_svg.rs) spins up a Postgres testcontainer via
`store::test_support::schema()`, runs migrations, introspects via `cli erd --format svg`, and asserts the output
byte-matches `docs/erd.svg`. The test runs as part of `cargo test -p cli` and `cargo test --workspace`.

When it fails the message tells you exactly what to do:

```text
docs/erd.svg drifted from a freshly rendered schema.
rendered: 29448 bytes
committed: 29447 bytes
line 142: rendered="    <text class=\"cn\" x=\"12\" y=\"...
         committed="    <text class=\"cn\" x=\"12\" y=\"...

To refresh:
  set -a && source .devx/env && set +a
  cargo run -p cli -- erd --format svg > docs/erd.svg
```

If a migration changes the schema and you forget to refresh `docs/erd.svg`, CI catches it. The intended workflow is:
write the migration → run `cargo test -p cli` → see the SVG drift → regenerate → commit both the migration and the
refreshed SVG together.

Mermaid-as-source is still useful — GitHub renders it natively in `docs/erd.md`. The rendered SVG is ours so it can be
byte-stable.

## Layout tuning

The current layout is intentionally simple — 4-column alphabetical row-major grid, straight-line edges, no crossing
avoidance. The priority is *byte-stable, readable enough*. If you want to improve it, the layout constants are at the
top of `cli/src/erd.rs`:

- `CHAR_WIDTH`, `ROW_HEIGHT`, `TITLE_HEIGHT` — text dimensions
- `CELL_PAD`, `CELL_GAP_X`, `CELL_GAP_Y` — spacing
- `GRID_COLS` — how many columns wide the table grid is
- `MARGIN`, `FONT_SIZE` — outer margin and base font size

Any change to layout will trip the idempotency test until `docs/erd.svg` is refreshed; that's a feature, not a bug.

## Related lenses on the schema

Sometimes you don't need a picture:

- `psql \dt+` — table list with sizes.
- `psql \d <table>` — column list with types, defaults, FKs,
  indexes.
- `cargo run -p cli -- erd --format mermaid` — plain text you can
  grep.
