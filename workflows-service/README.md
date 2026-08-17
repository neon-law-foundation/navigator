# Workflow service

`workflows-service` is Navigator's long-running Restate worker. It receives durable invocations for Notations, archives,
billing checks, and other registered workflows, and records their auditable state transitions.

It serves Restate and deployment operators rather than end users directly. One shared worker is necessary so durable
execution, retries, and journaling remain centralized; adding a workflow does not add another always-on service.

Cloud worker registration is an explicit operator action and is not implied by deploying a new image. See [durable
workflows](../docs/durable-workflows.md), [scheduled jobs](../docs/cronjobs.md), and
[`workflows`](../workflows/README.md).
