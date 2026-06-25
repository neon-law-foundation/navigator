# Observability overlay — OTel Collector + BigQuery sink

The deploy-side half of [`docs/observability.md`](../../../../docs/observability.md). Telemetry leaves the firm's trust
boundary, so the rule is structural: **identifiers and counts, never content** — and the Collector here is the choke
point that enforces it. The enforcement is not aspirational: a **fail-closed redaction processor** (see below) deletes
every attribute whose key is not on an explicit operational allow-list.

The Collector receives all three signals (traces, metrics, **and logs** — `telemetry` exports logs over OTLP now, while
still dual-emitting to stdout) and fans them out to Cloud Trace / Cloud Monitoring / Cloud Logging.

## 1. Wiring the binaries to the Collector

Point every binary at the Collector with one env var, supplied from the shared `navigator-otel-env` ConfigMap (defined
in `otel-collector.yaml`) that `navigator-web`, `workflows-service`, and every `*-trigger` CronJob `envFrom` — one
source of truth for the URL instead of eight copies:

```text
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector.navigator.svc.cluster.local:4317
```

When that variable is unset (KIND, CI, OSS forks) the binaries emit nothing over OTLP and only log to stdout — zero
cost, no network. Setting it flips on JSON stdout + OTLP export with no code change.

Grant the Collector's identity permission to write all three signals:

```bash
gcloud iam service-accounts create navigator-otel --project YOUR_PROJECT_ID
for ROLE in roles/cloudtrace.agent roles/monitoring.metricWriter roles/logging.logWriter; do
  gcloud projects add-iam-policy-binding YOUR_PROJECT_ID \
    --member="serviceAccount:navigator-otel@YOUR_PROJECT_ID.iam.gserviceaccount.com" \
    --role="$ROLE"
done
# Workload Identity binding for the in-cluster ServiceAccount:
gcloud iam service-accounts add-iam-policy-binding \
  navigator-otel@YOUR_PROJECT_ID.iam.gserviceaccount.com \
  --role roles/iam.workloadIdentityUser \
  --member "serviceAccount:YOUR_PROJECT_ID.svc.id.goog[navigator/otel-collector]"

# Validate the Collector config before applying (catches a bad processor or
# tail-sampling policy type without touching the cluster):
docker run --rm -v "$PWD/otel-collector.yaml:/m.yaml" --entrypoint sh \
  otel/opentelemetry-collector-contrib:0.116.1 -c \
  'sed -n "/config.yaml: |/,/^---/p" /m.yaml | sed "1d;s/^    //" > /tmp/c.yaml && \
   /otelcol-contrib validate --config=/tmp/c.yaml'

kubectl apply -f otel-collector.yaml
```

`roles/logging.logWriter` is the new grant — it is what lets the Collector's `googlecloud` exporter write the OTLP log
stream into Cloud Logging.

## Fail-closed redaction — the privilege control (LEGAL)

`roles` and IAM keep the *destination* private; the **redaction processor** keeps *client content* out of telemetry in
the first place. It runs on the traces, metrics, and logs pipelines with `allow_all_keys: false`, so any attribute whose
key is not on the allow-list is **deleted** — names, emails, addresses, free-text answers, resolved URL paths
(`url.path`), SQL (`db.statement`), and exception payloads (`exception.message` / `exception.stacktrace`) are dropped by
construction, because none of them are on the list. The allow-list is the whole control.

The v1 allow-list (attorney call, 2026-06-14) permits opaque UUID ids (`person_id`, `project_id`, `notation_id`, …) —
they carry no name or email and are what make a failed workflow traceable to its matter — plus operational keys
(`service`, `outcome`, `handler`, `step`, `error.class`, `status_code`, route **templates**, counts, durations) and the
`resourcedetection` cloud/host/k8s keys. To add a key, list it in the `redaction.allowed_keys` block.

**Deferred to Step 5 (the 10-year log lake):** value-level regex backstops (email / phone / SSN / EIN) and the
log-**body** scrub. The key allow-list does not cover the free-text log body, so the body control today is the rule in
`telemetry/src/lib.rs` (structured fields only, never interpolate client content into a message). The regex/body scrub
is a **prerequisite** for turning on the content-free 10-year `iceberg/otel_logs/` store.

