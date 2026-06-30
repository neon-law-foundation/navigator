# workflows-service

The Restate **worker** binary. Where [`workflows`](../workflows) is the *outbound* side — the library `web` uses to
submit jobs — this crate is the *inbound* side: the long-running endpoint that **Restate Cloud dials back into** to
drive durable execution. It is the only crate in the workspace that depends on `restate-sdk`.

In the reference deploy it runs as the `workflows-service` Service behind `workflows.your-domain.example` (Restate
worker + Envoy sidecar), one worker pod for *every* workflow — new workflows bind onto this endpoint, never a new pod.

## What it hosts

- **`notation_service.rs`** — the `Notation` virtual object's handlers: one object per Notation, two timelines
  (questionnaire + workflow) on one journal.
- **`journal.rs`** — projects each state-machine transition into the `notation_events` journal in Postgres (via the
  `store` crate) so every advance is auditable.
- **`archives` (dep)** — the Archives workflow + GCP cost step, hosted here on the same worker.

## Getting started

```bash
cargo test -p workflows-service
```

Registration is **not** automatic on Restate Cloud. In KIND, `cargo run -p cli -- restate register` wires the worker URL
into the in-cluster broker, so the dev loop just works. In **Restate Cloud**, registration is an explicit admin
operation — rolling a new worker image does **not** re-register, so a newly added service stays invisible at the ingress
(`404 "service not found"`) until you re-register the deployment. This is the single most common "why didn't my workflow
run" cause; the full mechanism, the auth-token-vs-admin-token distinction, and the re-register recipe are in
[`docs/durable-workflows.md`](../docs/durable-workflows.md).

See the workspace `README.md` "Workflows" section for the end-to-end picture and the `workflows` README for the spec /
runtime side.
