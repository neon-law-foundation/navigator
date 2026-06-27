---
name: web-preview
description: >
  Run the `web` app locally against the KIND dependency stack and look at it in a real browser — the canonical "spin up
  web, drive Chrome, screenshot to /tmp, verify" loop. Trigger whenever asked to run, preview, screenshot, or visually
  verify `web` / a page / a UI change, to "open the design page", "check it in chrome", or to prove a front-end behavior
  (syntax highlighting, a toast, a layout) actually renders. This is the browser half of the local loop;
  `kind-local-dev` is the cluster half it builds on. Skip for pure logic/unit work — `cargo test` uses testcontainers
  and needs no cluster.
---

# Previewing and screenshotting `web`

The recipe for seeing a `web` change in a real browser, against the real dependency stack. Every command here runs on
the user's machine (Docker, KIND, Chrome) — propose them for the user to run with `!`, or drive them when asked.

## The one rule that bites first

**`web` will NOT boot from `.devx/env` alone.** `web::config::enforce_prod_invariants` (called unconditionally from
`web/src/main.rs`) requires secrets that `devx up` does not write into `.devx/env` — `SENDGRID_EVENTS_SECRET`,
`SENDGRID_EVENTS_PUBLIC_KEY`, `DOCUSIGN_HMAC_KEY`. They live in Doppler (`navigator` / `dev`). So local `web` is
**always launched under `doppler run`**, with `.devx/env` sourced *after* so the KIND port-forward wiring (DATABASE_URL
→ `localhost:15432`, etc.) wins over Doppler's own values. Skipping Doppler crash-loops the pod with "production
invariants violated". See [[secrets-doppler]] and the `kind-local-dev` skill.

## The loop

### 1. Bring up the dependency stack (KIND)

```bash
cargo run --release -p cli -- start-dev-server        # cluster + Postgres + Keycloak + fake-gcs + OPA + Restate; writes .devx/env
```

This is "begin with KIND, all databases set up": Postgres is up and `web` runs migrations on boot, so the schema is
ready. The deps a `web` request actually touches (illustrative host ports, sourced from `.devx/env`):

| Dependency | Host port | What `web` uses it for | Skill |
| --- | --- | --- | --- |
| Postgres | `:15432` | every SeaORM query (port-forward to in-cluster Cloud-SQL-equivalent) | `postgres-in-kind` |
| Keycloak | `:30080` | OIDC sign-in (`/auth/login` → callback) | `keycloak-oidc` |
| fake-gcs | `:30443` | object storage (`cloud::StorageService`, GCS stand-in) | — |
| OPA | `:8181` | authorization decisions for `/portal/*` | `opa-policy` |
| Restate | `:9080` | durable workflow submission | `durable-execution` |

### 2. Run `web` (under Doppler, env layered on)

```bash
doppler run --project navigator --config dev -- \
  bash -c 'set -a; source .devx/env; set +a; cargo run -p web'
```

`web` binds `:3001`. Watch the boot log for `web listening addr=0.0.0.0:3001`. If it exits with "production invariants
violated", you skipped `doppler run`.

#### OpenTelemetry (on by default)

`navigator start-dev-server` stands up a Grafana **LGTM** pod (Loki/Grafana/Tempo/Prometheus + a bundled OTel Collector)
as a local OTLP sink, port-forwards its OTLP gRPC port, and writes
`OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317` into `.devx/env`.
So sourcing `.devx/env` (step 2) already flips host `web` to JSON logs + OTLP export — no manual port-forward. Browse
traces/logs/metrics at `http://localhost:3000` (Grafana, anonymous Admin). To run with plain stdout logs and no export,
set `OTEL_EXPORTER_OTLP_ENDPOINT=` (empty) in `.env`. Full local-telemetry loop is the [[grafana-lgtm]] skill; the
emit-side seam and the load-bearing "identifiers and counts, never client content" rule are the `observability` skill.

### 3. Open it in a real browser and screenshot

Screenshots go to `/tmp`, never the repo tree (`mkdir -p /tmp/navigator-screenshots` first).

```bash
mkdir -p /tmp/navigator-screenshots
google-chrome --headless=new --disable-gpu --no-sandbox --hide-scrollbars \
  --window-size=1366,4400 \
  --screenshot=/tmp/navigator-screenshots/page.png http://localhost:3001/design
```

`--screenshot` waits for the load event, so client JS (Bootstrap, htmx, Alpine, highlight.js) has run.

> `--dump-dom` does NOT execute load-event scripts — it captures the pre-JS DOM. Don't use it to check whether client
> JS ran; use a screenshot or a WebDriver session.

### 4. Prove client-side behavior (WebDriver)

For an assertion stronger than eyeballing a screenshot, drive the browser e2e suite against the running app. The tests
in `web/tests/browser_e2e.rs` skip cleanly when the harness is absent, so they double as a manual check:

```bash
chromedriver --port=9515 &
NAV_BASE_URL=http://localhost:3001 WEBDRIVER_URL=http://localhost:9515 \
  cargo test -p web --test browser_e2e -- --test-threads=1
```

### 5. Record a GIF of real interaction

