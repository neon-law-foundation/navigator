---
name: review-pr
description: >
  Review a pull request end-to-end AND close the loop on every reviewer comment. Given a PR number (or URL), it pulls
  the metadata + diff, reads the actually-changed files at the head commit, forms an independent correctness/quality
  assessment, then walks every outstanding comment — Greptile, CodeRabbit, human reviewers, any bot — and for each one
  validates or refutes the claim against the real code, asks the user whether to fix it, applies the fix when told to,
  and replies to the thread via the `gh` CLI so nothing is left hanging. Comment resolution is a REQUIREMENT, not a
  nicety: a PR is not "reviewed" until every comment has a reply (and, where it is a real thread, is marked resolved).
  When no PR is named, it DEFAULTS to the PR for the current branch (never asks "which PR?" unless the branch genuinely
  has none or maps to several), and when the branch has fallen behind `main` it updates the branch from `main` so the
  review lands on a current tree and auto-merge is unblocked. Trigger when the user says "review PR", "review the PR",
  "review PR #N", "review this pull request", "look at the comments on #N", "go through the Greptile comments", or
  pastes a GitHub PR URL and asks for a review. A bare "review PR" with no number is the common case — resolve it to the
  current branch's PR automatically. This is the COMMENT-RESOLUTION review front door; for grouping a dirty tree into
  commits and opening a PR use [[create-pr]], for a deep multi-agent cloud review use `/code-review ultra`, and for a
  from-scratch diff read with no existing comments the built-in `/review` also works.
---

# `/review-pr` — review a PR and resolve every comment

The job: take a pull request and leave it in a state where (1) you have given an honest, code-grounded assessment of the
change, and (2) **every reviewer comment has been answered** — fixed-and-replied, or acknowledged-with-rationale and
replied, and resolved where it is a real review thread. The deliverable is not just "here's what I think"; it is "every
open thread is closed or has a decision on it."

`main` is merge-only and lands via auto-merge once CI is green, so unanswered bot comments are the thing that quietly
rots a PR. This skill exists to make sure that never happens: **no comment is left without a reply.**

## The whole flow, in order

1. **Identify** the PR — explicit number/URL if given, else the current branch's PR → `{owner}/{repo}` + number.
2. **Read** the PR — metadata, the full diff, then the changed files at the head commit.
3. **Assess** independently — form your own correctness + quality view before reading any bot's opinion.
4. **Collect** every comment — inline review comments, the review summary, and issue/PR comments, from all reviewers.
5. **Adjudicate** each comment — validate or refute it against the real code; classify severity.
6. **Ask** the user, per actionable comment, whether to fix it (recommendation first).
7. **Fix** the ones they approve — on the PR branch, with the covering test, honoring the gate.
8. **Resolve** every comment — reply via `gh` to each thread; mark real threads resolved. This step is mandatory.
9. **Update from `main`** — if the branch is behind `main` and that blocks the merge, update it from `main` and push.
10. **Report** — give the user your findings, every verdict, the fix shas, and confirmation that all threads are closed.

Each step assumes the prior one. Do them in order.

## Step 1 — Identify the PR

