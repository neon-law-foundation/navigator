# Workflows

`workflows` defines Navigator's durable workflow specifications and outbound runtime interface. Application code uses it
to validate and submit work either to an in-memory test runtime or to Restate over HTTP.

It serves web handlers, tests, and workflow authors who need one representation of a legal-service process. Separating
submission from the inbound worker keeps the application free of the Restate SDK while preserving the same state machine
across local proof and production execution.

Workflow composition lives with the relevant Notation in `templates`; execution lives in
[`workflows-service`](../workflows-service/README.md). See [durable workflows](../docs/durable-workflows.md) and
[Notation authoring](../docs/notation-authoring.md).
