# Deployment examples

`examples/deploy` contains the reference Kubernetes and cloud scaffolding for running Navigator outside the development
cluster. The manifests show the production-shaped topology while leaving installation-specific projects, domains,
secrets, and image tags as operator-owned inputs.

It serves deployment operators and licensed adopters who need a concrete starting point without making one cloud
configuration part of the application runtime. These files are examples, not a second deployment system; the `navigator`
CLI remains the control plane.

See [OSS installation](../../docs/oss-install.md), [GKE production](../../docs/gke-prod.md), and [cloud
operations](../../docs/cloud-operations.md) before applying an overlay.