If the user named a PR — a bare number (`#33`), a URL — use it. **If they did not** (a plain "review PR" / "review the
PR"), **default to the PR for the current branch — do not ask "which PR?"**. The whole point of "review PR" is that the
branch already tells you which one:

```bash
gh repo view --json nameWithOwner -q .nameWithOwner          # the {owner}/{repo} for this checkout
gh pr view --json number -q .number                         # the PR for the CURRENT branch (no number needed)
```

`gh pr view` with no argument resolves the open PR whose head is the current branch. That is the answer in the common
case — proceed with it silently. Only fall back to asking the user when that lookup genuinely can't decide:

- **No PR for the branch** (`gh pr view` errors with "no pull requests found") — tell the user there's no PR for the
  current branch and ask for a number, or offer [[create-pr]] if the tree has unshipped work.
- **You're on `main`/detached, or in a different worktree than the branch under discussion** — there's no single
  obvious branch. List the recent open PRs (`gh pr list --json number,title,headRefName`) and ask which one, *unless*
  the immediately-preceding conversation already pins one (e.g. a PR you just opened this session) — then use that.

Use the resolved slug in every `gh` call below as `--repo <owner>/<repo>`.

## Step 2 — Read the PR

```bash
gh pr view <N> --repo <slug> \
  --json title,body,state,author,baseRefName,headRefName,additions,deletions,changedFiles,mergeable,reviewDecision
gh pr diff <N> --repo <slug>     # full diff; if large, scope to the files you care about
```

The diff is the claim; the **files at the head commit are the truth.** Read the real files, not just the patch hunks — a
comment can be wrong because of context outside the hunk. Either check the branch out, or shallow-clone the head ref to
`/tmp` (never into the working tree — see `CLAUDE.md` scratch rule):

```bash
git fetch origin <headRefName> && git switch <headRefName>     # to also be able to fix
# or, read-only:
git clone --depth 1 --branch <headRefName> <repo-url> /tmp/pr-<N>
```

## Step 3 — Assess independently first

Before you read a single bot comment, form your own view, so the bots don't anchor you. Focus on what actually breaks or
rots:

- **Correctness** — does each changed path do what its name/PR claims? Trace the real code path; "it compiles" and "it
  looks right" are not evidence (the `CLAUDE.md` no-assumptions rule). Run or read the covering test.
- **Tests that lie** — a test whose assertion passes for the wrong reason (short-circuits before the code it names,
  matches an always-present string). These give false confidence and are worth flagging even when the bots miss them.
- **Schema / migration ordering**, transactional integrity, auth checks (route through the [[authorization-model]]),
  and the workspace invariants in `CLAUDE.md`.
- **Quality** — reuse, dead code, altitude — but keep it secondary to correctness.

Write this up as your own findings. You will reconcile it with the bot comments in Step 5.

## Step 4 — Collect every comment

There are three distinct comment surfaces on a GitHub PR. Pull all three — bots split findings across them (Greptile,
for example, puts unplaceable findings in its summary, not inline):

```bash
# (a) inline review comments — anchored to a file + line, these form resolvable threads
gh api --paginate repos/<slug>/pulls/<N>/comments --jq '.[] | {id, user: .user.login, path, line, in_reply_to_id, body}'

# (b) issue/PR-level comments — top-level, includes bot review SUMMARIES (Greptile/CodeRabbit overview)
gh api --paginate repos/<slug>/issues/<N>/comments --jq '.[] | {id, user: .user.login, body}'

# (c) review bodies — the "approve/request-changes" top notes
gh pr view <N> --repo <slug> --json reviews -q '.reviews[] | {author: .author.login, state, body}'
```

`--paginate` is load-bearing: without it `gh api` returns only the first page (30 items) and silently drops every
comment past it — and a PR with more than 30 inline comments is exactly where leaving one unanswered is easiest, which
would break this skill's core invariant.

Read bot summaries in full — they often carry P-rated findings (P1/P2/P3), a confidence score, and "comments outside
diff" that never became inline threads. Treat every distinct finding as a comment to adjudicate, wherever it lives.

## Step 5 — Adjudicate each comment

First, **filter the Step 4a set to thread roots** — comments where `in_reply_to_id` is null. The non-null ones are
existing replies (a bot or human already responded), not new findings; adjudicating them produces duplicate re-replies
and inflates the question set. Keep the replies around for context — they tell you whether a thread is already answered
— but only roots enter the loop below.

For **each** finding, do not take the bot's word for it. Open the cited file at the head commit and decide:

- **Valid** — the claim holds against the real code. Confirm severity (a lying test or a missing auth check ranks above
  a style nit). Note the exact fix.
- **Invalid / false positive** — the claim is wrong (the bot missed surrounding context, the pattern is intentional and
  consistent with the codebase, the "bug" can't actually occur). Note *why*, with the file:line evidence.
- **Valid but won't-fix** — real but not worth changing (matches an established file-wide pattern, theoretical edge
  guarded elsewhere). Note the rationale.

State your verdict on each with the evidence, so the user is deciding from facts, not from the bot's confidence.

## Step 6 — Ask the user whether to fix

For every comment you classified **Valid** (and any **won't-fix** you're unsure about), ask the user whether to apply
the fix. Lead with your recommendation. Use `AskUserQuestion` for a clean per-comment decision when there are several; a
short inline question is fine for one or two. Do **not** silently fix or silently skip — the user decides what lands.

Invalid / false-positive comments don't need a fix question — but they still get a reply in Step 8 explaining why you're
not acting (that is the resolution).

## Step 7 — Apply the approved fixes

Make sure you're on the PR branch (`git switch <headRefName>`), then fix exactly what was approved:

- Add or update the **covering test in the same change** — for a "test that lies" finding, the fix *is* making the test
  exercise the path it names, then proving it (run it, and confirm the assertion now keys on something that only the
  real code path produces).
- Honor the workspace gate before committing (`cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test`), plus [[markdown-lint]] on any `.md`. See [[create-pr]] for the full gate.
- If a comment asks for a **live walkthrough / screenshot** (a common bot finding on a UI change), capture the changed
  states ([[web-preview]]) and embed them in the PR body with [[pr-image-upload]] — the embedded image is the fix; then
  reply + resolve in Step 8. This needs no code commit, so it can land even on a docs/UI PR with no Rust changes.
- Commit on the branch as a Conventional Commit referencing the finding, and push so CI re-runs:

```bash
git add <paths> && git commit -m "test(web): exercise the real client-DRI guard … (Greptile P2 on #<N>)"
git push
```

## Step 8 — Resolve every comment (REQUIRED)

This step is the point of the skill. **A PR is not reviewed until every comment has a reply *and* every thread you have
handled is marked resolved.** Replying is not enough on its own: branch protection requires conversations to be
resolved, so an answered-but-unresolved thread still **blocks auto-merge** — the PR sits there green but unmergeable.
Always do both, in this order: reply (say what you did), then resolve the thread. For each finding:

**Reply to inline threads** (use the comment id from Step 4a):

```bash
gh api repos/<slug>/pulls/<N>/comments/<comment_id>/replies -f body='Fixed in <sha> — <one line>.'
# or, for a won't-fix / false-positive:
gh api repos/<slug>/pulls/<N>/comments/<comment_id>/replies \
  -f body='Acknowledged, not fixing — <rationale with file:line evidence>.'
```

**Reply to summary-only findings** (the ones with no inline thread) with a top-level comment that names which finding it
answers:

```bash
gh pr comment <N> --repo <slug> --body 'Fixed the P2 "<finding title>" from the Greptile summary in <sha>: <what changed>.'
```

**Mark real review threads resolved** (REST replies don't flip the resolved flag — that's a GraphQL mutation). List
thread ids, then resolve each one you've answered:

```bash
gh api graphql -f query='
query($owner:String!,$repo:String!,$pr:Int!){
  repository(owner:$owner,name:$repo){ pullRequest(number:$pr){
    reviewThreads(first:100){
      pageInfo{ hasNextPage endCursor }
      nodes{ id isResolved
        comments(first:1){ nodes{ databaseId author{login} path line } } } } } }
}' -F owner=<owner> -F repo=<repo> -F pr=<N>

gh api graphql -f query='mutation($id:ID!){ resolveReviewThread(input:{threadId:$id}){ thread{ isResolved } } }' \
  -F id=<threadId>
```

Match each thread to the finding you handled by its first comment's `databaseId` — that is the same id you collected in
Step 4a, so the mapping is exact. Don't match on `author` + `path` alone: when one reviewer leaves several comments on
one file they collide, and you risk resolving the wrong thread while a real one stays open and blocks the merge. If
`reviewThreads.pageInfo.hasNextPage` is true, fetch the next page with `reviewThreads(first:100, after:<endCursor>)`
before you assume you have them all — the same first-page-only trap as the REST calls in Step 4.

Resolve a thread only after it is genuinely handled (fixed-and-pushed, or replied-with-rationale) — and resolving is
mandatory, not optional: an unresolved thread keeps the PR from merging even when the reply is already there. Leave a
thread open **only** when you are deferring to the user and they haven't decided yet — and say so explicitly in the
reply, since you are knowingly leaving the merge blocked until they answer.

## Step 9 — Update the branch from `main` when it's behind

A PR can be fully reviewed, every thread resolved, CI green — and still not merge because the branch has fallen behind
`main` and branch protection requires the head to be up to date. Reviewing the PR includes clearing that, so the review
actually lands. Check the merge state, and if it's behind, update the branch from `main`:

```bash
gh pr view <N> --repo <slug> --json mergeStateStatus,mergeable -q '{mergeState: .mergeStateStatus, mergeable}'
```

- **`BEHIND`** — the branch is out of date with `main`. Fast-path it with GitHub's own update (merges the base into the
  head, no local checkout needed):

  ```bash
  gh pr update-branch <N> --repo <slug>
  ```

  If that reports a conflict, the branch is effectively `DIRTY` at that moment, so the same consent gate applies: **stop
  and ask the user** before resolving anything locally — don't auto-resolve and push, that's a silent change to what
  lands. Only when they ask you to, switch to the PR branch first (you may still be on `main` if Step 7 applied no
  fixes), merge `main`, resolve, re-run the gate, and push:

  ```bash
  git switch <headRefName>                         # never merge into main — Step 7 may not have switched you
  git fetch origin main && git merge origin/main   # resolve conflicts, then cargo fmt/clippy/test + markdown lint
  git push
  ```

