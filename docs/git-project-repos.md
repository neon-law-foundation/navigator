# Every Project is a git repository

Every Project's documents live in **one simple git repository with a single branch, `main`** — large files (PDFs, docx,
images) versioned through **Git LFS** — and that repo *is* how we track changes to a matter. Nothing fancier: no second
branch, no tags, no pull requests, just commits appended to `main`.

Neon Law Navigator hosts one **append-only git repository per Project**, served Rust-native from `web`. The commit log
*is* the matter's audit trail — who changed what, when — and every version of every document is recoverable from
history. There is no separate versioning system and no new deployed service: `web` exposes a git smart-HTTP endpoint
gated by the session + OPA we already run.

This document is the durable design; the three councils' findings are folded into it. The raw deliberation is not kept.

## The pivot, stated plainly

On 2026-05-25 we moved per-Project storage from Gitea to a Google Drive shared-drive folder. **This design reverses
that.** Every Project becomes a real git repository because git gives us three things Drive never will:

- **Attribution** — the commit log records who changed what, natively. The audit trail *is* the history. **File
  history** — every version of every document, diffable and recoverable, with no separate versioning system.
  **Automation surface** — once "a matter is a repo," future automation (push-time notation linting, agent commits)
  composes on one well-understood primitive instead of a bespoke Drive API.

Google Drive is **fully removed**. `drive_folder_id` was dropped (migration
`m20260713_drop_drive_folder_id_from_projects`), along with the `DriveSync` workflow, the `aida_drive_*` tools, the
`cloud::drive` REST client, and the `cli drive` OAuth door — the git repo is the per-Project document system of record,
and you reach a matter by cloning its git URL (see below). Nothing in the dependency graph speaks to Drive any longer.

## Anchor decision

**Rust-native, hosted inside `web`. No Gitea, no new pod.** `web` serves git over HTTPS, gated by the existing session
and OPA. One auth model, one binary, one deploy. This honors the workspace's "Rust only" rule and the "share as much
auth as possible, keep it simple" goal that motivated the ask.

The reference URL shape is `https://www.your-domain.example/projects/<project-id>.git`. The `.git` suffix is the only
thing that distinguishes the git client (HTTP Basic, pack protocol) from the portal's HTML documents view (session
cookie) at the shared `/projects/:id` prefix — the router splits on it.

## Append-only, single `main` — the only ref

Per the matter-record requirement, each repo is **append-only with exactly one branch, `main`**. No other branches, no
tags, no pull requests — additive history only. The server enforces this so a misconfigured client cannot violate it:

- `receive.denyNonFastForwards = true` and `receive.denyDeletes = true` in each bare repo's config. A `pre-receive` hook
  rejects any ref update whose name is not `refs/heads/main`, and rejects any non-fast-forward update to `main`. The
  only writes that land are new commits appended to `main`.

This is a deliberate simplification, not a limitation we apologize for. Stating what the system *does*: it keeps one
linear, additive record per matter. There is therefore no branch-level ACL to design — push authorization is simply "may
this identity append to this matter's `main`?" (see Authorization). The single documented exception is the **governed
expunge** (see Confidentiality, retention & governed expunge), an out-of-band admin operation, never a push.

### Jujutsu (jj) — evaluated, not adopted for the server

The append-only, additive, linear model is exactly jujutsu's mental model, so jj is worth a look. The finding: jj does
**not** simplify the *server*.

- jj's only production-ready backend **is Git** — it reads and writes the same `.git` on-disk format via gitoxide, and
  the commits it makes are ordinary Git commits. A jj user and a git user can share the same remote and neither knows
  the difference.
- jj is a **client-side** ergonomic layer (no staging area, working-copy-as-commit, an operation log). None of that
  changes the wire protocol our multi-tenant server speaks: clients still `git clone` / `git push` over smart-HTTP, and
  the repo on disk is still a bare `.git`.

So the server design below is identical whether a given lawyer drives it with `git` or with `jj` — and a lawyer who
prefers jj's ergonomics can use it today against our git remote with zero server changes. We document jj as a supported
*client*, not a server dependency.

`jj-lib` (Rust, built on gitoxide) is explicitly designed to be usable "in a server serving requests from multiple
users," which makes it a candidate for **server-side commit authoring** (see Commit attribution) instead of shelling
`git commit`. We do **not** adopt it now: the library API is young and only the Git backend is production-ready. We
revisit `jj-lib` for server-side authoring when its public API stabilizes; until then server-side commits shell to `git`
(the same binary the transport already requires).

## 1. Smart-HTTP transport

