# Cloud

`cloud` defines Navigator's provider-neutral object-storage boundary and the small redirect service used by public
deployments. Its `StorageService` implementations support local filesystems, Google Cloud Storage, and S3-compatible
systems such as Garage.

It serves application crates that need documents or public assets without importing a cloud vendor SDK. This boundary
keeps local KIND, the reference GCP deployment, and white-label installations on the same storage contract while failing
closed when no backend is configured.

Project git repositories are intentionally handled by [`repos`](../repos/README.md), not object storage. See [cloud
operations](../docs/cloud-operations.md), [deployment environments](../docs/environments.md), and [project
repositories](../docs/project-repositories.md).
