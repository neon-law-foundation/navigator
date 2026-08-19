# Using the Navigator Workshop

This workshop uses one local matter from start to finish: **Simpson v. Flanders**. The matter code is `simpsons`, its
portal application is the [Navigator sample project](https://github.com/neon-law-foundation/navigator-sample-project),
and its local data is synthetic. Every attendee sees the same matter through the role assigned to their local account.

## Intro

### Learning objectives

- **Remember** — identify Project, Template, Notation, and Workflow in the workspace glossary.
- **Understand** — connect each noun to the database row that makes the workflow durable and inspectable.
- **Apply** — open the Simpsons matter, bind the shared retainer template, and view the client portal application.
- **Analyze** — inspect the notation state and the matter's participation-scoped views.
- **Evaluate** — review the client-facing portal and identify one useful improvement.
- **Create** — make a small, testable change in the sample project and refresh the local portal.

---

Start by naming the four nouns. The workshop keeps every exercise on Simpsons so the room shares one durable reference.

### The running matter

The local development fixture seeds one open Project:

- **Name** — Simpson v. Flanders
- **Code** — `simpsons`
- **Matter** — trespass to land
- **Repository** — [navigator-sample-project](https://github.com/neon-law-foundation/navigator-sample-project)
- **Portal** — `/app/projects/simpsons/portal/`

The Project code is the public URL key. Codes use lowercase letters and numbers separated by single hyphens, so a
project page is always readable as `/app/projects/<code>`.

---

Point out the code in the portal URL. It is the stable, human-readable project identity used throughout the exercise.

## Develop locally

### Start the local room

The Navigator CLI owns the complete local lifecycle. From a New Worktree, run:

```bash
cargo run -p cli -- dev worktree-env up --path "$PWD"
set -a; source .devx/env; set +a
cargo run -p neon
```

The boot command provisions the KIND dependency tier, applies the schema, seeds the Simpsons fixture, clones and builds
the sample project, stages its `dist/` output, and writes the generated environment. The host web process reads that
environment on startup, so the real sample application is ready at the portal link after each boot.

The explicit refresh command uses the same build and staging path when the sample project changes:

```bash
cargo run -p cli -- dev sample-project
```

Restart `web` after refreshing so it reads the new staged bundle. The generated `.devx/env` contains
`NAVIGATOR_SAMPLE_PROJECT_DIR`; source it before starting the host process.

---

Run the boot commands before proceeding. Confirm that `.devx/env` names the staged sample-project directory.

### Sign in

The local Rauthy fixture supplies five role-named accounts, all using the password `password`:

| Account | Role | Simpsons access |
| --- | --- | --- |
| `owner@neonlaw.com` | owner | firm-side matter view |
| `admin@neonlaw.com` | admin | administration surface; participation can be granted there |
| `lawyer@neonlaw.com` | lawyer | firm-side matter view |
| `clerk@neonlaw.com` | clerk | supervised matter view |
| `client@neonlaw.com` | client | client matter view and portal |

Open `$NAV_BASE_URL/auth/login`. Firm accounts land on `/app/team`; the client account lands on `/app/projects`. The
project list and detail page use `simpsons` in the URL. The client portal is available at:

```text
$NAV_BASE_URL/app/projects/simpsons/portal/
```

The portal is participation-scoped. The client, lawyer, clerk, and owner rows are part of the fixture, and the admin
account is the local administrator used to exercise the participation controls at `/app/admin`.

---

Have each attendee sign in with the role relevant to their work. Keep the browser on the Simpsons detail page.

## Work the Simpsons matter

### The four nouns in one workflow

1. **Project** — Simpson v. Flanders, the matter that owns the work.
2. **Template** — a versioned Markdown blueprint such as `onboarding__retainer`.
3. **Notation** — one client and one Template bound inside the Project.
4. **Workflow** — the states and transitions that move a Notation from intake through review and signature.

The shared retainer template is available in the canonical catalog. A lawyer can bind it through the AIDA catalog:

```text
aida_create_notation(template_code="onboarding__retainer", project_id=<Simpsons project id>)
```

The notation begins in its seeded workflow state. The lawyer reviews the generated work, advances the workflow through
the configured transitions, and the resulting documents remain tied to the Simpsons Project and its audit trail.

---

Trace one notation from its template through its workflow state. Relate each step back to the same Simpsons Project.

### Inspect the client portal

Sign in as `client@neonlaw.com`, open `/app/projects`, and select **Simpson v. Flanders**. The detail page keeps the
human-readable code in the address bar:

```text
/app/projects/simpsons
```

Select the portal link to open the sample application's bundled client experience. The application is built from the
public sample repository and mounted under the Project's code, which gives the sample a complete path from repository to
matter-specific browser surface.

---

Ask the room to identify what the client can see from this page and what the firm-side matter view adds for the team.

### Make a sample-project change

The sample repository declares its Navigator Project in `navigator.yml`:

```yaml
name: simpsons
```

Edit the sample project, run the refresh command, restart `web`, and reload the portal URL. Boot validates the manifest,
builds the frontend, stages the output, and publishes the generated assets before the entry document. This keeps the
portal tied to the declared Simpsons Project while the browser reloads the new version.

---

Make one small visual change, refresh the bundle, and show the reloaded portal. The manifest name remains `simpsons`.

## Wrap Up

### Verify the room

Run the browser and accessibility gate against the sourced environment:

```bash
cargo run -p cli -- dev browser-e2e
```

The gate signs in the local personas, checks the matter surfaces, and exercises the real local browser path. The Rust
suite and feature walkthrough cover the same fixture and its seeded participation rows:

```bash
cargo nextest run --workspace && cargo test -p features
```

The workshop is ready when the Simpsons matter appears in the intended role view, `/app/projects/simpsons` is the detail
URL, and `/app/projects/simpsons/portal/` renders the sample application.

---

Close by verifying the same Simpsons portal path together. The seeded matter, development flow, and browser proof all
meet at one URL.
