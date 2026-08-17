# Container images

`images` contains every OCI Containerfile Navigator ships: the web and workflow services, scheduled workflow triggers,
the git-serving tier, and the public redirect service.

It serves release engineers and deployment operators who build one reviewed source tree into the artifacts used by KIND
and cloud environments. Keeping the definitions together makes image ownership visible and prevents each workflow from
growing an unnecessary service or bespoke build path.

All build contexts are the repository root, and the `navigator` CLI owns normal image and rollout operations. See
[GitOps](../docs/gitops.md), [cloud operations](../docs/cloud-operations.md), and the files in this directory for exact
targets.
