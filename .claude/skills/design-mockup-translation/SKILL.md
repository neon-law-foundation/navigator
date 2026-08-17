---
name: design-mockup-translation
description: >
  Translate an accepted `design-mockup` issue — a GIF plus reference HTML/CSS/JS filed by a non-Rust contributor — into
  a Dioxus implementation in the `webapp` crate. Trigger when picking up an issue filed with the `design-mockup` form,
  when someone asks to build the mockup from a named issue, or when a vibe-coded prototype needs to become a shipped
  screen. The attached HTML/CSS/JS is reference material and is never merged, vendored, or served. Reads go through the
  `/app/api` read clusters of navigator 866 and writes through the REST command boundary of navigator 355 — never a
  bespoke JSON endpoint. Encodes the Dioxus gotchas (hydration comments, textarea RCDATA, script nonce, theme.css
  chrome, e2e selector hooks, inline English copy, the brand task-local, feature-file URLs) so each translation does not
  rediscover them. The intake side — what a filer submits and why — is
  [`docs/design-mockups.md`](../../../docs/design-mockups.md).
---

# Translating a design mockup to Dioxus

A `design-mockup` issue is a **specification**, not a contribution. The filer prototyped outside this repository and
attached a rendered GIF plus the HTML/CSS/JS that produced it. Your job is to read that intent and write the Rust that
delivers it. Nothing from the issue is committed, bundled, vendored, or served.

This is a "create a pull request" action under [`AGENTS.md`](../../../AGENTS.md). Start from the issue, work test-first,
and land the covering test with the minimal implementation it proves.

## Before the first edit

1. **Read the whole issue**, opening body through every comment. The GIF is the specification; the reference source is
   how you resolve spacing, ordering, and interaction timing that the image leaves ambiguous.
2. **Confirm it was accepted.** An unattested, image-less, or client-data-carrying mockup is closed, not built.
3. **Resolve the target surface against the real router**, not the issue's guess. Find the route in `server/src/site.rs`
   and the page module in `webapp/src/`.
4. **Map every "data the screen reads" line to an existing read endpoint** before writing a component. If the read does
   not exist yet, that is a dependency on an #866 cluster — say so on the issue rather than inventing a JSON route.
5. **Map every "data the screen writes" line to an existing command.** Writes travel the #355 boundary; see
   [`docs/command-boundary.md`](../../../docs/command-boundary.md).
6. **Check the authorization tier** the filer chose against [`docs/access-model.md`](../../../docs/access-model.md).
   `persons.role` is the system tier; per-project scope is `person_project_roles.participation`. The mockup shows one
   lens — build the others deliberately or state that the screen is single-tier.

## The two boundaries

- **Reads** go through the `/app/api` read clusters (#866). A page composes its data server-side from those handlers or
  the shared `store` query they call. It never opens a second read path.
- **Writes** go through the REST command boundary (#355). A form is a thin adapter over an `/app/api/*` command handler
  or the shared `portal` / `store` / `workflows` command it calls. Cookie-authenticated browser writes keep CSRF.
- **Never build a bespoke JSON endpoint for one screen.** A mockup that seems to need one is either missing a cluster
  (file or reference the issue) or asking for a shape the existing endpoint should return.

## The gotcha list

Each of these has cost a PR at least once. Read them before writing the component, not after the test goes red.

- **Dioxus SSR wraps text in hydration comments.** A `<!--node-id-->` sits between an element's attributes and its text,
  so assert on `>Text<` and never on `class="x">Text`. Attributes escape `&` as `&#38;`, and `ssr_only` emits `<title>`
  before the doctype. A "missing row" in an SSR test is usually a matcher bug, not a broken mount.
- **`<textarea>` ignores the `value` attribute** under HTML's RCDATA parsing — the box renders empty — and a child text
  node leaks hydration comments into the content. Set the body with `dangerous_inner_html` over **escaped** content.
  Only a real browser catches this; the SSR string looks fine.
- **Hydration data ships as inline `<script>` blocks** that a strict `script-src` blocks outright. The fix is the
  per-response nonce middleware (`portal/src/dioxus_app.rs`), never `unsafe-inline` and never a CSP relaxation. A unit
  test cannot see this: load the page in a real browser and read the console.
- **Every page needs its chrome rules in `server/public/css/theme.css`.** Page-chrome classes (`.lawyer-nav`,
  `.nav-link`, `.nav-table`, `.portal-*`) have shipped with no CSS behind them because SSR tests assert class names, not
  styles — an unstyled page ships green. Add the rules in the same PR and prefer the existing `.nav-table` over a new
  table style.
- **Keep the styling-free `admin-form` class on every form.** It is the selector the accessibility and browser suites
  drive (`form.admin-form` in `server/tests/accessibility_e2e.rs` and `server/tests/browser_e2e.rs`). Dropping it is a
  nightly `WaitTimeout` on the deploy gate, not a red test in your PR. Grep `server/tests/*_e2e.rs` for every selector
  the surface you are replacing exposes, and carry each one forward.
- **Copy is written inline, in English, in the component that renders it.** There is no catalog and no key lookup, so a
  string the mockup shows becomes a literal in the `rsx!` that renders it. Repeated copy earns a named `const` only when
  two surfaces must move together; otherwise write it where it renders.
- **Server functions do not inherit the brand task-local.** `FIRM_BRAND` / `firm_email` read inside a `#[server]` body
  yield the *default* brand, not the request's. Resolve brand values at construction from the request's brand bundle (or
  a pre-layer) and pass them in. Any portal `AppState` value a server fn needs is injected as a wasm-safe `Extension`,
  the way `CsrfToken` and `ViewerRole` are — never read from the environment.
- **Feature files encode public URLs.** A route change must sweep `features/tests/features/*.feature`; a source grep
  misses Gherkin paths. Those suites run with `cargo test -p features`, not nextest.

## Building it

- The component lives in the `webapp` crate under `webapp/src/`, in the Dioxus Components theme. Compose the lens
  server-side; do not ship a client-side authorization decision.
- Port the *design* — layout, ordering, states, copy — not the markup. The reference CSS is a description of the
  intended result; express it with the existing theme tokens and chrome classes rather than pasting declarations.
- Build every state the issue named: empty, loading, error, success. A mockup usually shows only the happy path, and the
  missing states are where translations go wrong.
- No CDN, no new npm dependency, no vendored copy of the filer's JavaScript. Progressive enhancement is limited to what
  is already vendored.

## Proving it

- An SSR test per lens over the real rendered string, matching `>Text<` (see the first gotcha).
- The e2e selector hooks preserved, verified by grepping `server/tests/*_e2e.rs` before and after.
- `cargo nextest run -p webapp -p portal -p server` for the changed area, plus `cargo test -p features` if you touched a
  route a feature file names. Clippy runs workspace-wide (`--workspace --all-targets -D warnings`); a `-p` subset misses
  lints. Leave the full sweep and coverage to CI.
- A real browser pass for anything the string tests cannot see: the nonce, the textarea body, and whether the page is
  actually styled. Use the `web-preview` skill and capture the GIF for the PR.

## Closing the loop

Link the PR to the mockup issue and post the walkthrough capture on it, so the filer sees their design running. If the
implementation deliberately departs from the mockup — an accessibility fix, an authorization constraint, a state the
prototype could not know about — say which detail changed and why, on the issue, before the PR merges.
