---
name: durable-execution
description: >
  The operational contract for Navigator's Restate-backed durable execution — how to keep the running workflows alive,
  diagnose why one didn't fire, and not break them. Covers the submit-vs-run split (workflows lib / workflows-service
  bin), the service inventory, the six-hourly Heartbeat liveness canary, the three start modes, and — front and center —
  the ranked failure modes with their one-line detectors and fixes (missing trigger image / Forbid wedge / registration
  drift / stale auth token / wrong email backend). Trigger when touching workflows-service, a *-trigger CronJob, the
  image Dockerfiles, when re-registering with Restate, when a scheduled or manual workflow "didn't run", or when adding
  a workspace crate (it must enter the Dockerfile COPY lists). This engine executes BINDING legal artifacts (retainer
  dispatch, signatures, filings, the matter-close invoice), so an outage is a diligence concern, not a backlog item.
  Debugging surfaces carry invocation ids and service names, NEVER client content (see the observability skill). To
  *add* a new workflow use create-legal-workflow; this skill keeps the existing ones alive. Deep architecture lives in
  docs/durable-workflows.md.
---

# durable-execution

Durable execution is not neutral plumbing here. It runs the firm's **binding obligations** — retainer dispatch,
signature flows, county / NV SoS filings, the matter-close invoice. When the engine silently stops, an engagement letter
doesn't go out and a deadline can slip, so reliability is a **competence-and-diligence** obligation with a real
escalation, not an SRE nicety. The control that makes it auditable is the Heartbeat: its *absence* is the alarm, and it
proves liveness without ever touching a client record.

## The model in one breath

**Kubernetes owns the clock; Restate owns the journal.** A trigger fires an invocation once; Restate makes execution
durable — journals every step, retries failures, runs it to completion on the one worker. Submit and run are split so
the workspace never binds `restate-sdk` outside one crate:

- `workflows` (lib) — **outbound**: `start_workflow` POSTs to the ingress. Called by `web` and every `*-trigger`.
- `workflows-service` (bin) — **inbound**: the worker Restate dials into. The only `restate-sdk` consumer.

Full architecture, the registration gotcha, and the auth model:
[`docs/durable-workflows.md`](../../../docs/durable-workflows.md).

## Service inventory

One worker pod hosts every service; new workflows bind onto the same endpoint, never a new pod. The canonical list is
`workflows_service::registry` (guarded by tests). Today: the `notation` virtual object plus the durable workflows
`Archives`, `Statutes`, `Heartbeat`, `BillingCanary`, `MatterCloseInvoice`, `RecurringBilling`, `ReconcileInvoices`.

**Heartbeat** is the liveness canary: a two-step (beat → notify), zero-dependency workflow that emails firm ops **every
6 hours** with the Restate + GCP links and the kubectl chain to debug a missing beat. A six-hour gap with no heartbeat
email is the alarm. After any worker deploy, the heartbeat's first email is also the proof that re-registration
happened.

## Three ways a workflow starts

| Mode | Fired by | Example | Idempotency key |
| --- | --- | --- | --- |
| Event-driven | `web`, on a user action | retainer intake | per-domain |
| Scheduled | a Kubernetes CronJob | Archives (nightly), Heartbeat (6h) | nightly = date; Heartbeat = date+hour |
| Manual | an admin button / `kubectl create job --from=cronjob/...` | recovery, testing | `manual-<uuid>` |

Get the key wrong and Restate silently dedupes: a date-only key on a six-hourly job runs once and drops three — a
failure that emits nothing. Match the key to the cadence.

## Ranked failure modes (what actually breaks, with the detector)

1. **Trigger image missing from Artifact Registry.** A fresh `*-trigger` build also fails if any workspace crate is
   absent from the Dockerfile COPY lists (the `forms` crate did exactly this and took every trigger image down).
   *Detect:* `navigator doctor` flags `ImagePullBackOff`; `gcloud artifacts docker images describe <image>:<tag>`.
   *Fix:* rebuild + push the trigger image (`docker build -f images/Dockerfile.trigger --build-arg CRATE=<crate>
   [--build-arg BIN=<bin>] -t <reg>/navigator-<name>:<sha> .` then push), repoint the CronJob image.
2. **Forbid wedge.** A failed trigger Job stays `Active`, and `concurrencyPolicy: Forbid` skips every subsequent run — a
   silent multi-day outage. *Detect:* `navigator doctor`. *Fix:* `kubectl -n navigator delete job <name>`; the
   `activeDeadlineSeconds: 120` backstop now self-terminates a stuck job going forward.
