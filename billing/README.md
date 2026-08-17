# Billing

`billing` is Navigator's provider-neutral interface for accounting actions such as resolving contacts and raising matter
invoices. It includes the production Xero adapter and a deterministic stub for local development and tests.

It serves web and workflow code that needs billing behavior without coupling the matter lifecycle to one vendor or one
runtime. The crate deliberately excludes the Restate SDK so both synchronous application code and durable worker code
can share the same contract.

Amounts are represented in cents, never floating-point values. See [Xero billing](../docs/xero-billing.md), [third-party
integrations](../docs/third-party-integrations.md), and [`billing-workflows`](../billing-workflows/README.md).
