---
name: vibe-react
description: >
  Build a Project's client portal in React + Vite, in that Project's repository under `portal/` — the one lane in this
  product where a fast, exploratory prototype IS the shipped implementation rather than a specification translated to
  Rust. Trigger when working in a Project repository's `portal/` tree, when asked to "build this in React", to add a
  screen or route to a Project's portal, or when a vibe-coded prototype should ship as itself. Encodes the mount
  contract (the base derived from the repository name plus the literal `portal`, `import.meta.env.BASE_URL`), the data
  boundaries (reads through Navigator's `/api` read clusters, writes through the one REST command boundary, never a
  bespoke JSON endpoint or a second backend), and the two hard rules (no legal files, no client data). The end-to-end
  loop — Linear issue, triage, pull request, review — is [`docs/vibe-coding.md`](../../../docs/vibe-coding.md). For a
  screen that belongs to Navigator itself, the React is a prototype and `design-mockup-translation` is the skill.
---

# Vibe coding a Project's client portal in React

A Project's portal is a React application built with Vite, living under `portal/` in that Project's private repository,
served by Navigator at `/app/projects/<code>/portal`. **What you write here ships as itself.** Nothing is translated, so
iterate fast: build the screen, look at it, keep going.

**The portal is what the client sees.** One Project, one portal, rendered in the client lens. It carries a link back to
`/app/projects`, and Lawyer and Clerk can preview a Project as the client to see exactly this screen.

That speed is safe because the constraints that matter are enforced outside your code. Navigator decides who may reach
the mount before your bundle is served, the command boundary decides whether a write is allowed, and Drive holds the
legal files. What is left for you is layout, states, copy, and interaction.

This is Navigator's *other* repository shape. Nothing in [`AGENTS.md`](../../../AGENTS.md) about Cargo, KIND,
`worktree-env`, or the workspace test suite applies to a `portal/` tree — it has no Rust, and Node never enters the
Navigator workspace. Use the portal's own `package.json` scripts.

## Before the first edit

1. **Confirm which lane you are in.** If the screen belongs to Navigator itself — a portal page, a lawyer surface, a
   marketing page, anything at a Navigator route — stop. The React is a prototype and the shipped screen is Dioxus; use
   [`design-mockup-translation`](../design-mockup-translation/SKILL.md). This skill is only for a screen that belongs to
   one Project and is served at that Project's portal mount.
2. **Note the repository's name.** It is the Project code, and with the literal `portal` it is the whole mount. Nothing
   declares it a second time, so there is no manifest to edit and nothing to keep in sync.
3. **Confirm the Project is real.** Only Navigator can answer whether this Project exists and carries a published
   portal:

   ```bash
   navigator projects doctor --project <project-code>
   ```

4. **Find the Linear issue.** Planning is Linear only; there are no GitHub issues in this lane. If the exploration has
   no issue yet, that is expected at this stage — write it after you have seen the screen work, per
   [`docs/vibe-coding.md`](../../../docs/vibe-coding.md).

## The mount contract

One repository serves one Project, is named for that Project's code, and holds one portal:

```text
<org>/<project-code>
├── portal/                 # the client-facing React + Vite portal
└── templates/              # the Project's notation templates
```

Navigator serves `portal/` at `/app/projects/<code>/portal/`. The trailing slash is load-bearing twice: Vite joins asset
URLs directly onto the base, and Navigator redirects the bare mount to that form.

`vite.config.ts` builds `base` as `/app/projects/<repository-name>/portal/`, and the mount gate checks the built bundle
against the same derivation. It needs no inputs, because CI already knows the repository name:

```yaml
- run: pnpm --dir portal build
- uses: neon-law-foundation/navigator/.github/actions/validate@YY.M.D
  with:
    version: "YY.M.D"
    project_repository: true
```

Pin that action to an exact Navigator release tag. Never consume `main` or `latest`.

There is no application name anywhere in this — no composed `<code>-<app>` string to be ambiguous about, and nothing for
a caller to choose. Do not reintroduce one.

The `portal` segment is also why Navigator's matter show page survives: `/app/projects/{id}` is a lawyer surface, and a
portal mounted at `/app/projects/<code>` directly would shadow it.

### The traps the gate exists to catch

- **An absolute path in source escapes the mount.** A Vite base rewrites module and asset URLs; it does **not** rewrite
  an `href`, `src`, or `to` string you wrote by hand. `href="/logo.svg"` survives the build pointing at whatever
  Navigator serves at `/logo.svg`, which is not your application. Build every absolute path from
  `import.meta.env.BASE_URL`, and let the router's `basename` come from the same value.
