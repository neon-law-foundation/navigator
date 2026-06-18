---
name: gcp-spend
description: >
  Report daily or arbitrary-window GCP spend by querying the BigQuery Cloud Billing export — gross cost, the credits
  applied, and therefore real net cost per day. Discovers the project from `.env` (`NAVIGATOR_GCP_PROJECT_ID`) and the
  billing table from `bq ls`, so nothing GCP-generated is hard-coded. Trigger when the user asks "what's my GCP spend",
  "daily cloud cost", "how much are we spending on GCP", "show the billing", or wants spend broken down by service or
  SKU. Use the billing export, never rate-card estimates — it is actual invoiced cost, with roughly a 24-hour lag so the
  current day is always partial.
---

# gcp-spend

Answer "what are we spending on GCP?" from the BigQuery Cloud Billing export — the only source that reflects what you
are actually charged. Console estimates and rate-card math drift from the invoice; the export does not.

## Why the export, not an estimate

Credits (free-tier, committed-use, promotional) live in a nested `credits` array and are negative. Always sum them, or
you overstate spend. NeonLaw's days currently run about $9 gross, fully offset by credits to roughly $0 net — reporting
only gross would be misleading. The export lands with roughly a 24-hour lag, so the most recent day is always partial.

See Google's reference: <https://cloud.google.com/billing/docs/how-to/export-data-bigquery>.

## Prerequisites

`.env` at the repo root gives `NAVIGATOR_GCP_PROJECT_ID` (and optionally `NAVIGATOR_GCP_LOCATION`). The `bq` CLI (part
of the gcloud SDK) must be authenticated against that project. These run on the user's machine — propose the commands
and let the user prefix with `!` to run them in-session.

Per the workspace rule, nothing here is hard-coded: the project comes from `.env`, and the billing table — whose name
GCP auto-generates per billing account — is discovered, never pinned in this skill.

## Step 1 — discover the billing table

The export conventionally lands in a dataset named `billing_export`; list datasets first if a fork named it differently.

```bash
set -a; source .env; set +a
: "${NAVIGATOR_GCP_PROJECT_ID:?set NAVIGATOR_GCP_PROJECT_ID in .env}"

bq ls "${NAVIGATOR_GCP_PROJECT_ID}:"                  # datasets, if billing_export isn't the name
bq ls "${NAVIGATOR_GCP_PROJECT_ID}:billing_export"    # tables — grab the gcp_billing_export_* one
```

Two export flavors exist; either gives daily totals. `gcp_billing_export_v1_*` is standard usage cost;
`gcp_billing_export_resource_v1_*` is the same plus per-resource detail (larger). Capture the table id into
`$BILLING_TABLE` for the queries below.

## Step 2 — daily spend (gross / credits / net)

```bash
BILLING_TABLE="billing_export.<table-id-from-step-1>"

bq query --use_legacy_sql=false --format=pretty "
SELECT
  DATE(usage_start_time) AS day,
  ROUND(SUM(cost), 2) AS gross_cost,
  ROUND(SUM(IFNULL((SELECT SUM(c.amount) FROM UNNEST(credits) c), 0)), 2) AS credits,
  ROUND(SUM(cost)
        + SUM(IFNULL((SELECT SUM(c.amount) FROM UNNEST(credits) c), 0)), 2) AS net_cost,
  ANY_VALUE(currency) AS currency
FROM \`${NAVIGATOR_GCP_PROJECT_ID}.${BILLING_TABLE}\`
WHERE DATE(usage_start_time) >= DATE_SUB(CURRENT_DATE(), INTERVAL 14 DAY)
GROUP BY day
ORDER BY day DESC
"
```

Credits come back negative (they reduce cost); `net_cost = gross_cost + credits`. The most recent day is partial, so
flag the ~24h lag when you report it.

## Step 3 — optional breakdown by service

```bash
bq query --use_legacy_sql=false --format=pretty "
SELECT
  service.description AS service,
  ROUND(SUM(cost), 2) AS gross_cost,
  ROUND(SUM(cost)
        + SUM(IFNULL((SELECT SUM(c.amount) FROM UNNEST(credits) c), 0)), 2) AS net_cost
FROM \`${NAVIGATOR_GCP_PROJECT_ID}.${BILLING_TABLE}\`
WHERE DATE(usage_start_time) >= DATE_SUB(CURRENT_DATE(), INTERVAL 30 DAY)
GROUP BY service
ORDER BY gross_cost DESC
LIMIT 20
"
```

Swap `service.description` for `sku.description` to get SKU-level detail.

## Reporting

Always show net alongside gross — net is the number that hits the invoice. Flag the current day as partial. If gross is
fully offset by credits, say so plainly. Prefer this skill over any estimate; if the export is unreachable, say that
rather than guessing from rate cards.