`web` serves git smart-HTTP by shelling to git's own `git upload-pack` (fetch) and `git receive-pack` (push) with
`--stateless-rpc`, exactly as git's reference HTTP server drives them. The axum handler runs `--advertise-refs` for the
`GET .../info/refs` ref advertisement (prefixing the pkt-line service banner), pipes the client's RPC body to the child
stdin on `POST`, and streams stdout back; gzip-encoded upload-pack request bodies are inflated first. Implemented in
`web/src/git_http.rs` (`git http-backend` CGI is an equivalent).

**Why shell out, not pure-Rust `gix`:** `feedback_infra_kind_gke` says lean on mature upstreams over hand-rolled
infrastructure. `gix` server-side pack negotiation is not mature enough to own the protocol edge-cases today. Shelling
to git's reference server gets the full, battle-tested protocol for free. **Fallback:** revisit a pure-Rust server when
`gix` server-side support lands; the handler is the only thing that would change.

**Cost owned explicitly — the runtime image must carry `git`.** The current prod runtime is
`gcr.io/distroless/static:nonroot` (`images/Dockerfile.web`), which has no shell and no `git` binary;
`web/src/git_meta.rs` already documents this. The git-serving path therefore runs from a minimal base image that
includes the `git` binary (e.g. `gcr.io/distroless/base` with `git` and its runtime deps copied in, or a
`debian:stable-slim` + `git`). This is a real, named cost of the transport choice.

## 2. Auth sharing — the credential a git client presents

A browser read can ride the session cookie, but `git clone` / `git push` from a CLI sends HTTP Basic or a bearer — it
has no cookie. `web` mints a **short-lived, Project-scoped Personal Access Token (PAT)** that the lawyer pastes into
git's credential helper. The git client sends it as HTTP Basic (`username: any`, `password: <pat>`, the GitHub-style
convention) over HTTPS.

`web` validates the PAT in the **same place `/mcp` validates its bearer** — beside `web::google_oauth`
(`require_google_oauth`, `web/src/google_oauth.rs:195`) — so there is one token-validation seam, not a parallel password
store. A PAT resolves to a single `persons` identity and is revocable in one database row. PATs are scoped (read vs.
read-write) and Project-scoped; a leaked read-PAT is revoked by deleting its row.

In KIND the same path holds (Keycloak is the OIDC provider, but the git credential is still a `web`-minted PAT, so the
git transport is identity-provider-agnostic).

## 3. Authorization — git verbs mapped to the model we already have

One repo ↔ one Project, so the existing project-scope check *is* the repo ACL (`authorization-model`). Read and write
resolve through `person_project_roles.participation` + OPA, with silent `admin` bypass.

- **Fetch / clone** (`GET .../info/refs?service=git-upload-pack`, `POST .../git-upload-pack`) → OPA query: may this
  identity *read* this Project? (`participation` present, or `admin`).
- **Push** (`GET .../info/refs?service=git-receive-pack`, `POST .../git-receive-pack`) → a **separate, stricter** OPA
  query: may this identity *write* this Project? Push is a superset of fetch and is never granted implicitly.

The authorization middleware sits in front of the transport handler, keyed on the `service` (for `info/refs`) or the URL
suffix (`git-upload-pack` vs `git-receive-pack`). The three failure modes return distinct statuses: **401** (no/invalid
PAT), **403** (valid identity, OPA denies), **404** (no such Project). A dying subprocess is **500**.

## 4. Where bare repos physically live — the single-writer git store

Git needs a POSIX filesystem for a bare repo; **GCS is not one.** Working repo storage is therefore a persistent volume,
not a bucket. This introduces the workspace's first stateful tier in the request path, so the topology matters.

**Reference deploy:** a dedicated **git-serving Deployment running the same `web` image** with a role flag, pinned to
`replicas: 1`, mounting a single **ReadWriteOnce** PVC at the bare-repo root (`GIT_PROJECT_ROOT`). The public, stateless
`web` tier proxies `/projects/:id.git/*` to it. This mirrors the shape we already run for Restate (a stateful backend
the stateless tier talks to) — git hosting is isolated, not smeared across every `web` replica.

- **Concurrency:** `replicas: 1` plus a per-repo advisory lock keyed by project id serializes `receive-pack` so two
  concurrent pushes cannot corrupt a bare repo.
- **Backup:** scheduled **volume snapshots** of the PVC, explicitly *distinct* from the Cloud SQL backup story we
  already run. The matter record now lives in two backup domains (SQL rows + repo volume); the doc names that.
- **KIND:** the same role-flagged binary with a `hostPath`/PVC volume; the `navigator` CLI wires the mount
  (`kind-local-dev`).
