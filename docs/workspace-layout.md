# Workspace layout

Neon Law Navigator is a single Cargo workspace. Every executable and library in it is written in Rust — the `navigator`
CLI (the `cli` crate) orchestrates every machine-bound flow, so there are no shell scripts and no Makefile. This doc is
the canonical crate map; the workspace `CLAUDE.md` links here.

```text
rules        lib   — validation rules
store        lib   — SeaORM entities, migrations, canonical seed
repos        lib   — per-Project bare git repos (append-only, single `main`); backs `web::git_http`
import       lib   — bulk contact-import engine (entities + persons + roles); one lib, many surfaces
cli          bin   `navigator` — validate, import, import-contacts, seed, list; login + live-site matter driver; KIND dev-loop + deploy + `gcp setup` orchestration (in `cli::devx`)
web          bin   `web` — axum + SeaORM + maud; hosts both AIDA surfaces + git smart-HTTP + LFS
views        lib   — maud HTML view components
workflows    lib   — durable workflow primitives (Restate-shaped); `web` submits jobs to the broker
workflows-service bin `workflows-service` — Restate worker; hosts the `Notation`, `Archives`, `Statutes`, billing-canary services + journal; only `restate-sdk` consumer
cloud        lib   — storage trait + GCS/Fs backends
live-inquiry lib   — provider-agnostic transcript segments, Inquiry normalization, and Coverage Findings for live-coverage demos/runtime
mcp          lib   — MCP server merged into `web` at /mcp (Claude / LibreChat / Cursor)
features     lib   — Cucumber-rust BDD suite (`cargo test -p features`)
forms        lib   — vendored government forms registry (bucket-shaped paths + bundled canonical PDFs)
lsp          bin   `navigator-lsp` — LSP server: rule diagnostics + source.fixAll
pdf          lib   — Typst-backed PDF rendering (Noto Serif firm typeface); persists via `cloud`
archives     lib   — nightly Postgres→Parquet snapshot Restate workflow + diagnostic email
statutes     lib   — weekly Nevada Revised Statutes scraper; bin `statutes_sync` reconciles into Postgres
billing      lib   — `BillingProvider` seam (Xero `ACCREC` invoices / stub) for the matter-close fee
billing-workflows lib — worker-side billing workflows (nightly Xero canary), hosted by workflows-service
```

## Adding a new crate

A new workspace crate must be added to the `images/Dockerfile.*` `COPY` lists so the prod images still build — see
[`durable-workflows.md`](durable-workflows.md) and [`rust-programming.md`](rust-programming.md).
