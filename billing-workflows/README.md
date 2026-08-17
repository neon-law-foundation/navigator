# Billing workflows

`billing-workflows` contains the Restate-managed billing jobs that call the provider-neutral `billing` crate. Today it
hosts the scheduled Xero canary and is the home for durable matter-billing orchestration.

It serves workflow maintainers and operators who need billing actions to survive retries and worker restarts. Keeping
this orchestration separate lets `web` use the billing provider contract without depending on the Restate SDK; the
shared worker hosts it rather than adding another service.

See [Xero billing](../docs/xero-billing.md) and [`billing`](../billing/README.md).
