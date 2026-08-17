---
name: web-preview
description: >
  Run the `web` app locally against the KIND dependency stack and look at it in a real browser — the canonical "spin up
  web, drive Chrome, capture to /tmp, verify" loop. Trigger whenever asked to run, preview, screenshot, or visually
  verify `web` / a page / a UI change, to "open the design page", "check it in chrome", or to prove a front-end behavior
  (syntax highlighting, a toast, a layout, a keyboard shortcut) actually renders. A PR walkthrough defaults to a GIF of
  the real interaction (§5); a still is only for a genuinely static change. This is the browser half of the local loop;
  `kind-local-dev` is the cluster half it builds on. Skip for pure logic/unit work — `cargo test` uses an embedded
  store
  and needs no cluster.
---

# Previewing and screenshotting `web`

The recipe for seeing a `web` change in a real browser, against the real dependency stack. Every command here runs on
the user's machine (Docker, KIND, Chrome) — propose them for the user to run with `!`, or drive them when asked.

## The one rule that bites first

Source the generated `.devx/env` before starting `web`. It carries the explicit staging harness plus the KIND
connections, the shared `navigator` database, and the unique port that the local process needs. A gitignored `.env` is
only for an optional live third-party integration.

## The loop

### 1. Bring up the dependency stack (KIND)

```bash
cargo run --release -p cli -- dev up        # cluster + SurrealDB + Rauthy + fake-gcs + OPA + Restate; writes .devx/env
```

This is "begin with KIND, all databases set up": the store is up and `web` applies the schema on boot, so it is
ready. The deps a `web` request actually touches (illustrative host ports, sourced from `.devx/env`):

| Dependency | Host port | What `web` uses it for | Skill |
| --- | --- | --- | --- |
| SurrealDB | `:18000` | every store query (port-forward to the in-cluster engine) | `kind-local-dev` |
| Rauthy | `:30080` | OIDC sign-in (`/auth/login` → callback) | `rauthy-oidc` |
| fake-gcs | `:30443` | object storage (`cloud::StorageService`, GCS stand-in) | — |
| OPA | `:8181` | authorization decisions for `/app/*` | `opa-policy` |
| Restate | `:9080` | durable workflow submission | `durable-execution` |

### 2. Run `web`

```bash
set -a; source .devx/env; set +a
cargo run -p neon
```

`web` binds `:3001`. Watch the boot log for `web listening addr=0.0.0.0:3001`.

### 2a. Public page images

Markdown images under `server/content/**` resolve through the public asset seam: authors write `img/<slug>/<file>`,
local dev serves `/public/img/<slug>/<file>` from `server/public/img/`, and production can serve the same key from the
public assets bucket via `NAVIGATOR_ASSET_BASE_URL`. That directory is gitignored by design.

- If a preview shows alt text or a broken image, first check whether the file exists under `server/public/img/`. On a
  fresh clone, restore the bucket-backed images with `cargo run -p cli -- ops assets pull` (set
  `NAVIGATOR_ASSETS_BUCKET` or pass `--bucket`).
- If the task adds or replaces a blog/marketing/workshop image, put the finished file at its final
  `server/public/img/<slug>/<file>` path for local preview, then make the cloud handoff explicit:

  ```bash
  cargo run -p cli -- ops assets upload --dir server/public/img
  ```

  That publishes recognized image files (`.avif`, `.webp`, `.jpg`, `.jpeg`, `.png`) to `gs://<project>-assets/img/...`.
- Do not commit `server/public/img/` bytes. The PR carries the Markdown/code change; the public bucket carries the image
  bytes. If you did not run the cloud upload, say so and provide the exact command.
- After uploading, confirm the reference resolves at the public origin with `cargo run -p cli -- ops assets verify` (or
  `--base-url http://localhost:<web-port>/public` against the running loop). It fetches every `img/…` image reference
  under `server/content` and fails on any 404 — the `deploy` workflow runs the same gate, so a missing hero blocks the
  release.

**Per-worktree preview (parallel agents / Codex).** To preview several worktrees at once without colliding on `:3001`,
run `cargo run -p cli -- dev worktree-env up` in each worktree first: it writes a `.devx/env` pointing `web` at that
worktree's own port (`3001` + a stable offset derived from the worktree path) on the shared deps. Then source that
`.devx/env` and run `web` exactly as above (it binds the per-worktree port). The **port is the only per-worktree
resource** — the deps and the `navigator` database are shared, so parallel worktrees see each other's rows; that same
sharing is what lets a worktree drive Restate-backed flows through the in-cluster worker. See `kind-local-dev` and
`AGENTS.md` §Local KIND development.