Tail sampling (traces only) keeps every `status=ERROR` and every `audit=true` trace and probabilistically samples the
rest, capping Cloud Trace ingestion cost. The live Cloud Trace view is therefore the sampled, lossy stream; the Iceberg
lake (Step 5) gets the full unsampled stream and is the integrity source.

## 2. Logs → BigQuery (the "everything in BQ" surface)

Logs **dual-path**: alongside the OTLP stream above, each binary still writes **structured JSON** to stdout, which Cloud
Logging already collects. This stdout path is the failure-isolation guarantee — if the Collector is down, Cloud Logging
still gets every line, so an outage degrades to "no live traces / no lake telemetry," never "lost a log line." A Logging
sink lands every line — including the `navigator.workflow.trigger.fired` outcome events with their `service` / `status`
/ trace ids — in a BigQuery dataset you can query:

```bash
# Dataset to receive telemetry logs.
bq --location=us-west4 mk --dataset YOUR_PROJECT_ID:navigator_telemetry

# Sink: route Neon Law Navigator *application* pod logs to that dataset. A BigQuery
# logging sink infers the table schema from the FIRST entry per field and pins
# it for the day's date-sharded table (every JSON number becomes FLOAT); a later
# entry that sends the same field as a non-numeric string is rejected with
# `table_invalid_schema` ("Cannot convert value to floating point"). Two things
# keep the schema stable, and BOTH matter:
#   1. Our own log fields must be type-stable across every call site. The
#      `status` field is logged as a numeric `.as_u16()` everywhere (see
#      `workflows/src/trigger.rs`, `web::oauth`, `web::idp_admin`) — never the
#      `%resp.status()` Display string "400 Bad Request", which is exactly the
#      collision that first broke this sink.
#   2. The filter is scoped to the containers that share `telemetry::init`'s one
#      JSON schema (web, worker, git, the *-trigger jobs, statutes-sync) and
#      EXCLUDES the non-app sidecars (envoy, cloud-sql-proxy, opa), whose
#      differently-shaped JSON would otherwise collide too.
# See "Recovering from a schema-mismatch error" below.
gcloud logging sinks create navigator-telemetry-bq \
  bigquery.googleapis.com/projects/YOUR_PROJECT_ID/datasets/navigator_telemetry \
  --project YOUR_PROJECT_ID \
  --log-filter='resource.type="k8s_container"
    AND resource.labels.namespace_name="navigator"
    AND NOT resource.labels.container_name=("envoy" OR "cloud-sql-proxy" OR "opa")'

# Grant the sink's writer identity permission to write to the dataset
# (the create command prints the writerIdentity; bind it as a Data Editor).
WRITER=$(gcloud logging sinks describe navigator-telemetry-bq \
  --project YOUR_PROJECT_ID --format='value(writerIdentity)')
bq add-iam-policy-binding \
  --member="$WRITER" --role=roles/bigquery.dataEditor \
  YOUR_PROJECT_ID:navigator_telemetry
```

Example query — every trigger outcome in the last day, by service:

```sql
SELECT
  jsonPayload.service AS service,
  jsonPayload.fields.outcome AS outcome,
  COUNT(*) AS n
FROM `YOUR_PROJECT_ID.navigator_telemetry.*`
WHERE jsonPayload.message = "workflow trigger accepted"
   OR jsonPayload.message LIKE "workflow trigger %"
GROUP BY service, outcome
ORDER BY service;
```

A `service` that should fire on a schedule but shows no rows is a silently-stopped trigger — the failure this whole
overlay exists to make visible. Pair it with `navigator doctor` for the in-cluster view.

### Recovering from a schema-mismatch error (`table_invalid_schema`)

If Cloud Logging emails that the sink "had errors while routing logs" with `Error Code table_invalid_schema` and a
detail like `Cannot convert value to floating point (bad value)`, a field arrived with a type that does not match the
column BigQuery already inferred for it. The first entry BigQuery received that day pinned the column type (every JSON
number becomes `FLOAT`); a later entry sent the same field as a non-numeric string (or an array), so BigQuery rejects
those rows. The dropped rows — never the whole table — are described in the auto-created `export_errors_*` diagnostic
table.

