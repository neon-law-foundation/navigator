---
name: cloud-rest-endpoints
description: >
  How `navigator gcp setup` talks to GCP REST APIs — service base URLs, idempotency convention, LRO polling, dry-run,
  and the recipe for refreshing or adding endpoints. Trigger when editing anything under `cli/src/devx/gcp/`, adding a
  new GCP service to the pipeline, debugging a 4xx/5xx from `navigator gcp setup`, or before bumping the
  `google-cloud-auth` / `google-cloud-storage` dependencies. Also trigger when an existing endpoint URL looks suspicious
  or returns 404 in production — Google occasionally retires v1 paths.
---

# Maintaining the GCP REST endpoints in `navigator gcp setup`

`navigator gcp setup` provisions GCP infrastructure (VPC, Cloud SQL, two
GCS buckets, Cloud Run) by **calling Google's REST APIs directly**
via `reqwest`. There is no `gcloud` shell-out and no Google SDK
wrapper crate — every URL, method, and request body shape lives
in `cli/src/devx/gcp/`. That's deliberate: it gives us a clean
intercept point for `--dry-run` and for wiremock tests, neither of
which we'd get from shelling out. The tradeoff is that **we own
the API contract** and have to keep it correct.

## The four things you must keep correct

1. **Base URLs** in `GcpService::default_base_url()`
   (`cli/src/devx/gcp/client.rs`). One per `*.googleapis.com` host.
2. **Per-endpoint paths** inside each step's module
   (`services.rs`, `network.rs`, `sql.rs`, `buckets.rs`, `run.rs`).
3. **Request body shapes** — the `serde_json::json!({...})` literals
   in each step.
4. **LRO status-path templates** passed to `lro::wait` — the
   second-to-last argument like `"/v1/{name}"` or
   `"/compute/v1/{name}"`. Wrong template → polling 404s.

If any of these drift from what Google ships, the wiremock tests
will keep passing (because they mock what *we* declare, not what
Google does), and the failure mode is a real-GCP 4xx/5xx that
nobody catches until Layer 2 of CI runs. Read on for how to
prevent that.

## Conventions every step follows

- **Always POST; treat 409 Conflict as success.** That's our
  idempotency model. Don't add a GET-then-POST dance — the user
  has explicitly asked for the simpler one.
- **LROs**: a 2xx response body with `name` and (often) `done` is a
  long-running operation. Call `lro::wait(...)` with the right
  status-path template for the service. Skip the wait on 409.
- **Dry-run is free.** Every call goes through `GcpClient::get` or
  `post_json`, which short-circuit in `Mode::DryRun`. You don't
  need to add per-step dry-run logic — just call the client and
  the framework handles it.
- **No secrets in dry-run output that the operator must save.**
  E.g. `sql.rs` skips the "save this password" `eprintln!` in
  dry-run because the password in the request body is never
  actually applied to a real user.

## Recipe: refresh an endpoint that has drifted

**Symptom:** Layer 2 (real-GCP) smoke test fails with a 404 / 400
that the wiremock tests don't reproduce.

1. **Find the canonical Google docs page** for the operation. Pin
   the documentation source — secondary blogs lie:
   - <https://cloud.google.com/service-usage/docs/reference/rest>
   - <https://cloud.google.com/compute/docs/reference/rest/v1>
   - <https://cloud.google.com/sql/docs/postgres/admin-api/rest>
   - <https://cloud.google.com/storage/docs/json_api/v1>
   - <https://cloud.google.com/run/docs/reference/rest>
2. **Compare against the code.** The path in the docs becomes the
   second argument to `client.post_json(GcpService::X, "/...", ...)`.
   The request body becomes the third argument.
3. **Update the wiremock test in the same module first** to match
   what Google now says, then update the implementation. The test
   should fail before your fix and pass after — that's how you
   know the test isn't tautological.
4. **Run `cargo run -p cli -- gcp setup --project-id <PROJECT> --dry-run`**
   and eyeball the printed call. If the URL or body looks wrong,
   fix it before any real-GCP attempt.