#### OpenTelemetry (on by default)

`navigator dev up` stands up a Grafana **LGTM** pod (Loki/Grafana/Tempo/Prometheus + a bundled OTel Collector) as a
local OTLP sink, port-forwards its OTLP gRPC port, and writes `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317` into
`.devx/env`. So sourcing `.devx/env` (step 2) already flips host `web` to JSON logs + OTLP export — no manual
port-forward. Browse traces/logs/metrics at `http://localhost:3000` (Grafana, anonymous Admin). To run with plain stdout
logs and no export, set `OTEL_EXPORTER_OTLP_ENDPOINT=` (empty) in `.env`. Full local-telemetry loop is the
[[grafana-lgtm]] skill; the emit-side seam and the load-bearing "identifiers and counts, never client content" rule are
the `observability` skill.

### 3. Open it in a real browser and screenshot

A still is the quick look for **yourself** while iterating. **A PR walkthrough defaults to a GIF (§5)** — reach for a
still only when the change is genuinely static (a color, a spacing fix, a one-shot render) and there is no keypress,
click, or state transition to show. "A screenshot can't show the keypress, so I'll shoot before/after" is backwards:
that is exactly the case a GIF exists for.

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
  cargo test -p server --test browser_e2e -- --test-threads=1
```

### 5. Record a GIF of real interaction — the default walkthrough

**Default to a GIF for every PR walkthrough.** A static screenshot proves a layout; a GIF proves *behavior* — a hover, a
language switch, a count populating, an arrow key advancing a slide. The reviewer sees the interaction happen instead of
taking your word that two stills are cause and effect, so a GIF is the default artifact and a still is the exception
(§3), not the reverse. Annotated before/after stills are not a substitute: they show two states, never the input that
connects them.

Drive chromedriver over its HTTP wire protocol with `curl` (no committed non-Rust code — it's an ephemeral `/tmp`
capture), snap a PNG frame after each action, then assemble with `gifski`. Frames and the GIF live under `/tmp`, never
the repo.

Keyboard-driven UI is captured the same way: send the real key through WebDriver's actions endpoint rather than clicking
the control it activates, so the GIF proves the *keystroke* path a reviewer doubts.

```bash
# One real ArrowRight keypress ($1 = the key's Unicode code point, e.g.  for ArrowRight).
key() { curl -s -X POST "$CD/session/$SID/actions" -H 'Content-Type: application/json' \
  -d "{\"actions\":[{\"type\":\"key\",\"id\":\"kbd\",\"actions\":[{\"type\":\"keyDown\",\"value\":\"$1\"},\
{\"type\":\"keyUp\",\"value\":\"$1\"}]}]}" >/dev/null; }
```

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
`github.com` PR page it must be hosted by the tenant, and the clean way is its native **user-attachments** store
(a `https://github.com/user-attachments/assets/…` URL, zero repo pollution). The [[pr-image-upload]] skill drives
that from the CLI with a single `curl` authenticated by `gh auth token` — no browser session, no extension — so you can
embed the `/tmp` capture into the PR body yourself, no drag-drop required. Avoid the tempting `pr-assets` orphan-branch
trick — it works, but leaves a stray binary-accumulating branch on the remote that someone has to remember to delete.

## CSP gotcha (front-end JS)

`web/src/api.rs` sets `Content-Security-Policy: … script-src 'self'` (no `'unsafe-inline'`). An inline
`<script>…</script>` is **silently blocked** by the browser — the script simply never runs. Put front-end JS in a
first-party external file under `server/public/js/` (served as `'self'`, like `northstar-review.js` /
`highlight-init.js`). Inline `style=` attributes are fine (`style-src` allows `'unsafe-inline'`). This is exactly how
the talk-slide highlighter broke; a browser e2e is the only thing that catches it.

## Tear down

```bash
cargo run --release -p cli -- dev down
```

## Anti-patterns

- Starting `web` without first sourcing `.devx/env` — misses the KIND wiring and local staging harness.
- Trusting `--dump-dom` to confirm client JS ran — it doesn't execute load-event scripts.
- Writing screenshots into the repo — they belong in `/tmp/navigator-screenshots/`.
- Committing a capture into the repo (or an orphan `pr-assets` branch) to embed it in a PR — keep captures in `/tmp`;
  drag-drop into the PR for GitHub-hosted rendering instead (§6).
- Gating `cargo test` on KIND — tests open their own embedded store; the cluster is for *running* the app.