3. **Registration drift (404).** Rolling a new worker image does NOT re-register it; a service added since the last
   registration 404s at the ingress (this hid `Heartbeat` and `RecurringBilling`). *Detect:* the heartbeat email never
   arrives after a deploy; `curl :8080/.../run/send` → 404. *Fix:* re-register (below).
4. **Stale `RESTATE_AUTH_TOKEN` (401).** A wrong/expired ingress key. *Detect:* the trigger logs a rejected event with
   `status=401`, or the metric `navigator.workflow.trigger.fired` shows `outcome=rejected`. *Fix:* the `key_…` ingress
   key in `navigator-web-secrets`, sourced from Doppler `prd` (never the SSO JWT).
5. **Email backend not SendGrid.** The worker logs `backend=SendGrid` at boot; anything else silently captures (logs
   "sent" without sending). *Detect:* worker boot log. *Fix:* `NAVIGATOR_EMAIL_BACKEND=sendgrid` + `SENDGRID_API_KEY`.

## Re-register (the one most-forgotten step)

The `restate` CLI may report "Unable to connect" to `:9070` even when it is reachable — the admin REST API works
directly with the SSO token. This is the form that works:

```bash
TOK=$(sed -n 's/^access_token = "\(.*\)"/\1/p' ~/.config/restate/config.toml | head -1)
ADMIN="https://<env>.env.<region>.restate.cloud:9070"
# dry-run discovery first:
curl -s -X POST "$ADMIN/deployments" -H "Authorization: Bearer $TOK" -H "Content-Type: application/json" \
  -d '{"uri":"https://workflows.<your-domain>/","force":true,"dry_run":true}'
# then commit (drop dry_run). force=true overwrites the existing endpoint and picks up every service.
```

Two tokens, two ports: **submit** uses the ingress (`:8080`) + the `key_…` `RESTATE_AUTH_TOKEN`; **register** uses the
admin API (`:9070`) + the SSO access token from `~/.config/restate/config.toml`. They look nothing alike; conflating
them is what silently broke prod. Never paste either into the repo.

## Debugging "the workflow didn't run" (the playbook)

1. **`navigator doctor`** — wedged trigger Jobs / unready workloads in plain language, each with the fix command. First
   stop.
2. **The trigger metric / logs** — `navigator.workflow.trigger.fired` by `service`/`outcome` in BigQuery, or the
   `workflow trigger accepted|rejected` log events (`status`). 401 = auth, 404 = registration, transport_error = hung
   ingress.
3. **Restate Cloud console → Invocations** — did the worker run it, which step failed. Via the admin API, `/query`
   returns Arrow unless you add the JSON `Accept` header:

   ```bash
   curl -s "$ADMIN/query" -H "Authorization: Bearer $TOK" \
     -H "Content-Type: application/json" -H "Accept: application/json" \
     -d '{"query":"SELECT target_service_name, status FROM sys_invocation ORDER BY created_at DESC LIMIT 5"}'
   ```

4. **The Heartbeat email** — its absence means durable execution itself may be down.

## Invariants that keep it from breaking (don't defeat these)

- **Every workspace member is in every workspace-building Dockerfile.** Add a crate → add `COPY <crate> <crate>` to
  `images/Dockerfile.{trigger,workflows-service,web}`. Guarded by
  `every_workspace_member_is_copied_into_each_workspace_image` in the `cli` crate (`cli::devx`).
- **Registered workflow names are PascalCase** and the registry matches `main.rs`'s `.bind(...)` calls — guarded by the
  `workflows_service::registry` tests (shares `rules::is_pascal_case`; template filenames are the separate snake_case
  convention `N103` enforces).
- **Trigger CronJobs carry `activeDeadlineSeconds` + `startingDeadlineSeconds`** so Forbid can't wedge them.
- **`start_workflow` has a 30s HTTP timeout** so a hung ingress can't keep a trigger pod alive.
- **Debugging stays identifier-and-status only — never client content** (the standing no-content rule; see the
  `observability` skill).

## Boundaries

- To *add* a new workflow (feature → template + questionnaire → Restate handlers): the `create-legal-workflow` skill.
- Telemetry, the trigger metric, the BigQuery landing: the `observability` skill.
- This skill keeps the *running* engine alive.