5. **Run the smoke test locally** if you have a test project:
   `cargo run -p cli --release -- gcp setup --project-id <PROJECT>`.
   If 409 fires because a prior run left resources behind, run the
   same teardown commands the CI workflow uses.

## Recipe: add a new GCP service to the pipeline

Example: you want to provision a Pub/Sub topic.

1. **Add the host** to `GcpService` in `client.rs`:

   ```rust
   pub enum GcpService {
       // ...
       PubSub,
   }
   ```

   …and its real base URL in `default_base_url()`:
   `"https://pubsub.googleapis.com"`.
2. **Add the service ID** to `services::REQUIRED_SERVICES` so
   `batchEnable` turns the API on first:
   `"pubsub.googleapis.com"`.
3. **Create a new module** `cli/src/devx/gcp/pubsub.rs` following
   the existing pattern:
   - `pub const TOPIC_NAME: &str = "navigator-...";`
   - `pub async fn ensure_topic(client, project_id) -> Result<()>`
     that POSTs to `/v1/projects/{project}/topics/{TOPIC_NAME}`
     and treats 409 as success.
   - Two wiremock tests minimum: happy path + idempotent-on-409.
   - One dry-run test asserting the recorded call.
4. **Wire it into `gcp::run`** (`cli/src/devx/gcp/mod.rs`) in the right
   order — after `services::enable_services`, before whatever
   consumes it.
5. **Update the end-to-end dry-run test** in `gcp/mod.rs` to
   bump the expected call count and assert the new step's URL.
6. **Update `cli/README.md`** — the pipeline list.

If you're tempted to skip any of (1)–(6), the answer is no.
Each step is load-bearing: (1) without the base URL the test can't
point at wiremock; (2) without enabling the API the call 403s on a
fresh project; (5) the end-to-end test is the only thing that
catches "step added but never called."

## Bumping `google-cloud-auth`

The ADC plumbing in `cli/src/devx/gcp/auth.rs` uses
`google_cloud_auth::token::DefaultTokenSourceProvider`. The API has
shifted twice already; the previous `create_token_source(...)`
helper is deprecated. When you bump the crate version:

1. Check that `DefaultTokenSourceProvider::new(Config)` still
   exists and still returns a type implementing
   `google_cloud_token::TokenSourceProvider`.
2. Check that `TokenSource::token()` still returns
   `Ok("Bearer <token>")` (with the prefix). If they drop the
   prefix, remove the `strip_prefix("Bearer ")` in `AdcToken::token`.
3. The `google-cloud-token` crate version is pinned separately —
   it's the trait crate, and `google-cloud-auth`'s minor bumps
   can move it. Make sure they stay compatible.

## Things NOT to do

- **Don't add per-call retry logic** in the step modules. The
  framework guarantees we re-run the whole `setup` invocation
  cleanly via the 409 idempotency rule; one retry layer is enough.
- **Don't add a `gcloud` fallback** "just in case the API call
  fails." The error from `reqwest` already includes the URL and
  status; that's the diagnostic. A silent fallback hides the bug.
- **Don't introduce a `google-cloud-compute` or
  `google-cloud-run` SDK crate** for one or two calls. The whole
  point of raw REST is the small surface area; pulling in a 50k-
  line wrapper for `networks.insert` is a bad trade.
- **Don't move base URLs into env vars.** Tests need to override
  them per-service, which is exactly what `with_base_url(...)`
  does. An env var would be a global override and break
  parallelizable tests.

## Where to look when you're stuck

- The pipeline order and step descriptions are in
  `cli/src/devx/gcp/mod.rs` (module docs).
- The dry-run framework is in `cli/src/devx/gcp/client.rs` — start
  with `Mode`, `RecordedCall`, and `record_and_synthesize`.
- The LRO contract is in `cli/src/devx/gcp/lro.rs`.
- See [[rust-best-practices]] for workspace-wide conventions on
  error handling, unwraps, and clippy.
- See [[postgres-in-kind]] for how local Postgres mirrors the
  Cloud SQL setup `navigator gcp setup` provisions.