Remediate in three steps (all on the deployer's machine — see the project guide on running cloud commands):

1. **Identify the offending field and producer.** Read the diagnostic table for the affected day:

   ```bash
   bq query --use_legacy_sql=false --project_id=YOUR_PROJECT_ID \
     'SELECT _TABLE_SUFFIX AS day, * FROM `YOUR_PROJECT_ID.navigator_telemetry.export_errors_*`
      ORDER BY _TABLE_SUFFIX DESC LIMIT 50'
   ```

   Read the embedded `logEntry` and `schemaErrorDetail`: they name the `container_name`, the field, and the value that
   would not convert. The original incident showed the `web` container logging `fields.status` as the Display string
   `"400 Bad Request"`, while `workflows/src/trigger.rs` logged the same field as a numeric `.as_u16()` — so the column
   was pinned `FLOAT` and the string rows were rejected.

2. **Fix the source.** There are two independent causes; fix whichever the diagnostic shows (it can be both):

   - **Our own field logged with two types** (the cause of the original incident). Make the field one type at every
     `tracing::` call — for status codes that means the numeric `.as_u16()`, never the `%resp.status()` Display string.
     This is a code fix that ships with the next image; identifiers and counts, never content (see
     [`docs/observability.md`](../../../../docs/observability.md)).
   - **A non-application sidecar** (`envoy`, `cloud-sql-proxy`, `opa`) whose JSON shares a field name with ours. Narrow
     the live sink filter to the app containers (an existing deploy created before this change still has the broad
     filter):

     ```bash
     gcloud logging sinks update navigator-telemetry-bq \
       --project YOUR_PROJECT_ID \
       --log-filter='resource.type="k8s_container"
         AND resource.labels.namespace_name="navigator"
         AND NOT resource.labels.container_name=("envoy" OR "cloud-sql-proxy" OR "opa")'
     ```

3. **Let Logging recreate the table.** Per Google's guidance, after the source is fixed, rename the conflicting
   date-sharded table so Logging recreates it with the corrected schema (rename, don't delete, until you've confirmed
   recovery — the old rows stay queryable under the new name):

   ```bash
   DAY=$(date -u +%Y%m%d)   # or the YYYYMMDD reported in the error
   bq cp -f YOUR_PROJECT_ID:navigator_telemetry.navigator_telemetry_$DAY \
     YOUR_PROJECT_ID:navigator_telemetry.navigator_telemetry_${DAY}_preschemafix
   bq rm -f -t YOUR_PROJECT_ID:navigator_telemetry.navigator_telemetry_$DAY
   ```

   The next log entry recreates `navigator_telemetry_$DAY` from the now type-stable stream. Confirm with the example
   query above; the `export_errors_*` table should stop growing.

If the sink is genuinely no longer wanted, delete it instead — but that gives up the "everything in BigQuery" log
surface, so prefer the fix above:

```bash
gcloud logging sinks delete navigator-telemetry-bq --project YOUR_PROJECT_ID
```

## 3. Telemetry Iceberg lake — the unsampled stream (operator-applied)

The full, **unsampled** telemetry stream goes to a cheap GCS lake; only the **sampled** stream pays Cloud Trace
ingestion. That split is the key cost lever. The nightly `Archives` workflow's `iceberg_telemetry` step already authors
Iceberg metadata over `iceberg/otel_{logs,traces,metrics}/data/dt=<date>/*.parquet` (reusing the entity-table writer,
`archives::author_iceberg_for_prefix`) — it is a clean **no-op until those Parquet files exist**.

Two pieces are operator-applied (and **must pass `otelcol validate` before `kubectl apply`**, since a bad component
crashes the collector — `roles/storage.objectCreator` on the exports bucket is also required):

1. **Trace split** — a second traces pipeline that skips `tail_sampling` and exports the full stream to the lake, while
   the existing pipeline keeps `tail_sampling` → Cloud Trace:

   ```yaml
   service:
     pipelines:
       traces:                 # sampled → Cloud Trace (cost-bounded)
         receivers: [otlp]
         processors: [memory_limiter, resourcedetection, redaction, tail_sampling, batch]
         exporters: [googlecloud]
       traces/lake:            # full unsampled → GCS lake
         receivers: [otlp]
         processors: [memory_limiter, resourcedetection, redaction, batch]
         exporters: [<gcs-object-sink>]
   ```

   The `logs` and `metrics` pipelines (already unsampled) just add `<gcs-object-sink>` as a second exporter.

2. **An OTLP→Parquet bridge.** The collector has **no Parquet marshaler** — the contrib `googlecloudstorage`-style
   sinks write OTLP protobuf/JSON, not Parquet, and the nightly step needs Parquet (it reads the schema + row counts
   from the footer). So the lake sink writes OTLP blobs and a small operator-provided conversion lands them as
   `iceberg/otel_*/data/dt=<date>/*.parquet` with fixed logs/traces/metrics schemas. Until that bridge exists, the
   `iceberg_telemetry` step reports `(no data)` and the lake is empty — the rest of the overlay is unaffected. This is
   the one piece the brief assumed the collector could do natively; it can't, so it is called out here rather than
   silently skipped.

## 4. Split retention (decided 2026-06-14)

Retention is split by signal, applied as a prefix-scoped GCS Object Lifecycle on the exports bucket
([`exports-bucket-lifecycle.json`](exports-bucket-lifecycle.json)):

| Prefix | Lifecycle | Iceberg snapshot log |
| --- | --- | --- |
| `iceberg/otel_logs/` | Coldline at 365d, **delete at 3650d (10y)** | full log (kept) |
| `iceberg/otel_traces/`, `iceberg/otel_metrics/` | **delete at 30d** | pruned to 30d (snapshot-expiry) |
| `iceberg/audit_events/` | **no rule here** — own 10y governance | (separate) |
| `iceberg/<entity-table>/` | (unchanged) | full log (kept) |

```bash
gcloud storage buckets update gs://YOUR_PROJECT_ID-exports \
  --lifecycle-file=exports-bucket-lifecycle.json
```

The 10-year `otel_logs` retention is safe **only because the logs are content-free** — the fail-closed redaction
allow-list (§ above) is the enforcing control, so a decade-long log store stays an internal operational record, not a
discoverable client-privilege surface. Content-free ⇒ no preservation duty ⇒ scheduled auto-expiry is not spoliation; no
legal-hold mechanism is needed for logs/telemetry (it remains relevant only to matter-file + audit data).

GCS lifecycle is the actual deleter. On the 30-day tables the nightly `iceberg_telemetry` step additionally runs
**Iceberg snapshot-expiry** (`SnapshotInput::expire_before_ms`), dropping snapshots older than 30d from the log so the
metadata never references data files lifecycle has already deleted — a deliberate divergence from the entity tables and
`otel_logs`, which keep their full snapshot log.

## 5. Self-monitoring — don't let observability be a silent SPOF

Two independent signals catch a collector that has died or is silently shedding telemetry:

1. **GMP drop/failure alerts** ([`collector-monitoring.yaml`](collector-monitoring.yaml)). The collector exposes its own
   metrics on `:8888` (`service.telemetry.metrics.address: 0.0.0.0:8888`); a `PodMonitoring` scrapes them via the
   already-running Google Managed Prometheus, and a `Rules` resource alerts when `otelcol_exporter_send_failed_*` or
   `otelcol_processor_dropped_*` is nonzero for 10 minutes — the "telemetry is silently being lost" signal. (Validate
   the CRD versions against your cluster before applying; a bad rule is rejected by the API server and can't crash the
   collector.)

2. **Heartbeat collector-reachability line.** The six-hourly `Heartbeat` email (`workflows-service::heartbeat`) adds a
   best-effort TCP reachability probe of the collector. It is **non-fatal** — the heartbeat's core claim (the durable
   engine is alive) depends on nothing, so an unreachable collector is reported as an operational note, never a failed
   beat.

Because logs **dual-path** to stdout→Cloud Logging, total collector loss degrades to "no live traces + no lake
telemetry," never "lost an audit record" — so these alerts are about restoring fidelity, not a data-loss incident.

## How this relates to the Iceberg archive

This overlay is the *operational* telemetry (logs, traces, metrics). The nightly `Archives` workflow
([`docs/iceberg-archive.md`](../../../../docs/iceberg-archive.md)) is the *data* archive — Postgres tables snapshotted
to Parquet on GCS, now promoted to Iceberg tables (`archives::iceberg`). Both query from BigQuery; they answer different
questions.
