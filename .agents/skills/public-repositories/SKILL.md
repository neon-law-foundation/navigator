---
name: public-repositories
description: Check or configure Navigator's public-repository governance posture.
---

# Public repository posture

Read [`docs/licensing.md`](../../../docs/licensing.md), [`docs/gitops.md`](../../../docs/gitops.md), and
[`docs/public-contributor-safety.md`](../../../docs/public-contributor-safety.md) before changing repository settings.

- Keep the licence, notice, contribution policy, and merge rules consistent with this repository's canonical files.
- Read the live repository state before proposing any mutation; make no production or irreversible change without
  explicit user authorization.
- A public source repository is for code and synthetic or firm-owned examples only. Do not add client data, legal
  files, real contact details, production identifiers, or operational maps.
- A **publish workflow in a public repository** grants `id-token: write` and writes its coordinates into a public
  Actions log. Pass the deployment's applications bucket, publisher service account, and Workload Identity provider as
  repository **secrets**, never variables: GitHub redacts a secret's exact text and does not redact a variable, and two
  of those three carry the deployment's GCP project identity in their own text. The Workload Identity binding on
  Google's side remains the access control — secrets are disclosure reduction and must not be mistaken for the boundary.
  See [`docs/project-repositories.md`](../../../docs/project-repositories.md).
- A Project's publisher is confined to **one** `<code>/portal` prefix by an IAM condition, and its service account is
  derived from the deployment's GCP project id, so one deployment carries one Project's portal publisher. Provisioning a
  second Project against the same publisher is refused rather than repointed — repointing would revoke the first
  Project's publish silently. Give the second Project its own publisher identity.
