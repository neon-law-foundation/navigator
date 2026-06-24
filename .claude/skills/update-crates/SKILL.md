---
name: update-crates
description: >
  Update the workspace's Rust crate dependencies. Two distinct operations under one verb: `cargo update` (refresh
  `Cargo.lock` to the latest SEMVER-COMPATIBLE versions — routine, reversible, the monthly default) and `cargo upgrade`
  (rewrite version requirements in `Cargo.toml`, which can cross MAJOR versions — explicit, reviewed, occasional). The
  toolchain is pinned at Rust 1.96.0 (`rust-toolchain.toml`); updates must keep building under it. Trigger when the user
  says "update the crates", "bump dependencies", "cargo update", "upgrade the Rust deps", or as the crate half of a
  periodic refresh. This is SEPARATE from `update-web-assets` (vendored Bootstrap/HTMX/Alpine/Icons) — never bundle the
  two in one commit. To do BOTH in one periodic sweep (still two commits), use the `update` skill (`/update`).
---

# Updating Rust crate dependencies

The workspace pins a toolchain (`rust-toolchain.toml` → **1.96.0**) and declares shared versions in the root
`[workspace.dependencies]`, which member crates reference via `<dep>.workspace = true`. Some deps are deliberately held
back — e.g. `tower-cookies = "0.10"` / `maud = "0.26"` are capped because newer versions require axum 0.8 (see the
comment in `Cargo.toml`). **Respect existing pin comments**; if a bump would cross one, stop and surface it rather than
forcing it.

## Two operations — pick the right one

### `cargo update` — the default (lockfile only)

Refreshes `Cargo.lock` to the newest versions allowed by the existing `Cargo.toml` requirements. Stays within semver,
edits no manifests, reverts in seconds (`git checkout Cargo.lock`). This is the routine monthly bump.

```bash
cargo update                 # whole workspace, semver-compatible
cargo update -p <crate>      # one dependency
cargo update --dry-run       # preview what would move
```

### `cargo upgrade` — explicit, occasional (manifest requirements)

Rewrites the version requirements in `Cargo.toml` (needs `cargo-edit`: `cargo install cargo-edit`). This is the only way
to cross a **major** version, and majors can bring breaking API changes. Run it deliberately, one crate at a time, and
read the changelog first:

```bash
cargo upgrade --dry-run                   # preview requirement rewrites
cargo upgrade -p <crate> --incompatible   # allow a major bump for one crate
```

A major bump that ripples across crates (e.g. an axum 0.7→0.8 that drags tower-cookies + maud) is an architecture
decision — take it through `/council` before editing manifests.

## MSRV gate — stays at 1.96.0

Do not bump `rust-toolchain.toml` as part of a dependency update. If a new crate version requires a rustc newer than the
pinned 1.96.0, that is its own decision: stop, name the crate and the MSRV it demands, and ask before raising the
toolchain. Keeping the pin is what guarantees dev == KIND == prod build the same bytes.

## Verification — the standard gate

Run the workspace's canonical pre-commit gate (from `CLAUDE.md`), under the pinned toolchain:

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- `cargo test --workspace` needs Docker (Postgres via `testcontainers`).
- A new clippy version may surface new pedantic lints (workspace clippy is pedantic-at-warn, `unsafe_code = "forbid"`).
  Fix them in the same commit; don't blanket-`allow`.
- If a transitive bump breaks a build, prefer pinning that one crate back (`cargo update -p <crate> --precise <ver>`)
  over reverting the whole lockfile.

## Commit discipline

- **Branch → PR → auto-merge — never commit on `main`.** Per [`CLAUDE.md`](../../../CLAUDE.md) Commit discipline, do the
  bump on a topic branch (`git switch -c <topic>`), push and open a PR (`git push -u origin <topic>` → `gh pr create`),
  then enable auto-merge (`gh pr merge --auto --squash`). `ci.yml` runs on the PR and GitHub squash-merges it once every
  required check is green — never commit to `main`, never merge by hand.
- One commit for the crate bump, separate from any web-asset refresh.
- `cargo update`-only changes touch just `Cargo.lock`: `chore(deps): cargo update (lockfile)`.
- `cargo upgrade` changes also touch `Cargo.toml` (root and/or members): `chore(deps): bump <crate> <old> -> <new>`.
- Tests in the same commit as any code change made to accommodate a new API (TDD discipline from `CLAUDE.md`).

## Cadence

Monthly for `cargo update` (cheap, reversible). `cargo upgrade` / majors only when a specific crate needs it or a
security advisory lands — and never as a drive-by alongside the lockfile refresh.
