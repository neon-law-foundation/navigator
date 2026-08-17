# Project workspace and repository contract

Each Navigator [Project](glossary.md#project) coordinates four distinct surfaces. They are not interchangeable stores:

| Surface | Authority | Contains |
| --- | --- | --- |
| Google Drive | Firm working files | Legal files and internal working material |
| Navigator | Matter record | Project identity, participation, Notations, and asset provenance |
| Project repository | Source control | Notation templates and client-portal source only |
| Served client portal | Authorized application | The Project's client-facing surface |

Git never stores legal files. Google Drive and Navigator assets do. A Project's deletion handoff contains legal files
only; it does not include the repository, portal source, CI output, or operational history.

## One repository per Project code

A Project has **one** repository, named for its Project code, in **one** organization. It holds that Project's notation
templates and its client portal side by side:

```text
<organization>/<project-code>
├── .github/workflows/gate.yml
├── portal/            # React + Vite; the client's portal
├── templates/         # *.md notation blueprints
├── AGENTS.md
├── CLAUDE.md
├── LICENSE.md
└── README.md
```

The Project code is the stable Navigator `projects.code`. It is the repository name, and it is the Project folder
basename in its deployment's selected Drive root. That equality is why the slug rules are what they are: lowercase
letters, digits, and single hyphens, alphanumeric at both ends, at most 80 characters. Drive and macOS are
case-insensitive, so uppercase would let one folder answer to two codes; one separator keeps the mapping an equality
check rather than a normalization.

`new` is refused as a Project code. `/app/projects/new` is Navigator's matter-open form, so a Project coded `new` would
collide with a literal route. Which side of a genuine collision wins depends on route registration order, so the code is
refused rather than the precedence reasoned about — in `store::projects::is_valid_code` and in an `ASSERT` on
`project.code`, because a Rust check only guards the write paths that call it.

## Nothing declares its own name

The repository name **is** the Project code, and Navigator serves that Project's portal at the repository name plus one
literal segment:

```text
/app/projects/<project-code>/portal/
```

So the Vite base is derivable from the repository name alone, and there is nothing left for a manifest to say. Both
manifests that used to restate it are gone: an application repository's root `mount.json` and a template repository's
`navigator.toml`. Each existed only to re-spell what the repository was already called, which meant four things had to
agree where two facts now suffice — the repository name and the directory it is in. CI reads the name it already has, as
`github.event.repository.name`.

The trailing slash is load-bearing twice: Vite joins asset URLs directly onto the base, and Navigator redirects the bare
mount to the slashed form.

**The extra `portal` segment is the point.** Mounting at `/app/projects/<code>` directly would shadow Navigator's own
matter show page at `/app/projects/{id}`. The two differ in path shape — three segments after `/app/projects` versus two
— so neither can match the other's request.

## The organization is configuration, not a name in source

Navigator spells no organization and no forge host anywhere in its source. Both come from the deployment's own
configuration, `NAVIGATOR_GITHUB_ORG` and `NAVIGATOR_GIT_HOST`, and `cli/tests/forge_coordinate_retired.rs` is the guard
that keeps it that way.

| Deployment | GCP project | Organization | Drive root |
| --- | --- | --- | --- |
| Production | `neon-law` | `neon-law` | `Projects` |
| Staging | `neon-law-stg` | `neon-law` | `Staging Projects` |
| Foundation | `neon-law-org` | `neon-law-foundation` | `NLF Projects` |

The active deployment is identified by `NAVIGATOR_GCP_PROJECT_ID`. It is deliberately not `NAVIGATOR_ENVIRONMENT`, which
is a two-valued dev/production switch and cannot name three deployments.

**One string means two different things across those two vocabularies: the organization `neon-law` is staging, while the
GCP project `neon-law` is production.** That inversion is accepted rather than accidental — the organizations are named
for the entities and the GCP projects for the deployments. It is the single most likely way to ship to the wrong place,
so it lives in the configuration an operator reads rather than in source where it would have to be remembered.

### An absent coordinate is legitimate

A Project's repository is a **derived coordinate that may not exist**. With no deployment named there is no organization
and no host, so there is no coordinate — and that is correct rather than degraded. The local development loop and the
test suite name no deployment, and neither writes one into `.devx/env`.

So the two absences are different questions with different answers:

| State | Answer |
| --- | --- |
| No deployment named | The repository pointer is absent. Not an error. |
| A deployment named, with no organization or host | A hard error, with no fallback. |

There is no default forge host. A fallback to a public one would silently aim every Project's clone URL at a namespace
the Firm does not control, which is exactly the defect this contract removed: `ops github setup` documented having no
public fallback while the pointer that actually served users had one.

## The CI gate

One composite action is the whole gate, consumed identically by every Project repository in every organization:

```yaml
- uses: actions/checkout@<sha>  # v7
- run: pnpm --dir portal build
- uses: neon-law-foundation/navigator/.github/actions/validate@YY.M.D
  with:
    version: "YY.M.D"
    project_repository: true
```

It carries no organization, host, deployment, or client name, because none of those vary: the repository name is the
Project code and the mount is that name plus a literal. Only the host differs between deployments, and a host never
appears in a Vite base. `cli/tests/project_gate.rs` pins the shell against the Rust definitions it transcribes, because
bash cannot call Rust.

**There is no path filter, and that is deliberate.** A filtered job that skips reports success for work it never did,
and a required check a skip can satisfy is not a gate. So the one job always runs and each half no-ops over a repository
that does not carry it. The job is spelled `ci`, which is the one required context `navigator ops github setup` binds.

What the gate proves:

- The layout is source-only. Client uploads, answers, generated documents, secrets, dependencies, and build output are
  refused by path and by extension.
- Every direct `templates/<code>.md` passes the notation rules, and each template's `code` equals its filename stem.
- Where a `portal/` exists, it is a Vite workspace — a `package.json`, an `index.html`, and a lockfile. The lockfile
  flavor is not constrained and there is deliberately **no dependency allowlist**: third-party libraries are the point,
  and Node never enters the Navigator workspace.
- The built `index.html` is mounted at `/app/projects/<code>/portal/`, so a base that never reached the build fails here
  rather than in production.
- No absolute path in `portal/src/` escapes the mount. A Vite base rewrites module and asset URLs and never an `href`
  written by hand, so a literal in-app path survives the build pointing at whatever Navigator serves there instead.
  Navigator's own namespaces, `/app/` and `/auth/`, are the deliberate exception: a portal links back to `/app/projects`
  and out through `/auth/logout`, and those are outside the mount on purpose.

Pin the action to an exact `YY.M.D` release tag, never `main` or `latest`. Publishing a rolling pointer is allowed;
consuming one is not.

## Publishing the built bundle

The gate proves the bundle; a second composite action publishes it.
`neon-law-foundation/navigator/.github/actions/application-publish@YY.M.D` runs after the gate, in the same job, and
uploads `portal/dist/` to `<code>/portal/` in the deployment's private `<deployment>-applications` bucket, which
Navigator streams object-by-object. Objects land **flat** under that prefix; the action derives `<code>` from the
repository name, exactly as the gate does, so the object prefix cannot disagree with the served mount.

It carries no organization, host, or client. The three coordinates it cannot derive are passed from GHE repository
**variables** — a provider resource name, a service-account email, and a bucket name are public identifiers, and the
trust lives in the Workload Identity binding on Google's side, not in the workflow:

| Variable | Value |
| --- | --- |
| `NAVIGATOR_APPLICATIONS_BUCKET` | the deployment's private applications bucket, e.g. `neon-law-applications` |
| `NAVIGATOR_APP_PUBLISHER_WIF_PROVIDER` | the full `ghe-oidc` Workload Identity provider resource |
| `NAVIGATOR_APP_PUBLISHER_SERVICE_ACCOUNT` | `navigator-app-publisher@<project>.iam.gserviceaccount.com` |

Authentication is keyless: the job mints a short-lived OIDC token from the enterprise issuer
`https://token.actions.githubusercontent.com` and federates it into the publisher, so no service-account key exists.
That issuer is a property of the provider resource, not a workflow parameter — the same subtlety the [marketing
sites](marketing-sites.md) document explains.

The thin caller workflow lives in the Project repository, not here. It grants `id-token: write`, installs with a locked
dependency graph, lints, typechecks, tests, and builds with the derived Vite base, runs the gate, then publishes:

```yaml
# <organization>/<project-code>/.github/workflows/publish.yml — an example of what a
# Project repository contains, not a file in this repository.
name: publish
on:
  push:
    branches: [main]
permissions:
  contents: read
  id-token: write            # required to mint the OIDC token WIF federates
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@<sha>            # v7
      - run: pnpm --dir portal install --frozen-lockfile
      - run: pnpm --dir portal lint
      - run: pnpm --dir portal typecheck
      - run: pnpm --dir portal test
      - run: pnpm --dir portal build            # Vite base /app/projects/<code>/portal/
      - uses: neon-law-foundation/navigator/.github/actions/validate@YY.M.D
        with:
          version: "YY.M.D"
          project_repository: true              # the one gate: source-only, no legal files, mounted
      - uses: neon-law-foundation/navigator/.github/actions/application-publish@YY.M.D
        with:
          applications_bucket: ${{ vars.NAVIGATOR_APPLICATIONS_BUCKET }}
          workload_identity_provider: ${{ vars.NAVIGATOR_APP_PUBLISHER_WIF_PROVIDER }}
          service_account: ${{ vars.NAVIGATOR_APP_PUBLISHER_SERVICE_ACCOUNT }}
```

**Upload order is load-bearing, and the never-delete rule is what distinguishes a private, shared applications bucket
from a public marketing site.** The action uploads in two passes — everything except `index.html` first, then
`index.html` last — so that by the time any HTML naming a new hashed filename is readable, that file already exists.
Neither pass deletes: a stale hashed asset is left unreachable rather than removed, and one Project's publish can never
prune another's objects out of the flat namespace. It then stamps `index.html` with the publish provenance — commit,
build time, and repository metadata — as GCS custom metadata surfaced at `x-goog-meta-commit` and its siblings.

**Rollback is a revert on `main`, republished.** There is no rollback job. A bad bundle is undone by reverting it on the
Project repository's `main` and letting the caller workflow publish the reverted tree; because the action never deletes,
every rollback is a forward publish rather than a recovery of something removed.

The versioned reusable-workflow home for the shared caller is `ux/core`; wiring the thin caller there — so a Project
repository consumes one `uses:` line instead of transcribing the job above — is a hand-off, because this repository
cannot push to `ux/core`.

## Scaffolding a repository

```bash
navigator projects repository scaffold <project-code> --dir .
navigator projects repository validate .
```

`scaffold` is idempotent and leaves existing files alone. It writes the repository shell and the templates half — the
gate workflow, `README.md`, `AGENTS.md`, `CLAUDE.md`, a `templates/project_template.md` placeholder, and `tests/`.

It does **not** write `portal/`. That arrives from the vibe-coding lane ([`vibe-coding`](vibe-coding.md)), which knows
how to make a Vite application and which released `@neon-law/ux` version to pin. Keeping it out of the scaffold is what
lets `validate` be unambiguous: `portal/` present means there is a portal to hold to the Vite contract, and absent means
this Project does not have one yet.

`validate` accepts all three shapes — templates only, a portal only, or both — and reports a repository carrying neither
distinctly rather than failing it. A Project may legitimately open before either half exists.

The template directory is flat. Each `templates/<code>.md` file is a Project-local notation blueprint; it is not part of
Navigator's shared `templates/neon_law` or `templates/forms` catalog. Navigator reads the file at `main`, validates its
notation contract, persists its bytes as a content-addressed Asset, and records the imported commit SHA as provenance.

## Local checkouts

One checkout root per organization, holding one directory per Project code. The root is the organization's own name, so
nothing has to be translated between the coordinate and the path:

```text
~/neon-law/<project-code>
~/neon-law/<project-code>
~/neon-law-foundation/<project-code>
```

These are **source** roots. Git never stores legal files, so they must not converge with the Drive mount
(`NAVIGATOR_PROJECTS_DRIVE_MOUNT`), which is a separate path holding the firm's working files.

## Verifying a machine

`navigator projects doctor` reports whether this machine and one Project workspace actually satisfy the map above,
before anything is created:

```bash
navigator projects doctor
navigator projects doctor --project spotonix
```

It resolves the active deployment from `NAVIGATOR_GCP_PROJECT_ID`, then reports that deployment's Google Workspace,
Shared Drive, and Projects root folder, an optional local Drive mount, the stored site login, and — with `--project` —
that Project's Drive folder path, its one repository coordinate, and the path its portal mounts at.

The command is strictly read-only, and it now makes no network or database call at all: the diagnosis is a pure function
of an environment lookup, a filesystem-existence probe, the stored credentials, and a clock. A Workspace, Drive, folder,
or identity mismatch exits nonzero rather than warning. Configuration that is genuinely optional, such as an unset Drive
mount or an absent login, is reported as a warning and does not fail the run. A deployment that cannot be resolved stops
the report immediately, because every later coordinate would otherwise describe some other Workspace.

It is not `ops doctor`, which diagnoses scheduled-job health in a running Kubernetes namespace.

## Shared notations

The notations that are not specific to one Project live in **this** repository, under `templates/neon_law/<product>/`
and `templates/forms/`. They are Navigator's own catalog, versioned and validated with its source.

A Project repository's `templates/` directory is separate from that catalog rather than an extension of it: it carries
the blueprints belonging to that Project, and Navigator imports each one at the commit it reads. A Project-local
notation may record its lineage in a `derived_from` frontmatter field, which is documentation — the rule engine does not
resolve it, so it creates no dependency on any other repository being present.

**There is no cross-deployment template repository, and a Project repository must not assume one.** Everything a
Project's notations need is either in that Project's own `templates/` or in this repository's catalog.

## Source boundaries

`neon-law-foundation/navigator` is Navigator's source repository. It is not a Project repository. Each Project's
repository is its own deployment-specific source repository, and its portal pins a released shared component-library
version.

A Project repository may hold notation templates, portal source, fixtures, tests, and the checked-in configuration
required to build or validate them. It may not hold client uploads, answers, generated legal documents, secrets,
dependencies, or build output.

## Access boundary

Navigator Project participation authorizes Navigator and the served client portal. It never grants GitHub Enterprise
access. Outside lawyers work through Navigator, Drive, and the served portal without GHE membership. Repository access
is an independently administered source-control decision.

This is also why the model stops at one repository per Project rather than one repository per organization with
`projects/<code>/` subdirectories, which would be the same logic one step further. **Repository access is the per-matter
access boundary**: a per-organization monorepo would hand every contributor every matter's source. One repository per
Project code is the floor.

## Implementation boundary

This contract deliberately leaves deployment provisioning, access reconciliation, and migration to their own
implementations. Those changes must preserve these authorities and may not reintroduce legal files into Git.
