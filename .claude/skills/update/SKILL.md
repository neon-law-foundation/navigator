---
name: update
description: >
  One command — `/update` — for the periodic dependency refresh: bump every Rust crate to its latest compatible version
  AND refresh the vendored front-end assets (Bootstrap, HTMX, Alpine, Bootstrap Icons, the Noto Serif webfonts) that
  `web` serves from `web/public/`. These assets are downloaded once from their CDN and served same-origin from axum
  `ServeDir` at `/public` — we NEVER link a CDN at runtime, and two tests enforce it: `web/tests/vendor_assets.rs`
  (pinned hashes) and `web/tests/no_cdn_assets.rs` (nothing served reaches off origin). Trigger when the user says
  "/update", "update everything", "refresh all dependencies", "update the crates and web assets", or runs the periodic
  bump. This skill ORCHESTRATES the two single-purpose skills [[update-crates]] and [[update-web-assets]] — it does not
  replace them, and it keeps their work in SEPARATE commits (different blast radius). Majors stay gated behind /council.
---

# `/update` — refresh crates and vendored web assets

The periodic dependency sweep, in one entry point. It runs two independent refreshes back to back and **commits each
separately**, because a Rust lockfile bump and a minified-asset swap have entirely different blast radii and
verification needs. Nothing here links a CDN: every front-end byte is vendored under `web/public/` and served from
axum's `/public` mount — "load assets from axum, never a CDN" is the whole point, and it is test-enforced (see below).

This skill is the conductor. The mechanics live in the two skills it drives:

- **[[update-crates]]** — `cargo update` (latest semver-compatible) for the whole workspace.
- **[[update-web-assets]]** — re-vendor Bootstrap / HTMX / Alpine / Bootstrap Icons (+ Noto Serif) at the latest
  minor/patch, recording new hashes in `web/public/VENDOR.toml`.

Run them in this order; do the crate bump first so the asset browser-smoke runs against an already-updated workspace.

## The no-CDN invariant (why these assets are vendored)

The lawyer's and the applicant's browser must never fetch code or styling from an unpinned third party. So every
third-party CSS/JS/font is **downloaded once** (from a CDN URL, recorded as `upstream_url` in `VENDOR.toml`) and then
**served same-origin** from `web/public/` via `ServeDir`. The `<link>`/`<script>` tags in `views/src/layout.rs`
reference `/public/...` paths only — never an `https://cdn...` URL. Two tests make this durable:

- `web/tests/vendor_assets.rs` — recomputes each `served_path`'s SHA-256 and fails on drift from `VENDOR.toml`.
- `web/tests/no_cdn_assets.rs` — fails if any served `.html` loads a subresource off origin, any `.css`
  `@import`s/`url()`s off origin, or any first-party script names a CDN host. The runtime backstop is the CSP
  (`script-src 'self'` in `web::csp_value`); this test catches the offending byte at build time.

If a refresh ever tempts you to point a tag at a CDN "just for now," don't — vendor it and these tests stay green.

## Phase 1 — crates (lockfile)

Follow [[update-crates]]. The routine form is the semver-compatible lockfile refresh:

```bash
cargo update                 # whole workspace, semver-compatible — the default
cargo update --dry-run       # preview what would move
```

- "Latest version" here means **latest compatible**. Crossing a **major** (which `cargo upgrade --incompatible` does)
  can break APIs and is an architecture call — surface it, take it through `/council`, and do NOT fold it into this
  sweep. Respect the existing pin comments in `Cargo.toml` (e.g. `maud`/`tower-cookies` held for axum 0.7).
- The toolchain pin (`rust-toolchain.toml` → 1.96.0) does **not** move as part of `/update`.

Gate, then commit on its own (`chore(deps): cargo update (lockfile)`):

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Phase 2 — vendored web assets

Follow [[update-web-assets]] per asset: pick the latest **minor/patch**, download from the pinned `upstream_url` (swap
the version), re-apply any local modification (the Bootstrap Icons `@font-face` path rewrite), and record the new
`version` + `sha256` in `web/public/VENDOR.toml`.

- **Minor/patch only.** A major (Bootstrap 5→6, Alpine 3→4, HTMX 2→3, Icons 1→2) restyles or re-behaves every page —
  stop and run `/council` first.
- Network egress (the `curl`s) happens on the user's machine — propose the commands; they run them with `!`.

Verify the asset half, then commit on its own (`chore(web): bump <lib> <old> -> <new> (vendored asset)`):

```bash
cargo test -p web --test vendor_assets     # pinned-hash guard (fast)
cargo test -p web --test no_cdn_assets      # nothing serves off origin
cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Asset bumps also need a **browser smoke** (minified CSS/JS can break rendering unit tests never exercise). Per
[[kind-local-dev]], run it on the user's machine:

```bash
cargo run -p cli -- start-dev-server
source .devx/env && cargo run -p web
cargo test -p web --test browser_e2e
```

Eyeball: `/` renders (Bootstrap intact), an admin HTMX delete still swaps, an Alpine modal/toggle still opens.

## Commit discipline — two commits, never one

`/update` produces **two** commits, never a combined one:

1. `chore(deps): cargo update (lockfile)` — touches `Cargo.lock` (and `Cargo.toml` only if a requirement was rewritten).
2. `chore(web): bump <lib> <old> -> <new> (vendored asset)` — touches the blob(s) under `web/public/`, `VENDOR.toml`,
   and `views/src/layout.rs` only if a tag changed.

**Branch → PR → auto-merge — never commit on `main`.** Per [`CLAUDE.md`](../../../CLAUDE.md) Commit discipline, make
both commits on one topic branch (e.g. `git switch -c periodic-dependency-update`), push and open a PR
(`git push -u origin <topic>` → `gh pr create`), then enable auto-merge (`gh pr merge --auto --squash`). `ci.yml` runs
on the PR and GitHub squash-merges it once every required check is green — never commit to `main`, never merge by hand.

## Cadence

Monthly is the natural rhythm: `cargo update` is cheap and reversible, and a quarterly-or-on-advisory asset refresh
rides along. Either phase can also run alone via its own skill — `/update` is just the "do both now" entry point. Run
either immediately on a published security advisory for a crate or one of the vendored libraries.
