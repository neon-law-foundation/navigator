---
name: update-web-assets
description: >
  Refresh the vendored front-end assets that `web` serves from `web/public/` — Bootstrap (CSS + JS bundle), HTMX,
  Alpine.js, and Bootstrap Icons (CSS + webfont). These are vendored, NOT loaded from a CDN: they are served via axum
  `ServeDir` at `/public`. The single source of truth is `web/public/VENDOR.toml` (version + upstream_url + sha256 per
  asset), guarded by `web/tests/vendor_assets.rs`. Trigger when the user says "update the web assets", "bump
  Bootstrap/HTMX/Alpine", "refresh the vendored JS/CSS", or as the asset half of a periodic dependency refresh. This is
  SEPARATE from `update-crates` (Rust deps) — different tools, different blast radius, different verification. Cap at
  minor/patch bumps; a MAJOR bump (Bootstrap 5 -> 6, Alpine 3 -> 4, HTMX 2 -> 3) restyles or re-behaves every page and
  MUST go through `/council` before any bytes change. To refresh crates AND assets in one periodic sweep (still two
  commits), use the `update` skill (`/update`).
---

# Updating vendored web assets

`web` serves four third-party front-end libraries from `web/public/`, vendored (no CDN) so the same bytes run in dev,
KIND, and prod and nothing in the lawyer's / applicant's browser is fetched from an unpinned third party.

| Library | served_path | referenced in |
| --- | --- | --- |
| Bootstrap CSS | `css/bootstrap.min.css` | `views/src/layout.rs` |
| Bootstrap JS | `js/bootstrap.bundle.min.js` | `views/src/layout.rs` |
| HTMX | `js/htmx.min.js` | `views/src/layout.rs` |
| Alpine.js | `js/alpine.min.js` | `views/src/layout.rs` |
| Bootstrap Icons | `icons/bootstrap-icons.css` + `icons/fonts/bootstrap-icons.woff2` | `views/src/layout.rs` |

The `<link>` / `<script>` tags live in `views/src/layout.rs` (around lines 107–123) and reference assets by path only —
**no version in the tag**. The version lives in **`web/public/VENDOR.toml`**, which is the single source of truth.
`web/tests/vendor_assets.rs` recomputes the SHA-256 of every `served_path` and fails if it doesn't match the manifest,
so the manifest can never silently drift from disk.

## Scope rule — minor/patch only

This skill bumps **minor and patch** versions. A **major** bump (Bootstrap 5→6, Alpine 3→4, HTMX 2→3, Bootstrap Icons
1→2) changes class names, component markup, or JS behavior and will restyle or break pages that `cargo test` never
renders. **Stop and run `/council` first** for any major — the council gates it behind a design review, then this skill
executes the agreed version.

## Recipe (per asset, pin-and-verify — never "latest")

1. **Pick the target version.** Check the latest minor/patch for the library (its release page / npm). Do not jump a
   major (see scope rule).

2. **Download from the pinned URL.** Take the `upstream_url` from `web/public/VENDOR.toml`, swap the version, and fetch
   it. Propose these as commands for the user to run (network egress happens on their machine). For example, to take
   HTMX from 2.0.4 to 2.0.5:

   ```bash
   curl -fsSL https://unpkg.com/htmx.org@2.0.5/dist/htmx.min.js -o web/public/js/htmx.min.js
   ```

   - Bootstrap: `https://cdn.jsdelivr.net/npm/bootstrap@<v>/dist/{css/bootstrap.min.css,js/bootstrap.bundle.min.js}`
   - HTMX: `https://unpkg.com/htmx.org@<v>/dist/htmx.min.js`
   - Alpine: `https://unpkg.com/alpinejs@<v>/dist/cdn.min.js` → `js/alpine.min.js`
   - Bootstrap Icons CSS: `https://cdn.jsdelivr.net/npm/bootstrap-icons@<v>/font/bootstrap-icons.css`
   - Bootstrap Icons font: `https://cdn.jsdelivr.net/npm/bootstrap-icons@<v>/font/fonts/bootstrap-icons.woff2`

3. **Re-apply local modifications.** `icons/bootstrap-icons.css` is marked `modified = true` in the manifest: its
   `@font-face` `src` is rewritten to our local `./fonts/bootstrap-icons.woff2`. After downloading the upstream CSS,
   re-apply that rewrite (point `src` at the local font, drop the woff fallback if absent) before recording its hash.

4. **Recompute and record the hash** in `web/public/VENDOR.toml` — bump `version` and replace `sha256` with the new
   on-disk digest. Update the file's leading provenance comment too, if it carries the version:

   ```bash
   sha256sum web/public/js/htmx.min.js
   ```

5. **Verify the manifest matches disk.** This is the fast gate — it fails immediately if any `sha256` / `version` /
   bytes triple is inconsistent:

   ```bash
   cargo test -p web --test vendor_assets
   ```

## Verification — the full gate

Asset refreshes need more than `cargo test`: minified JS/CSS changes can break rendering and in-page behavior that unit
tests never exercise.

1. Manifest guard (above): `cargo test -p web --test vendor_assets`.
2. Standard workspace gate:

   ```bash
   cargo fmt
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

3. **Browser smoke test — required for asset bumps.** Bootstrap/Alpine/HTMX power layout, modals/toggles (Alpine,
   admin), and in-page swaps (HTMX, admin delete). Per `kind-local-dev`, the browser e2e runs on the user's machine —
   propose the commands; they run them with `!`:

   ```bash
   cargo run -p cli -- start-dev-server                # bring deps up in KIND
   source .devx/env && cargo run -p web   # run web against in-cluster deps
   cargo test -p web --test browser_e2e   # then drive the browser e2e
   ```

   Eyeball at minimum: `/` renders (Bootstrap CSS intact), an admin page's HTMX delete still swaps, and an Alpine
   modal/toggle still opens.

## Commit discipline

- **Branch → PR → auto-merge — never commit on `main`.** Per [`CLAUDE.md`](../../../CLAUDE.md) Commit discipline, do the
  refresh on a topic branch (`git switch -c <topic>`), push and open a PR
  (`git push -u origin <topic>` → `gh pr create`), then enable auto-merge (`gh pr merge --auto --squash`). `ci.yml` runs
  on the PR and GitHub squash-merges it
  once every required check is green — never commit to `main`, never merge by hand.
- One commit per refresh round; **never** in the same commit as a `cargo update` (different blast radius — see
  `update-crates`).
- The commit touches: the vendored file(s) under `web/public/`, `web/public/VENDOR.toml`, and (only if a tag changed)
  `views/src/layout.rs`.
- Suggested message: `chore(web): bump <lib> <old> -> <new> (vendored asset)`.

## Cadence

Quarterly, or immediately on a published security advisory for one of the four libraries. Crates move on their own
schedule (`update-crates`, monthly) — keep the two flows in separate commits.
