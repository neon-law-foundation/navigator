# Neon Law Navigator documentation index

This is the front door for humans and LLM agents. Top-level files in `docs/` are published on the website at
`/docs/<slug>`; nested files stay repo-local unless another page links to them.

Read [`glossary.md`](glossary.md) before using domain words. Read [`access-model.md`](access-model.md) before making
claims about `client`, `staff`, `admin`, or project participation. Read
[`agent-decision-councils.md`](agent-decision-councils.md) before using the Engineering, Legal, or Client Council review
patterns.

## How this index works

- **Start with the task.** Use the sections below to find the workflow, system, or integration you are touching.
  **Confirm the vocabulary.** Use [Glossary quick links](#glossary-quick-links) for the core nouns, then jump to
  [`glossary.md`](glossary.md) for the full definition.
- **Follow the most specific doc.** If two docs overlap, prefer the doc closest to the thing being changed, and keep the
  broader doc as orientation.
- **Keep links descriptive.** Link to the page or heading that answers the reader's actual question.

## Glossary quick links

These are the terms agents most often need before making a decision. The full alphabetical reference is
[`glossary.md`](glossary.md); notation-specific vocabulary is in [`notation.md`](notation.md).

- [AIDA](glossary.md#aida) — domain agent persona and protocol bridge. See
  [`aida-a2a-interaction.md`](aida-a2a-interaction.md) and [`gemini-enterprise-mcp.md`](gemini-enterprise-mcp.md).
- [Blob](glossary.md#blob) — stored bytes behind `cloud::StorageService`. See
  [`cloud-operations.md`](cloud-operations.md) and [`git-project-repos.md`](git-project-repos.md).
- [Council](glossary.md#council) / [Counsel](glossary.md#counsel) — decision councils and attorney spelling. See
  [`agent-decision-councils.md`](agent-decision-councils.md).
- [`ctx.run`](glossary.md#ctxrun) — Restate journaled side-effect primitive. See
  [`durable-workflows.md`](durable-workflows.md) and [`agent-workflows.md`](agent-workflows.md).
- [Document](glossary.md#document) — project-scoped reference to a Blob. See [`gov-forms.md`](gov-forms.md) and
  [`docusign-esignature.md`](docusign-esignature.md).
- [Engagement / Retainer](glossary.md#engagement--retainer) — client-English name for a running Notation. See
  [`retainer_intake.md`](retainer_intake.md) and [`notation-authoring.md`](notation-authoring.md).
- [Participation](glossary.md#participation) — per-project scope row, not system role. See
  [`access-model.md`](access-model.md) and [`oidc.md`](oidc.md).
- [Person](glossary.md#person) / [Entity](glossary.md#entity) — human and legal-organization nouns. See
  [`bulk-contact-import.md`](bulk-contact-import.md) and [`access-model.md`](access-model.md).
- [Project](glossary.md#project) — matter container. See [`git-project-repos.md`](git-project-repos.md) and
  [`nautilus-workflows.md`](nautilus-workflows.md).
- [Workflow Runtime](glossary.md#workflow-runtime) — durable runtime model. See
  [`durable-workflows.md`](durable-workflows.md) and [`cronjobs.md`](cronjobs.md).

## Agent operating model

- [`agent-workflows.md`](agent-workflows.md) — the two codebase actions: create a PR, or review/update an existing PR.
  Preparation, GitOps, Markdown lint, Restate, and workflow authoring are supporting checks inside those actions.
- [`agent-decision-councils.md`](agent-decision-councils.md) — Engineering Council, Legal Council, Client Council.
  [`cloud-operations.md`](cloud-operations.md) — local dev, GCP setup, deploy, prod DB, spend, observability.
  [`rust-programming.md`](rust-programming.md) — Rust language conventions, async, Axum, SeaORM, service lifecycle.

## Vocabulary and access

- [`glossary.md`](glossary.md) — workspace vocabulary. [`notation.md`](notation.md) — notation-system vocabulary.
  [`access-model.md`](access-model.md) — role + participation authorization model. [`oidc.md`](oidc.md) — OpenID Connect
  login and role loading. [`i18n.md`](i18n.md) — English-first rule and the two allowed localization surfaces.

## Workspace and development

- [`workspace-layout.md`](workspace-layout.md) — crate map. [`RUNBOOK.md`](RUNBOOK.md) — local KIND runbook.
  [`test-database.md`](test-database.md) — test Postgres model. [`env-driven-devx.md`](env-driven-devx.md) —
  env-var-driven dev and deploy surfaces. [`assets.md`](assets.md) — photography pipeline (build/upload/pull); pull
  photos down for local dev. [`secrets-doppler.md`](secrets-doppler.md) — Doppler and secret handling.
  [`editing-workflows.md`](editing-workflows.md) — editing notation templates.
  [`notation-authoring.md`](notation-authoring.md) — authoring notation templates. [`lsp/README.md`](lsp/README.md) —
  editor integrations for notation diagnostics.

## Shipping and operations

- [`gitops.md`](gitops.md) — branch, PR, release tag, deploy. [`gke-prod.md`](gke-prod.md) — GKE production
  architecture. [`oss-install.md`](oss-install.md) — installing Neon Law Navigator on your own cloud.
  [`multi-cloud.md`](multi-cloud.md) — AWS, Azure, and self-hosted sketches. [`observability.md`](observability.md) —
  logs, traces, metrics, and the no-content rule. [`durable-workflows.md`](durable-workflows.md) — Restate durable
  execution and operations. [`cronjobs.md`](cronjobs.md) — scheduled jobs.
  [`deploy/gke-power-push-example.md`](deploy/gke-power-push-example.md) — deploy walkthrough example.

## Legal workflows and documents

- [`retainer_intake.md`](retainer_intake.md) — retainer intake state machine.
  [`northstar-estate-flow.md`](northstar-estate-flow.md) — estate-planning flow.
  [`nautilus-design.md`](nautilus-design.md) — Nautilus design. [`nautilus-workflows.md`](nautilus-workflows.md) —
  Nautilus workflow details. [`gov-forms.md`](gov-forms.md) — government form provenance.
  [`docusign-esignature.md`](docusign-esignature.md) — DocuSign e-signature.
  [`solana-attestation.md`](solana-attestation.md) — on-chain attestation. [`erd.md`](erd.md) and [`erd.svg`](erd.svg) —
  database relationship diagram.

## Data, billing, and integrations

- [`aida-a2a-interaction.md`](aida-a2a-interaction.md) — AIDA, A2A, and MCP interaction.
  [`gemini-enterprise-mcp.md`](gemini-enterprise-mcp.md) — Gemini Enterprise MCP integration.
  [`bulk-contact-import.md`](bulk-contact-import.md) — bulk contact import payloads.
  [`email-events-pipeline.md`](email-events-pipeline.md) — inbound/outbound email events.
  [`git-project-repos.md`](git-project-repos.md) — per-Project git repositories.
  [`iceberg-archive.md`](iceberg-archive.md) — archive export. [`recurring-billing.md`](recurring-billing.md) —
  recurring billing. [`third-party-integrations.md`](third-party-integrations.md) — vendor account convention.
  [`xero-billing.md`](xero-billing.md) — Xero billing.