- **GKE Autopilot:** Filestore CSI (RWO) or a PD-backed PVC; Filestore provisioning has real lead time, so it is
  provisioned ahead of cutover (`project_gcp_production_stack`).

**This is the single riskiest unknown** the engineering council named: concurrent-write safety and backup of the
bare-repo volume behind a stateless web tier. The single-writer Deployment + advisory lock + volume snapshots are the
mitigation; it gets real weight in implementation and review.

## 5. Git LFS, backed by `cloud::StorageService`

PDFs, docx, and images go through **Git LFS**, and the LFS object store is our existing `cloud::StorageService`
(`cloud/src/lib.rs:65`) — GCS in prod, the Fs backend in KIND. *This* is where GCS stays in the picture; the repos
themselves do not live in a bucket.

- Each repo ships a `.gitattributes` routing binary types to LFS: `*.pdf`, `*.docx`, `*.png`, `*.jpg`, `*.jpeg` →
  `filter=lfs diff=lfs merge=lfs -text`.
- `web` implements the **LFS batch API** (`POST .../info/lfs/objects/batch`) plus object upload/download actions. An
  upload action `put`s the object to `StorageService` under an env-driven bucket, with no CDN; a download action issues
  a `signed_url`. The LFS *pointer* (committed in the pack) and the `StorageService` object reconcile by the pointer's
  `oid` (sha256) → storage key.
- The same OPA fetch/push checks gate the LFS batch endpoints — read for download actions, write for upload actions.

## 6. Data model + migration

- A SeaORM migration (`store`, `m`-prefixed, `inserted_at`/`updated_at` per `feedback_timestamp_convention`) adds repo
  identity to `projects`: `git_initialized_at` (nullable timestamp; set when the bare repo is created). There is **no**
  branch column — the ref is always `main`, enforced by the `pre-receive` hook and pinned once in
  `repos::DEFAULT_BRANCH`, so a per-row branch name would only duplicate that constant. The original
  `git_default_branch` column (default `main`) was therefore dropped in
  `m20260719_drop_git_default_branch_from_projects`; `drive_folder_id` and the retired `drive_syncs` table were likewise
  dropped (the latter in `m20260718_drop_drive_syncs` — see the note above).
