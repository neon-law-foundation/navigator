# Observability overlay

This overlay connects Navigator workloads to the OpenTelemetry collector and the reference GCP telemetry sinks. It
defines the production shape for sampled diagnostics, searchable logs, long-term telemetry export, retention, and
self-monitoring.

It serves deployment and incident-response operators. The overlay is necessary because legal-service telemetry must be
useful without carrying client content: redaction fails closed, access is controlled as a privilege boundary, and the
unsampled archive remains distinct from operational views.

Treat this directory as operator-applied scaffolding, not an application default. The authoritative privacy,
configuration, and recovery requirements are in [observability](../../../../docs/observability.md) and [cloud
operations](../../../../docs/cloud-operations.md).
