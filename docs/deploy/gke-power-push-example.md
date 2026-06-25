# GKE power-push — the roll-only model

This page used to document a local-build → Artifact Registry → GCS-bundle ship. That flow is **retired**. Images are no
longer built on a laptop and there is no Artifact Registry: CI (`.github/workflows/deploy.yml`) builds every image and
publishes it to the **public** `ghcr.io/neon-law-foundation/navigator-*` packages, tagged `YY.MM.DD` (the release date)
plus `latest`. `power-push` only **rolls the cluster** onto an already-published image.

The canonical, maintained walk-through is [`cloud-operations.md`](../cloud-operations.md). Read it for the full recipe
(resolving the tag, the Secret-invariant check, the concurrent rollout, the Restate re-registration, and the
secret-rotation "no-rebuild push"). This page is only the short orientation.

## The new model in one breath

- **CI publishes; nothing builds locally.** A PR merges to `main`; the daily tag flow (`release-tag.yml`) cuts a
  `YY.MM.DD` tag; the tag push triggers `deploy.yml`, which runs the KIND integration suite and publishes both
  `ghcr.io/neon-law-foundation/navigator-web:YY.MM.DD` and
  `ghcr.io/neon-law-foundation/navigator-workflows-service:YY.MM.DD` (plus `latest`).
- **`power-push` rolls, it does not build.** It resolves the latest published `YY.MM.DD` ghcr tag (or takes
  `--tag YY.MM.DD`, with an optional `.HH` suffix — e.g. `26.06.25.14` — for an ad-hoc same-day release), confirms the
  prod Secret satisfies the new binary's boot invariants, pins **both** the `navigator-web` and `workflows-service`
  deployments to that **one** tag, rolls them out together, and re-registers the worker with Restate. It builds no
  images, pushes nothing to a registry, and archives no git bundle.
- **The cluster pulls anonymously.** The ghcr packages are public, so the GKE nodes need no imagePullSecret and there is
  no registry credential to rotate.
- **Always both binaries, one tag.** `navigator-web` and `workflows-service` share a Secret and a workflow contract;
  rolling one alone invites version skew. Never roll just one, never pin them to different dates.
- **Verify the roll with `/version`.** `GET https://www.<your-domain>/version` reports the running build; the headline
  `release` field is the `YY.MM.DD` ghcr tag now live.

## The fast path

```bash
# Roll the cluster onto the latest published YY.MM.DD image (both deployments, together).
doppler run --project navigator --config prd -- cargo run --release -p cli -- power-push

# Roll onto a specific published tag instead of the latest.
doppler run --project navigator --config prd -- cargo run --release -p cli -- power-push --tag 26.06.23

# Print every command, run nothing.
doppler run --project navigator --config prd -- cargo run --release -p cli -- power-push --dry-run

# No-rebuild push: restart both deployments so they re-read a rotated Secret value.
doppler run --project navigator --config prd -- cargo run --release -p cli -- power-push --restart-only
```

Configuration is read from the environment — `NAVIGATOR_GHCR_OWNER` (the lowercase GitHub owner that owns the published
packages; defaults to `neon-law-foundation`, overridable by a fork), the GCP project / region / cluster for the kubectl
context, and `NAVIGATOR_PRIMARY_DOMAIN` for the smoke check. Nothing is hard-coded. See
[`.env.example`](../../.env.example) and [`cloud-operations.md`](../cloud-operations.md) for the full list and the
manual `kubectl` fallback.
