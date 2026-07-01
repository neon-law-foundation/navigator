---
name: pr-image-upload
description: >
  Embed a local screenshot or GIF into a GitHub PR (or issue) body so it actually RENDERS on github.com, driven from the
  CLI — no drag-drop, no committing the file, no image-hosting branch, no release/tag. It uploads the `/tmp` capture to
  GitHub's native `user-attachments` CDN via the `gh-image` extension (which borrows your logged-in github.com browser
  session) and returns a real `https://github.com/user-attachments/assets/…` URL to drop into the body. Trigger as the
  embed half of [[create-pr]] Step 6 (after [[web-preview]] captures the visual), when a reviewer comment on a PR asks
  for a "live walkthrough"/screenshot during [[review-pr]], or any time you have a `/tmp` image that must appear in a
  PR/issue body or comment. This is the LOCAL/`gh` path for hosting and embedding a capture. Capture lives in
  [[web-preview]] §3/§5; this skill only hosts + embeds it.
---

# Embedding screenshots in a PR body from the CLI

GitHub deliberately ships **no public API** for the drag-and-drop image upload used in PR/issue bodies. The web UI hits
an undocumented endpoint (`/upload/policies/assets`) that **only accepts a logged-in browser session cookie** — a PAT or
the `gh` OAuth token is rejected. So an `<img src="/tmp/…">` in a `gh`-created body renders **broken** (GitHub resolves
it to `https://github.com/tmp/…` → 404), and the clean hosting options are all off the table per `CLAUDE.md`: don't
commit the file to the tree, don't push an image-hosting branch, don't cut a release/tag just to host a PNG.

The one path that satisfies all of that is GitHub's own **user-attachments** CDN, reached by replaying the browser
upload flow with your existing github.com session. The `gh-image` extension does exactly that.

## One-time setup

```bash
gh extension install drogers0/gh-image   # ships prebuilt binaries; no Go toolchain needed
```

It needs an **active github.com login in a supported browser** (Chrome/Brave/Edge/Chromium/Firefox/Safari) on this
machine — it reads the `user_session` cookie from the local cookie store. On macOS the first run pops a **Keychain
prompt** ("… wants to use the Chrome Safe Storage key"); the user must click **Allow** or nothing uploads. For a
headless/CI run with no browser, pass the session out of band via `GH_SESSION_TOKEN` instead (never `--token` on the
command line — it shows up in `ps`).

## The recipe

```bash
# 1. Capture to /tmp first (see web-preview §3 screenshot / §5 GIF) — never into the repo tree.
# 2. Look at it yourself: Read the PNG/GIF so it renders inline, and confirm it shows the change.
# 3. Upload — prints `![name](https://github.com/user-attachments/assets/<uuid>)`. Grab the URL directly
#    (grep the user-attachments link, not the parens — robust if the tool also emits a progress/warning line).
URL=$(gh image /tmp/navigator-screenshots/page.png --repo <owner>/<repo> \
  | grep -oE 'https://github\.com/user-attachments/assets/[^)]+')

# 4a. New PR: reference $URL in the body you pass to `gh pr create` (an <img> tag or ![alt]($URL)).
# 4b. Existing PR: splice it into the current body and update.
gh pr view <N> --json body --jq .body > /tmp/body.md
printf '\n\n## Walkthrough\n\n<img alt="…" src="%s" />\n' "$URL" >> /tmp/body.md
gh pr edit <N> --body-file /tmp/body.md

# 5. Verify it really resolves to an image (follows the redirect to signed storage).
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' -L "$URL"   # expect: 200 image/png
```

Upload **all** of a PR's images in one pass, then do a single `gh pr edit`, so the body isn't rewritten N times.

## When a review comment asks for a walkthrough

A common [[review-pr]] finding (e.g. Greptile P2 "missing live walkthrough artifact") is satisfied here: capture the
changed states ([[web-preview]]), embed them in the PR body with this skill, then **reply to the thread and resolve it**
([[review-pr]] Step 8) noting what the capture shows. Embedding the image is the fix; the reply + resolve closes it.

## Rules and caveats

- **Trust:** `gh-image` is third-party code that reads your full github.com **web-session cookie** (broader than a
  scoped PAT). It runs locally and the cookie only travels to GitHub, but treat installing/running it as a deliberate
  choice — the Keychain prompt is the consent gate.
- **Never** commit the capture, push an image-hosting branch, or create a release/tag to host it — user-attachments has
  zero repo pollution and is the only sanctioned host. (See [[web-preview]] §6 and `CLAUDE.md`.)
- The asset URL is scoped to the repo it was uploaded against — always pass the right `--repo`.
