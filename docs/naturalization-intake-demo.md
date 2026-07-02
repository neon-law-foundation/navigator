# Walk the naturalization intake locally — `us__naturalization` with the CLI

This is the anyone-can-follow recipe for the immigration demo: bring the whole Neon Law Navigator stack up on one
machine, open a naturalization matter (USCIS Form N-400), answer its intake questionnaire from the terminal, approve it
as staff, and download the rendered intake summary — every step driven by the `navigator` CLI. The walk itself is fully
local: the only services involved run inside the local KIND cluster.

The template behind the demo is `templates/forms/united_states/federal/uscis/us__naturalization.md`: a ten-question
intake questionnaire plus a workflow that parks at [`staff_review`](glossary.md#staff-review) before anything renders.
Today the walk ends with the rendered **intake summary** the template body defines; the vendored N-400 AcroForm itself
is tracked separately (issue #277) and slots into the same walk when it lands.

## Prerequisites

* A checkout of this repository, on `main`.
* Docker running (Docker Desktop on macOS) — KIND runs the whole dependency stack in containers.
* `kind`, `kubectl`, and `helm` on your `PATH` — `start-dev-server` checks for them and says which one is missing.
* The Rust toolchain via `rustup` — the pinned version in `rust-toolchain.toml` is selected automatically on the first
  `cargo run`.
* The Doppler CLI, logged in with access to the `navigator` project's `dev` config — `web` reads a few Doppler-only
  secrets at boot and refuses to start without them. See [`secrets-doppler.md`](secrets-doppler.md).

Every command below runs from the repository root.

## 1. Start the dependency stack

```bash
cargo run --release -p cli -- start-dev-server
```

This brings up a KIND cluster named `navigator` with Postgres, Keycloak, fake-gcs, OPA, Restate, and Grafana LGTM, opens
host port-forwards to each, and writes the connection details to `.devx/env`.

The cluster is a **persistent fixture**: if it is already up from a previous session, re-running the command is exactly
right — it reuses the existing cluster and restores any port-forwards that died with a sleep or reboot. Messages like
"already exists" or "already alive" are success, not failure.

If it instead fails with `bind: address already in use`, a port-forward from an earlier session still holds the port —
one started by hand, or left behind by a previous run that failed partway. Kill them all and re-run; the command
re-opens every forward it needs:

```bash
pkill -f "kubectl.*port-forward"
cargo run --release -p cli -- start-dev-server
```

## 2. Boot `web`

In a second terminal, from the repository root:

```bash
doppler run --project navigator --config dev -- \
  bash -c 'set -a; source .devx/env; set +a; cargo run -p web'
```

`.devx/env` must be sourced **after** Doppler injects its values so the KIND wiring wins on collisions. Wait for the
`web listening` log line — the app is on `http://localhost:3001` (`.devx/env` sets `PORT=3001`).

Leave this terminal running; everything else happens in the first one.

## 3. Seed the database

Load the environment `start-dev-server` wrote, then import the template catalog and pre-seed the staff user:

```bash
set -a; source .devx/env; set +a
cargo run --release -p cli --quiet -- import templates
cargo run --release -p cli --quiet -- grant-staff
```

`import` walks `templates/`, validates every file, and imports the clean ones — `us__naturalization` among them —
auto-registering each referenced question code. It is idempotent: on a database that already holds the catalog it
reports `Imported 0 template(s)`, which is fine. `grant-staff` pre-seeds `staff@neonlaw.com` with the `staff` role so
the admin-gated intake routes authorize; run it **before** logging in so the session carries the role from the start.

## 4. Sign in

```bash
cargo run --release -p cli --quiet -- login --host http://localhost:3001
```

Your browser opens to the local site's sign-in, which lands on the Keycloak dev realm. Sign in as user `staff`, password
`staff`. The very first login prompts for a last name (the realm import omits one) — type anything. The CLI receives a
short-lived bearer token on a loopback listener and stores it at `~/.navigator.json` (`whoami` shows the exact time it
has left). Confirm:

```bash
cargo run --release -p cli --quiet -- whoami
```

It should print `staff@neonlaw.com (staff)` with the time the token has left.

## 5. Open the matter

```bash
cargo run --release -p cli --quiet -- notation create us__naturalization \
  --client-email applicant@example.com
```

The conflict check runs first, then the command prints the new notation's UUID and its review URL. Copy the UUID — every
remaining step takes it. (`applicant@example.com` is a demo address; use any email that is not a real client.)

## 6. Walk the questionnaire

```bash
cargo run --release -p cli --quiet -- intake answer <notation-id>
```

The interactive walk asks one question per prompt, in the questionnaire's own order — the applicant's full legal name,
date of birth, country of birth, country of citizenship, the date they became a lawful permanent resident, a daytime
phone, the eligibility basis (a numbered choice list), marital status (also a choice list), days outside the United
States in the last five years, and the good-moral-character disclosure. Dates are entered as `YYYY-MM-DD`.

To script the same walk non-interactively, pass one `--answer` per question in that order:

```bash
cargo run --release -p cli --quiet -- intake answer <notation-id> \
  --answer "Ada Applicant" \
  --answer "1990-01-15" \
  --answer "Brazil" \
  --answer "Brazil" \
  --answer "2019-03-01" \
  --answer "+1 702 555 0100" \
  --answer "five_year" \
  --answer "single" \
  --answer "180" \
  --answer "No"
```

When the final answer lands the questionnaire reaches `END`, intake persists, and the post-intake workflow advances the
notation to `staff_review`.

## 7. Review as staff: approve and download

```bash
cargo run --release -p cli --quiet -- notation status <notation-id>
cargo run --release -p cli --quiet -- notation approve <notation-id>
cargo run --release -p cli --quiet -- notation document <notation-id> --out /tmp/n400-intake-summary.pdf
```

`status` shows the workflow already at `sent_for_signature__pending` with `document_ready true`: completing the
questionnaire renders the intake summary and parks the walk there. `approve` is an idempotent confirmation of that
render — safe to re-run, a no-op once the document is parked. `document` downloads the rendered PDF — open it and check
the answers you gave in step 6 appear in the summary prose. Nothing outbound happens in this demo: dispatching for
signature (and everything after it) is a separate, deliberate command, and in a real matter a licensed attorney reviews
the rendered summary before anything leaves the firm.

## If the walk 500s after a schema-changing merge (version skew)

The host-side `web` and CLI are built from your checkout, but the in-cluster `workflows-service` pod runs whatever image
the cluster last loaded. After a merge that changes the database schema — and before the next green deploy publishes
fresh images — the two can skew: the questionnaire walk fails with HTTP 500 and the `web` log shows the workflow runtime
rejecting the write, e.g. `null value in column "acting_person_id" of relation "notation_events" violates not-null
constraint`.

The fix is to rebuild the worker image from your checkout and load it into the cluster:

```bash
docker build -f images/Containerfile.workflows-service -t navigator-workflows-service:dev .
docker save navigator-workflows-service:dev -o /tmp/navigator-workflows-service-dev.tar
kind load image-archive /tmp/navigator-workflows-service-dev.tar --name navigator
kubectl --context kind-navigator --namespace navigator delete pod -l app=workflows-service
```

The pod restarts on the fresh image and the walk resumes; re-run the failed `intake answer` step.

## Cleaning up

Stop `web` with `Ctrl-C` in its terminal. Leave the KIND cluster running — it is the persistent fixture the next session
reuses. `cargo run --release -p cli -- down` deletes the cluster entirely; reserve it for a deliberate clean rebuild,
not routine cleanup.
