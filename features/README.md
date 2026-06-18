# features

The Cucumber-rust BDD suite — Navigator's executable specification. Gherkin `.feature` files describe how lawyers and
clients actually move through the firm (intake, onboarding, the portal, e-signature, filings, closing) and the runners
drive the real `web` router, the workflow walkers, the `rules` validators, and the OIDC callback against in-memory
wiring. No production code depends on this crate; it exists to be run.

Legal flows are **feature-first**: the `.feature` is written before the template and the workflow it specifies, so the
Gherkin is the spec and the Rust is the proof.

## Layout

- `tests/features/*.feature` — the Gherkin specs (one file per flow).
- `tests/<name>.rs` — one `harness = false` runner per feature file; each owns its own `cucumber::World` and step set.
- `src/lib.rs` — shared scaffolding only (the pieces more than one runner would otherwise duplicate): the per-test
  Postgres-schema `in_memory_db`, an in-memory `AppState` constructor, an unsigned-JWT forger for the OIDC scenarios,
  and `template_shapes`.
- `src/webdriver.rs` — behind the `webdriver` cargo feature; pulls in `fantoccini` for the browser-driven walkthroughs.

## What the specs cover

- **End-to-end journeys** — one client + one lawyer across the whole arc of a representation (intake → portal → work
  product → signature → filing / close), crossing the seams between crates rather than pinning one surface:
  `northstar_estate` (estate plan: review surface → closing → flat-fee invoice), `nest_formation` (Nevada entity
  formation), `nexus_fractional_gc` (ongoing fractional-GC engagement + repo delivery + question routing),
  `nautilus_debt_shield` (debt-collection shield), `inbound_email_round_trip` (the "headless Front" loop),
  `git_repo_collaboration` (Project repo + PAT + governed-expunge), `bulk_import_engagement` (import → matter), and
  `spanish_client_journey` (the `/es` funnel). Shared journey mechanics live in `src/journey.rs`; the recipe for editing
  the workflows they exercise is [`docs/editing-workflows.md`](../docs/editing-workflows.md).
- **Client journey** — `retainer_intake`, `estate_intake`, `intake_language`, `onboarding_welcome`,
  `aida_welcome_chain`, `closing_letter`.
- **Portal** — `portal_landing_per_role`, `portal_projects_detail`, `portal_projects_writes`,
  `portal_admin_firm_surface`, `brand_routing`.
- **Documents & signature** — `trust_esignature`, `esignature_webhook`, `acroform_fill`, `drive_sync_resume`.
- **Workflow shapes** — `legal_workflow_shapes`, `compliance_filings_workflow_shapes`, `nonprofit_workflow_shapes`,
  `nautilus_workflows`, `annual_report_filing`.
- **Agent & platform** — `mcp_create_notation`, `oidc_callback`, `template_validate`,
  `deploy_the_navigator_walkthrough`, `workshop_navigator_walkthrough`.

## Running

```bash
# Whole suite (Postgres comes from testcontainers — Docker required).
cargo test -p features

# One flow, with output, including the browser-driven walkthroughs.
cargo test -p features --features webdriver --test retainer_intake -- --nocapture
```

**Caveat — Cucumber's `.run()` is non-fatal:** a failing or *skipped* scenario does not fail `cargo test`. Always scan
the output for scenario failures **and** for `Step skipped` (a drifted step matcher silently skips its assertion) rather
than trusting the exit code alone.
