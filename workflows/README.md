# workflows

Durable workflow primitives shaped after Restate. Ships an in-memory runtime for tests and dev plus a Restate adapter
that posts to a broker over HTTP. This is the **outbound** side — the library `web` uses to *submit* jobs; the
**worker** that Restate dials back into to *run* the handlers is the separate
[`workflows-service`](../workflows-service) binary (the only crate that depends on `restate-sdk`).

Retainer intake — the state machine that walks a new client from "form submitted" to "sent for signature" — was the
first workflow. Drive-sync, archives, and the filing / certified-mail / document-open steps now ship alongside it.

## Getting started

```bash
cargo test -p workflows
```

The in-memory runtime is the default; tests never need a broker. The Restate adapter is exercised via `wiremock` so unit
tests can assert on the exact HTTP shape without standing up Restate locally.

To use the crate, hand `web` a `Arc<dyn WorkflowRuntime>` in `AppState`. The in-memory runtime is fine for dev and CI;
in production the binary wires the Restate adapter and points it at the broker URL.

## What's next

Workflow specs live in the YAML frontmatter of notation templates under `notation_templates/forms/...` or
`notation_templates/neon_law/...` — see `notation_templates/neon_law/shared/retainer.md`. The crate extracts the
`workflow:` block, typechecks the transitions, and produces a `WorkflowSpec` that either runtime can execute.

Adding a workflow is: one notation template (markdown + frontmatter), one `specs.rs` constant, and the handlers that
signal it from `web`.