- A `git_access_tokens` table holds PATs: `id`, `person_id`, `project_id` (nullable = all the person's projects),
  `token_hash`, `scope` (`read` | `write`), `expires_at`, `inserted_at`/`updated_at`. Tokens are stored hashed; the
  plaintext is shown once at mint time.
- **Backfill:** existing Project documents (in GCS) become the **initial commit(s)** of each repo, with a
  one-time migration that preserves authorship and date metadata where the source records it (commit author = the
  `persons` identity who uploaded; commit date = the document's recorded date), so the initial history is faithful
  rather than a single "import" blob.
- Regenerate the ERD (`docs/erd.md` + `docs/erd.svg` via `erd-visualization`) when the migration lands.

## 7. Commit attribution = the audit trail

Commits made on a person's behalf — portal upload, inbound-email attachment, e-sign completion, an agent action — are
authored as **that `persons` identity** (name + email), so `git log` is a faithful "who did what, when." Server-side
commits set `GIT_AUTHOR_NAME` / `GIT_AUTHOR_EMAIL` / `GIT_COMMITTER_*` (or the `jj-lib` equivalent if we later adopt it)
from the acting person's row. Demo identities stay zodiac (`project_zodiac_demo_users`): commits in seeded matters are
authored as `<sign>@example.com`.

## 8. Surfaces that currently touch Project documents

Each becomes a read/write against the repo. **The client never sees the word "git"** — the portal stays a documents view
(the client council guards this), and a view-layer test asserts that no client-facing template emits `git`, `clone`,
`branch`, or a commit SHA.

- **Inbound-email attachments** → matter documents become commits to `main` authored as the sender's `persons` identity.
  **E-signature flow** (`project_esignature_design`) → the generated and signed PDFs are committed (PDFs ride LFS).
  **Northstar review surface** (`project_northstar_estate_flow`, `review_documents` / `document_comments`) → reads the
  document content from the repo HEAD.
- **`/portal` document listing** → a plain, dated, named list rendered from the repo working tree at HEAD, with a
  one-click **"Download all my documents"** that produces a friendly **ZIP of files** (never a packfile or git bundle).

## 9. Confidentiality, retention & governed expunge (legal council)

The commit log is an acceptable — indeed superior — record of a legal matter, framed precisely as **tamper-evident and
append-only by default**, never "immutable" or "cannot be deleted." A reviewing court reads "we cannot delete it" as
obstruction, so the design ships a governed deletion path from day one.

- **Governed expunge** is an **admin-only** primitive keyed by project id, used for a privilege clawback, a sealing
  order, or a client's lawful deletion request. It rewrites history to remove the blob, deletes the corresponding object
  from `StorageService`, `gc`s the pack, and **records the expunge itself** — who authorized it, when, and the category
  (privilege / sealing / client-request), but **not** the content — so the audit trail survives the redaction. This is
  the only operation that is not append-only, and it is never reachable as a git push. *Implemented:*
  `repos::RepoStore::expunge_path` (history rewrite + prune + gc), `store::expunge_records` (the who/when/category audit
  log), and `web::expunge::expunge` (the admin-gated orchestrator tying rewrite + storage deletion + record). An
  `/admin` HTTP route to drive it is the remaining UI wiring.
- **Retention:** the repo is retained for the bar's record-retention period after the matter closes (a floor commonly
  cited at five years; the attorney of record confirms the exact period per jurisdiction), then becomes eligible for
  governed deletion. No indefinite-by-inertia retention.
- **Confidentiality:** the per-Project ACL (= the Project's participation set) is a confidentiality improvement over a
  shared Drive. Read-PATs are individually revocable and Project-scoped; force-push and expunge are restricted to
  `admin` and logged.
- **Client export + lawful deletion** are both first-class: "Download all my documents" (ZIP, no git jargon) and a
  client-initiated "Delete this document" that enqueues the attorney-authorized governed expunge and confirms honestly
  only once the working tree, history, and LFS object are all scrubbed.

## 10. KIND + prod parity

The same Rust code path runs in both, per CLAUDE.md: transport and LFS are identical; only the volume class and the
`StorageService` backend differ by env. KIND uses a `hostPath`/PVC volume and the Fs `StorageService` backend, wired by
the `navigator` CLI (`kind-local-dev`). Prod uses an Autopilot RWO PVC and the GCS backend
(`project_gcp_production_stack`). Every per-deploy value (bucket names, volume class, repo root) is env-driven.

## Implementation sequence

Per the engineering council's Libra: ship fetch before push (clone-only is useful and halves the auth blast radius).

1. `store` migration + entity for repo identity and `git_access_tokens`; regenerate ERD.
2. Bare-repo store (init `main`-only append-only bare repo, path-for-project, ensure-exists).
3. PAT minting + validation beside `google_oauth`.
4. Read-only fetch transport (`info/refs` + `git-upload-pack`) gated by the OPA read query.
5. Push transport (`git-receive-pack`) gated by the OPA write query, with the append-only `pre-receive` hook.
6. LFS batch API over `StorageService`.
7. Commit attribution + repoint the document surfaces; add a CLI git helper.

## Deploying the git-serving tier

The transport and `commit_as` shell the `git` binary, which `gcr.io/distroless/static` (the `navigator-web` runtime)
does not carry. The git-serving tier therefore runs from [`images/Dockerfile.git`](../images/Dockerfile.git) — the
*same* musl-static `web` binary on a `debian:stable-slim` + `git` base. Reference GKE manifests are in
[`examples/deploy/k8s/gke/git/git-serving.yaml`](../examples/deploy/k8s/gke/git/git-serving.yaml): a `replicas: 1`
`Recreate` Deployment with an RWO PVC mounted at `NAVIGATOR_GIT_REPO_ROOT`, a Service, and a `VolumeSnapshot` for the
backup story (distinct from Cloud SQL's). Add one ingress prefix rule (`/projects` → `navigator-git`) so the stateless
`navigator-web` tier proxies the whole transport + LFS surface to the single writer. The manifests are not yet wired
into the overlay's `kustomization.yaml` — apply and validate them on the cluster (the PVC bind, the snapshot schedule,
the ingress split are machine-side checks), then wire them in.

In **dev/KIND** the `web` binary runs on the host (`kind-local-dev`), so there is no git-serving pod: point
`NAVIGATOR_GIT_REPO_ROOT` at a local directory in `.devx/env` and the host binary serves repos from there against the
in-cluster deps.

## Follow-ups (not done here)

- Repoint the Northstar review (`review_documents`, HTML-in-DB — decide whether it maps to the repo), and build the
  client-facing "Download all my documents" ZIP export. The governed-expunge primitive is built (see §9); only its
  `/admin` HTTP route + the client "Delete this document" button remain.
- Wire `examples/deploy/k8s/gke/git/git-serving.yaml` into the overlay `kustomization.yaml` and the ingress once it is
  validated on the cluster.
- Revisit `jj-lib` for server-side commit authoring when its public API stabilizes.