A static screenshot proves a layout; a GIF proves *behavior* — a hover, a language switch, a count populating. Drive
chromedriver over its HTTP wire protocol with `curl` (no committed non-Rust code — it's an ephemeral `/tmp` capture),
snap a PNG frame after each action, then assemble with `gifski`. Frames and the GIF live under `/tmp`, never the repo.

```bash
mkdir -p /tmp/navigator-screenshots/frames && rm -f /tmp/navigator-screenshots/frames/*.png
pgrep -x chromedriver >/dev/null || chromedriver --port=9515 &   # reuse if already up

CD=http://localhost:9515
SID=$(curl -s -X POST "$CD/session" -H 'Content-Type: application/json' -d '{"capabilities":{"alwaysMatch":\
{"browserName":"chrome","goog:chromeOptions":{"args":["--headless=new","--hide-scrollbars",\
"--window-size=1366,900","--force-device-scale-factor=1"]}}}}' | jq -r .value.sessionId)

nav() { curl -s -X POST "$CD/session/$SID/url"          -d "{\"url\":\"$1\"}" >/dev/null; }
js()  { curl -s -X POST "$CD/session/$SID/execute/sync" -d "{\"script\":\"$1\",\"args\":[]}" >/dev/null; }
url() { curl -s "$CD/session/$SID/url" | jq -r .value; }
# A real in-page click ($1 = selector), then wait for navigation to land ($2 =
# substring the URL should reach). Dispatch via JS rather than the native
# element-click endpoint: in practice the native click did not reliably fire
# navigation on footer links, and JS .click() needs no `{}` POST body to forget.
click(){ js "document.querySelector('$1').click()"
  local i=0; until echo "$(url)" | grep -q "$2" || [ $i -ge 12 ]; do sleep 0.3; i=$((i+1)); done; }
# Force instant scroll (CSS scroll-behavior:smooth otherwise races the shot)
# and settle briefly before each frame so the footer is framed, not mid-scroll.
foot(){ js "document.documentElement.style.scrollBehavior='auto';window.scrollTo(0,document.body.scrollHeight);"; }
cap() { sleep 0.5; curl -s "$CD/session/$SID/screenshot" | jq -r .value | base64 --decode \
  > "/tmp/navigator-screenshots/frames/$(printf '%03d' "$1").png"; }

# One frame per beat — narrate the change the PR makes.
nav "http://localhost:3001/";      cap 0   # top of page
foot;                              cap 1   # scrolled to the English footer
click ".language-switcher" "/es";  foot; cap 2   # one real click → Spanish footer + English-legal note
curl -s -X DELETE "$CD/session/$SID" >/dev/null

gifski --fps 1.5 --quality 90 --width 1100 \
  -o /tmp/navigator-screenshots/footer.gif /tmp/navigator-screenshots/frames/*.png
```

`gifski` ships via `brew install gifski` (pair with `ffmpeg` if you'd rather record video and convert). Keep it short —
3–6 beats — and let each frame land on a distinct state, so the reviewer reads the interaction, not filler.

### 6. Share the capture

The capture lives in `/tmp` (e.g. `/tmp/navigator-screenshots/footer.gif`). Surface it for review — `Read` the PNG/GIF
so it renders inline in the agent session — and describe what it shows in the PR body's **Screenshots** section.

**Do NOT commit captures to the repo, and do NOT create an image-hosting branch.** For an image to *render* on the
github.com PR page it must be hosted by GitHub, and the only clean way is GitHub's native **user-attachments** (a
`user-attachments`/`user-images` URL, zero repo pollution). GitHub ships no public API for that upload, but the
[[pr-image-upload]] skill drives it from the CLI by replaying the browser upload flow with your github.com session — so
you can embed the `/tmp` capture into the PR body yourself, no drag-drop required. Avoid the tempting `pr-assets`
orphan-branch trick — it works, but leaves a stray binary-accumulating branch on the remote that someone has to
remember to delete. (Inside Cursor Cloud, skip [[pr-image-upload]] and use the artifact tags its PR tool resolves.)

## CSP gotcha (front-end JS)

`web/src/api.rs` sets `Content-Security-Policy: … script-src 'self'` (no `'unsafe-inline'`). An inline
`<script>…</script>` is **silently blocked** by the browser — the script simply never runs. Put front-end JS in a
first-party external file under `web/public/js/` (served as `'self'`, like `northstar-review.js` / `highlight-init.js`).
Inline `style=` attributes are fine (`style-src` allows `'unsafe-inline'`). This is exactly how the talk-slide
highlighter broke; a browser e2e is the only thing that catches it.

## Tear down

```bash
cargo run --release -p cli -- down
```

## Anti-patterns

- Sourcing `.devx/env` and running `web` without `doppler run` — crash-loops on missing invariant secrets.
- Trusting `--dump-dom` to confirm client JS ran — it doesn't execute load-event scripts.
- Writing screenshots into the repo — they belong in `/tmp/navigator-screenshots/`.
- Committing a capture into the repo (or an orphan `pr-assets` branch) to embed it in a PR — keep captures in `/tmp`;
  drag-drop into the PR for GitHub-hosted rendering instead (§6).
- Gating `cargo test` on KIND — tests get Postgres from testcontainers; the cluster is for *running* the app.