- **`BLOCKED`** — usually just pending CI or an unresolved thread, *not* staleness; don't update-branch to "fix" it.
  Confirm with `gh pr checks <N>` and the Step 8 thread list. Auto-merge stays armed and lands it when the blocker
  clears, so a `BLOCKED`-on-pending-CI PR needs no action beyond what you've already done.
- **`DIRTY`** — the branch has merge conflicts with `main`. Flag this in the report, do not claim that no update was
  needed, and do not attempt an automated update unless the user asks you to resolve the conflicts locally and re-run
  the gate.
- **`CLEAN` / `HAS_HOOKS`** — nothing to do; it's mergeable.
- **`UNKNOWN` / `UNSTABLE`** — GitHub hasn't finished computing mergeability (`UNKNOWN`), or the branch is mergeable but
  a non-required check is failing/pending (`UNSTABLE`). Neither is staleness — don't update-branch. Re-query after a
  moment for `UNKNOWN`; for `UNSTABLE`, confirm with `gh pr checks <N>` and leave auto-merge to land it once green.

Only update a branch you're reviewing — never force-push, and never update someone else's branch out from under them
without saying so in the report. After an update, CI re-runs; auto-merge (if enabled) still lands it when green.

## Step 10 — Report

Summarize for the user: your independent findings, every comment and its verdict (valid / invalid / won't-fix), what was
fixed (with commit shas), whether the branch needed updating from `main`, and confirmation that every thread now has a
reply and the handled ones are resolved. Call out anything still open and why.

## Boundaries

- **Comment resolution is non-negotiable.** Never end a review with an unanswered reviewer/bot comment. Reply to every
  one *and* mark every handled thread resolved — a reply without resolution still blocks auto-merge, so both are
  required to actually land the PR.
- **Don't rubber-stamp the bots.** Every bot finding is adjudicated against the real code before it earns a reply —
  false positives get a reasoned refutation, not a fix.
- **The user decides what lands.** Recommend, then ask, before applying a fix. No silent fixes, no silent skips.
- **A bare "review PR" resolves itself.** With no number, default to the current branch's PR — don't ask which one
  unless the branch has no PR or maps to several. The branch is the answer.
- **Updating from `main` is part of the review.** When the branch is `BEHIND`, update it from `main` so the merge isn't
  blocked on staleness — but only ever the branch under review, never with a force-push.
- **Reviews, doesn't open PRs.** Turning a dirty tree into a PR is [[create-pr]]; shipping to prod is [[power-push]].
  This skill operates on an existing PR.
- **Honors the gate.** Any fix you push clears `fmt` + `clippy` + `test` (+ markdown lint) and ships with its covering
  test, same as every other committing flow.
