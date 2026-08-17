# Navigator CLI

The `cli` crate builds the `navigator` command, Navigator's control plane for every machine-bound workflow. It validates
and renders Notations, manages local KIND environments, operates deployments, maintains assets and forms, and drives
authorized matter work against a live site.

It serves developers, deployment operators, and authorized firm lawyers who need one auditable interface instead of
independent scripts or manual infrastructure steps. Centralizing those flows in Rust keeps validation, environment
selection, safety checks, and production boundaries consistent with the application.

The local staging lifecycle (`navigator dev staging`) runs under the `NAVIGATOR_ENVIRONMENT=dev` application profile;
staging is a lifecycle target, not an application environment.

Run `navigator --help` for the current command surface. Use [AGENTS.md](../AGENTS.md) for the local development loop,
[agent workflows](../docs/agent-workflows.md) for repository work, and [cloud operations](../docs/cloud-operations.md)
for deployment procedures.
