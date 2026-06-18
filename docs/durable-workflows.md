# Durable workflows

How Navigator runs long-lived, crash-safe work — retainer intake, Drive sync, the nightly Archives backup — on
[Restate](https://restate.dev), and how an operator tells *why one didn't run*.

> **The one rule that costs two hours when forgotten:** a registered Restate deployment is a **snapshot, not a
> subscription**. Rolling a new worker image does **not** re-register it. A service you just added (or its new
> handlers) stays invisible at the ingress — `404 "service not found"` — until you **re-register the deployment**.
> See [The registration gotcha](#the-registration-gotcha).
>
> **The mental model:** **Kubernetes owns the clock; Restate owns the journal.** A trigger fires an invocation once;
> Restate makes its execution durable — journals every step, retries failures, runs it to completion on the worker.

## Two sides: submit vs. run

Durable execution is split across two crates so the rest of the workspace never binds to `restate-sdk`.

| | `workflows` (lib) | `workflows-service` (bin) |
| --- | --- | --- |
| Role | **Outbound** — *submit* a job | **Inbound** — *run* the handlers |
| Who calls it | `web` and the `archives` trigger | Restate dials *into* it |
| Runtime | `InMemoryRuntime` (dev/CI) or `RestateRuntime` | the worker itself |
| `restate-sdk` | no | **yes — the only crate with it** |
| Tested via | `wiremock` (exact HTTP shape) | `cargo test -p workflows-service` |

One worker pod hosts **every** service — new workflows bind onto the same endpoint, never a new pod. Today that worker
serves one virtual object — `notation` (questionnaire + workflow timelines on one journal) — and the durable workflows
`Archives`, `Statutes`, `Heartbeat`, `BillingCanary`, `MatterCloseInvoice`, `RecurringBilling`, and `ReconcileInvoices`.
The exact set is the single source of truth in `workflows_service::registry`, whose tests assert every workflow name is
PascalCase (template filenames follow the separate snake_case convention `F103` enforces) and that the registry never
drifts from the worker's actual `.bind(...)` calls. In the reference deploy the worker runs behind
`workflows.your-domain.example` (Restate worker + Envoy sidecar).

The runtime is chosen by `RESTATE_BROKER_URL`: unset means in-process / in-memory, so KIND works with zero config; set
means the `RestateRuntime` adapter posts to the broker over HTTP. The same selection is used in `web::main`,
`web::drive_sync`, and the `archives` trigger.

## Three ways a workflow starts

Every workflow is kicked off in exactly one of three ways. All three land on the same worker.

| Mode | Fired by | Example | Code |
| --- | --- | --- | --- |
| **Event-driven** | `web`, on a user action | retainer intake; Drive sync | `web::retainer_walk` |
| **Scheduled** | a Kubernetes `CronJob` | Archives; statutes; canary | `archives`/`statutes`/`billing-workflows` |
| **Manual** | an admin button | `POST /portal/admin/archives/run` | `web::archives` |

The submit shape is identical in all three: `POST {ingress}/{Service}/{key}/run` (append `/send` for one-way), with the
optional bearer. The shared helper is `workflows::start_workflow`.

## Where the schedule lives

**Restate has no cron.** The nightly schedule is a **Kubernetes `CronJob`** named `archives-trigger` in the `navigator`
namespace — stored in the cluster (etcd), evaluated by the kube-controller-manager, sourced from
`examples/deploy/k8s/exports/cron-archives-trigger.yaml` (`schedule: "0 10 * * *"`, UTC, = 02:00 PST). Each firing runs
the thin `navigator-archives-trigger` image — one `POST` to the ingress, then it exits. Restate owns the retry schedule
from there.

```text
kube CronJob (0 10 * * *) --fires--> trigger pod --POST /Archives/<date>/run/send--> Restate ingress
                                                                                          | Accepted
                                                                                          v
                                                          worker runs: snapshot -> cost -> email (journaled)
```

To inspect or fire the schedule by hand:

```bash
kubectl -n navigator get cronjob archives-trigger
kubectl -n navigator create job --from=cronjob/archives-trigger archives-trigger-manual-001
```

## Idempotency is the workflow key

Restate admits **at most one invocation per workflow key**. The key choice *is* the idempotency policy:

- **Nightly Archives** keys on the **UTC run date**, so a double-fire on the same day is a silent no-op — exactly what
  a backup wants.
- **Manual runs** key on a unique `manual-<uuid>`, so every click actually executes and emails — a test button that
  deduped against the nightly run would look broken.

## Auth: two tokens, two ports

The single most error-prone area: there are **two different credentials** on **two different ports**, and conflating
them is what silently broke prod.

- **Submit / trigger** an invocation — ingress, port **`:8080`**. Credential: `RESTATE_AUTH_TOKEN`, the Restate Cloud
  **`key_…`** API key (72 chars). Lives in the k8s secret `navigator-web-secrets`, sourced from Doppler `prd`.
- **Register** a deployment — admin API, port **`:9070`**. Credential: the **SSO access token** (a long JWT written by
  `restate cloud login`), in `~/.config/restate/config.toml` under `[global.cloud] access_token`.

The secret takes the **ingress `key_`**, never the SSO JWT — they look nothing alike. If `navigator-web-secrets` holds a
long `eyJ…` JWT, it is **wrong** and the ingress answers `401 Unauthenticated`. Doppler `prd` is the source of truth
(see [secrets in Doppler](secrets-doppler.md)). We once had this token drift across three places — Doppler `key_`,
Secret Manager `stub`, and a stale SSO JWT in the k8s secret — which is the failure this section exists to prevent.

## The registration gotcha

Restate routes the ingress to **registered** services. Registration is a **snapshot of the worker's handler list at
register time** — it does not follow new deploys.

- In **KIND**, `cargo run -p cli -- restate register` wires the worker URL into the in-cluster broker, so the dev loop
  just works.
- In **Restate Cloud**, registration is an **explicit admin operation**. Rolling a new worker image does **not**
  re-register, so a service or handler added since the last registration is invisible at the ingress:

```text
POST :8080/Archives/<key>/run/send
404 {"message":"service 'Archives' not found, make sure to register the service before calling it."}
```

**Fix — re-register the deployment** (re-runs discovery against the live worker and picks up every service). Either use
the Restate Cloud console (your env, Deployments, Register deployment, overwrite the existing endpoint), or the admin
REST API authenticated with the SSO token:

```bash
ADMIN="https://<env>.env.<region>.restate.cloud:9070"
TOK=$(sed -n 's/^access_token = "\(.*\)"/\1/p' ~/.config/restate/config.toml | head -1)
# dry-run first — confirm the discovered service list before committing:
curl -s -X POST "$ADMIN/deployments" -H "Authorization: Bearer $TOK" \
  -d '{"uri":"https://workflows.your-domain.example/","force":true,"dry_run":true}' | jq '.services[].name'
# then commit (drop dry_run):
curl -s -X POST "$ADMIN/deployments" -H "Authorization: Bearer $TOK" \
  -d '{"uri":"https://workflows.your-domain.example/","force":true}'
```

The `restate` CLI is configured for the env but may report "Unable to connect" to `:9070` even when the host is
reachable; the admin REST API above works directly with the same SSO token.

### How `power-push` re-registers (step 7d)

After rolling both deployments, `power-push` re-registers the worker so any handler added since the last registration is
reachable. Two design points:

- **It targets the real worker URL, not the placeholder.** The `navigator` CLI resolves the register target in
  precedence order: explicit `--url` → `NAVIGATOR_WORKFLOWS_URL` → derived
  `https://workflows.<NAVIGATOR_PRIMARY_DOMAIN>/` → the `workflows.example.com` placeholder of last resort. The
  derivation step exists because the 2026-06-10 ship had a domain configured but no explicit `NAVIGATOR_WORKFLOWS_URL`,
  fell through to the placeholder, and silently no-op'd the register. Under `doppler run --config prd` the explicit URL
  is now injected; the derivation is the belt-and-suspenders default for an operator who hasn't set it.
- **It picks its transport from the environment.** When `RESTATE_ADMIN_URL` **and** `RESTATE_ADMIN_TOKEN` are both set
  (wired in Doppler `prd`), `restate_register` POSTs `{"uri":<worker>,"force":true}` straight to the admin REST API —
  headless, needs no `restate cloud env configure` (which requires a TTY) and works with a non-expiring admin-scoped API
  key. Otherwise it shells out to the pinned `restate` CLI, which only reaches Restate Cloud when the operator has a
  selected environment in `~/.config/restate/config.toml` and a fresh SSO token; with neither, the CLI defaults to the
  `local` environment (`localhost:9070`) and the step fails. Wiring the two env vars is what makes an unattended ship
  re-register reliably.
- **It warns and continues — it does not gate the ship (yet).** A `:9070` admin endpoint that is firewalled from the
  operator's network, or an expired SSO token, would otherwise block every ship. Re-register is therefore best-effort: a
  failure prints `WARN: Restate re-register failed (continuing)` and the ship completes. The cost of warn-and-continue
  is a silent `404` on a *newly added* service until someone re-registers by hand; the `Notation` (retainer) service is
  already registered and unaffected. Flip step 7d to fail-the-ship only once `:9070` reachability is proven from the
  ship host and an **admin-scoped** token (`RESTATE_ADMIN_TOKEN` + `RESTATE_ADMIN_URL` in Doppler `prd`) is wired —
  until then a hard gate trades a rare silent 404 for a frequent blocked ship.

## Adding a workflow

1. Author the spec in a notation template's `workflow:` frontmatter (see [notation authoring](notation-authoring.md))
   or, for non-notation flows, bind a new Restate service in `workflows-service`.
2. Signal it from `web` (event-driven) or add a trigger (scheduled / manual).
3. Ship the worker — see [GKE production](gke-prod.md) and the `power-push` skill (always both `navigator-web` and
   `workflows-service` at one SHA).
4. **Re-register the deployment** (above) — otherwise the new service `404`s at the ingress no matter how clean the
   deploy was. This step is invisible in `kubectl` and easy to forget.

## The heartbeat: proving the engine itself is alive

Every other scheduled workflow proves an *integration* — `Archives` proves the database and GCS are reachable,
`BillingCanary` proves Xero still agrees with us. None answers the bluntest operator question: *is durable execution
itself alive right now?* A silent `Archives` is ambiguous (engine down, or just a GCS outage?).

`Heartbeat` removes the ambiguity. It is a two-step Restate workflow (`beat` → `notify`) that depends on **nothing** —
no database, no object storage, no third-party API — so a green run can only mean the engine accepted an invocation,
journaled step one, and ran step two to completion. It fires **every six hours** (`0 */6 * * *` UTC), keyed on the UTC
date + hour so the four daily runs each get a distinct workflow key (a date-only key would dedupe three of four into
no-ops). Each run emails firm ops a **"Where to look"** report carrying the exact Restate Cloud + GCP console links and
the kubectl/curl chain below — so the same email that confirms health onboards whoever debugs its *absence*.

The signal that matters most is the missing one: **a six-hour window with no heartbeat email means the engine may be
down** — walk the chain below. Like every new service, `Heartbeat` is invisible at the ingress until the deployment is
**re-registered** (see [the registration gotcha](#the-registration-gotcha)); the absence of its first email after a ship
is itself the test that re-register happened.

## Debugging "the workflow didn't run"

Work down the chain; the break is almost always near the top:

1. **Did the trigger fire?** `kubectl -n navigator get cronjob archives-trigger` (last schedule) plus the trigger pod
   logs. For manual: did the admin button return the confirmation page or an error?
2. **Did the ingress accept it?** A `401` is a wrong or stale `RESTATE_AUTH_TOKEN`; a `404 service not found` is the
   registration gotcha — both have dedicated sections above.
3. **Did the worker run it?** Check the invocation in the Restate Cloud console (Invocations) or via the admin API; a
   failing step retries and surfaces there.
4. **Did the side effect happen?** Email transmits through SendGrid only when `NAVIGATOR_EMAIL_BACKEND=sendgrid` and
   `SENDGRID_API_KEY` are present; otherwise the worker silently uses a capturing backend that logs "sent" without
   sending. See the `power-push` skill's manifest-drift notes.

## See also

- The *what* of each individual workflow: [notation](notation.md), [retainer intake](retainer_intake.md),
  [Nautilus workflows](nautilus-workflows.md).
- Scheduling any periodic job (the CronJob pattern, both flavors): [Scheduled jobs](cronjobs.md).
- Deploy and secret mechanics: [GKE production](gke-prod.md), [secrets in Doppler](secrets-doppler.md).
- Crate entry points: [`workflows/README.md`](../workflows/README.md) and
  [`workflows-service/README.md`](../workflows-service/README.md).
