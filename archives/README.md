# Archives

`archives` is the durable export service for Navigator's operational data. It snapshots SurrealDB tables to Parquet,
records cloud-cost data when configured, and reports each scheduled run to firm operators.

It serves deployment operators and analysts who need recoverable, queryable history without placing reporting work on
the application database. Restate hosts the workflow in `workflows-service`, so retries and partial failures do not
require a separate long-running archive service.

See [Iceberg archive](../docs/iceberg-archive.md) for the data contract, [scheduled jobs](../docs/cronjobs.md) for the
runtime shape, and the crate source for implementation details.