- **The link back to `/app/projects` is the one allowed absolute path.** It points deliberately outside the mount, so
  the gate must permit it explicitly. Every other absolute path is a defect.
- **A repository is not a published portal.** The gate proves the build is mounted where the repository name says; only
  `navigator projects doctor` can say whether Navigator will actually serve it.
- **Slug shape** is lowercase letters, digits, and single hyphens, alphanumeric at both ends, at most 80 characters.
  That is the Project *code*'s rule — there is no application name to validate.
- **The repository holds two kinds of source.** `portal/` and `templates/` are gated differently, so a change under
  `portal/` runs the mount gate and a change under `templates/` runs the notation validator. Do not assume a green check
  on one proves the other ran.

## The data boundaries

**You own the screen. You do not own the data.** A Project repository holds template and portal source plus its
checked-in configuration, and never brings its own matter-data backend.

- **Reads** go through Navigator's `/api` read clusters. Compose the screen from those responses.
- **Writes** go through the one REST command boundary in
  [`docs/command-boundary.md`](../../../docs/command-boundary.md). A form is a thin adapter over the command that
  already performs the write.
- **Never add a bespoke JSON endpoint for one screen**, and never stand up a second backend, database, or storage
  bucket. A screen that seems to need one is either missing a Navigator read cluster or asking an existing endpoint for
  a shape it should return. Either way that is a Navigator dependency and its own Linear issue — say so rather than
  routing around it.
- **Never make a client-side authorization decision.** Authorization comes from Navigator: `persons.role` for the
  system tier and `person_project_roles.participation` for Project scope, resolved before the bundle is served. See
  [`docs/access-model.md`](../../../docs/access-model.md). Hiding a button is presentation, not access control, and must
  never be the only thing standing between a viewer and data.
- **The client lens is the only lens you build.** Lawyer and Clerk previewing the portal see the client's view — that is
  the point of the preview. Never branch the portal on viewer tier to reveal firm-side material.
- **Session comes from Navigator's session**, not a gateway endpoint of your own and not a hand-written cookie.

## The two hard rules

**No legal files, ever.** Git never stores legal files; Drive and Navigator assets do. A Project repository may not hold
client uploads, answers, generated legal documents, secrets, dependencies, or build output.

**No client data in fixtures, tests, or copy.** Every name, address, email address, matter title, and document body is
invented or firm-owned. Non-firm email addresses use a reserved example domain (`example.com`, `example.org`,
`example.net`); real phone numbers do not appear at all. This is the same rule the no-client-data test enforces on every
Navigator pull request, and it applies here by discipline plus the CI proof.

## Building it

- **Pin `@neon-law/ux`.** The shared component library is published from `neon-law/ui` and each portal pins a
  released version. Reach for its components before writing a local one; a divergent local button is how two portals
  stop looking like one product.
- **Build every state.** Empty, loading, error, and success. Exploration naturally produces the happy path, and the
  missing states are where these screens actually fail in front of a client.
- **Copy is English, written where it renders.** There is no catalog and no key lookup anywhere in this product, and no
  application publishes a translated surface.
- **Third-party libraries are the point.** There is deliberately no dependency allowlist for a Project repository —
  it owns its own graph, and the lockfile flavor is not constrained. Keep the lockfile committed.

## Proving it

Run this repository's own scripts — lint, typecheck, unit tests, and build — before opening the pull request:

```bash
pnpm install --frozen-lockfile && pnpm lint && pnpm typecheck && pnpm test && pnpm build
```

Then verify the two things a green unit suite cannot see:

- **The build is mounted where Navigator will serve it.** The mount gate checks `dist/index.html` against the derived
  base, so confirm the built asset URLs start with `/app/projects/<code>/portal/`.
- **The screen actually works in a browser** at its base path, not at `/`. A route that works at the dev-server root and
  breaks under the mount is the most common defect in this lane, and it is invisible to every test that does not load
  the built bundle.

Capture the interaction for the pull request. A walkthrough defaults to a GIF of the real interaction — states included
— and a still only for a genuinely static change.

## Closing the loop

Open the pull request against the Project repository's `main` with the Linear magic word in the body, so the merge
transitions the issue. Failed checks and inline review comments are [`fix-checks`](../fix-checks/SKILL.md): one finding,
one fix, one reply carrying the proof.
