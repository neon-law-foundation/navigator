---
name: fix-checks
description: >
  Keep an open pull request green — the review loop's two inputs, handled as the two different tasks they are: a
  **failed check** (read the log, find the first actionable failure, smallest root-cause fix) and an **inline review
  comment** (a reviewer on a specific line; adjudicate from evidence, fix only what it asked). This is action 5 of
  [`docs/agent-workflows.md`](../../../docs/agent-workflows.md) plus the mechanics of action 4. Trigger when a PR has a
  red check, when CI failed, when asked to "fix the build", "get this green", "address the review", or when a reviewing
  agent (Greptile, CodeQL, Codesmith) leaves findings. Covers both repository shapes: the Rust workspace and a Project
  repository, whose React applications share the loop and none of the Rust gates. For a single named comment on a Rust
  PR, `review-pr` is the narrower action.
---

# `/fix-checks` — keep the pull request green

The authoritative procedures are `Address a failed GitHub Action` and `Address a PR comment`, both in
[`agent-workflows`](../../../docs/agent-workflows.md). Read the relevant one before acting and keep durable detail
there. This skill is the dispatcher over both, plus what changes in a Project repository.

## The two inputs are not the same task

Collapsing them is the most common way this loop goes wrong — a CI repair bundled with reviewer comments produces a diff
nobody can review and a root cause nobody confirmed.

| | Failed check | Inline review comment |
| --- | --- | --- |
| Finding is | A machine's, about the whole branch | A reviewer's, about a specific line |
| First move | Read the log to the **first** actionable failure | Read the thread and the code at the PR head |
| Judgment | Was this the cause, or a symptom? | Is the claim valid, invalid, or won't-fix? |
| Scope | The smallest root-cause fix | Only what that comment asked for |
| Closing | Push; the check re-runs | Reply with the proof, resolve only that thread |

Never expand from one to the other in the same commit. Report the other findings; do not silently fix them.

## Orient first

Navigator and its sibling repositories live on github.com, in the `neon-law-foundation` organization. `gh` defaults to
that host, so no `GH_HOST` and no `--hostname` is needed. The posture those repositories carry — issues in Linear,
squash-only behind a signed ruleset — is [[public-repositories]].

```bash
gh repo view --json nameWithOwner -q .nameWithOwner
gh pr view --json number -q .number
gh pr checks <N> --repo <slug>
```

The `gh` API rate limit is shared across every agent working this workspace, and exhausting it kills `gh` for all of
them. Do not poll a running check in a tight loop — wait on the order of five minutes between checks.

## A failed check

1. **List the checks** and find the failed job. A cancelled job is usually a symptom of another job's failure.
2. **Read the failing step, not the summary:**

   ```bash
   gh run view <run-id> --log-failed
   ```

   The first actionable error is the lead. Later cancellations and cascaded errors are symptoms until proven otherwise.
3. **Read the workflow step, the source it invokes, and its covering test.** A check fails for a reason that lives in
   the repository; find it before editing.
4. **Reproduce the exact command locally** when the environment permits. In the Rust workspace that means the real gate
   with the right `.devx/env` sourced; in a Project repository it is that application's own script.
5. **Make the smallest root-cause fix.** Do not bundle reviewer comments, dependency refreshes, broad formatting, or
   unrelated warnings into a CI repair.
6. **Add the covering test** when the failure exposed a behavior gap — not to restate an infrastructure outage.
7. **Push and report** the root cause, the minimum fix, the local proof, and any unrelated failing check you left alone.

### Before assuming the code is wrong

A red check is not always a defect in the diff, and treating a known-flaky or environmental failure as a code bug wastes
the round trip. Check whether the failure is:

- **A flake with a known signature.** Re-run the job once and say you did; an unexplained re-run is how a real defect
  gets buried.
- **A stale base.** A branch opened from an old `main` can stall on a check it never reports. Fetch and rebase onto
  `origin/main`, signed, then push again.
- **An earlier gate masking a later one.** A validation job that fails first can hide the failure that actually
  matters, so re-read the checks after the first fix rather than assuming the branch is now green.
- **A missing registration rather than broken code.** In the Rust workspace, a new workspace member, CLI subcommand, or
  migration each has a companion file that must move with it. In a Project repository, the equivalent is the registry
  row for the application, and the pinned gate release.

## An inline review comment

Follow `Address a PR comment` in [`agent-workflows`](../../../docs/agent-workflows.md) — it carries the full `gh api`
and GraphQL mechanics for reading threads, replying, and resolving. The judgment it asks for is worth restating:

**Adjudicate before editing.** Every finding is valid, invalid, or valid-but-not-worth-changing, and each verdict needs
`file:line` evidence. A reviewing agent's finding is a claim, not an instruction — Greptile, CodeQL, and Codesmith all
produce confident false positives, and applying one uncritically ships a worse change than the one it flagged. Reply to
an invalid finding with the evidence and change nothing.

**Reply, then resolve only the thread you handled.** REST replies do not resolve a thread; resolution is a separate
GraphQL mutation. Leave every thread you did not address open.

Posting a comment can cancel an in-flight check run, so re-read the checks after a reply round rather than trusting the
state you saw before it.

## In a Project repository

A Project repository shares this loop and almost none of the Rust gates. Nothing about Cargo, clippy, nextest, KIND, or
`worktree-env` applies to its `applications/` tree — there is no Rust there, and Node never enters the Navigator
workspace. It does hold two kinds of source, gated differently: a change under `applications/` runs the application gate
and a change under `templates/` runs the notation validator, so a green check on one does not prove the other ran. See
[`vibe-react`](../vibe-react/SKILL.md) for what the repository actually contracts to.

Reproduce a failure with that repository's own scripts:

```bash
pnpm install --frozen-lockfile && pnpm lint && pnpm typecheck && pnpm test && pnpm build
```

The failures that are specific to this shape, and what each one actually means:

- **The mount gate refused the build.** The gate compares the base derived from the repository name plus the literal
  `portal` against the built `portal/dist/index.html`. The fix is almost never to rename the repository — the name *is*
  the Project code. It is that `base` did not reach the build.
- **An absolute path in source escapes the mount.** The gate refuses `href`, `src`, or `to` strings starting with `/`
  that are not under the mount, because a Vite base does not rewrite a path you wrote by hand. Build it from
  `import.meta.env.BASE_URL`.
- **The gate is pinned to the wrong release.** It is consumed as
  `neon-law-foundation/navigator/.github/actions/validate@YY.M.D` with `project_repository: true`, at an exact Navigator release
  tag — never `main` or `latest`. A gate change in a new Navigator release requires bumping that reference deliberately.
- **A screen that works at `/` and breaks under the mount.** Not visible to any test that does not load the built
  bundle. Verify in a browser at the base path.
- **A client-data finding.** Fixtures and copy must be synthetic or firm-owned, with non-firm email addresses on a
  reserved example domain. This one is never argued down.

## Linear stays in step

Planning is Linear only, and the merge does the transition. Do not hand-transition an issue a merge should have moved,
and do not close a Linear issue because CI went green.

When a review round changes what the work *is* — a rejected approach, a constraint nobody knew about, a fix that
revealed a second concern — leave a dated comment on the Linear issue saying what changed and why. The description holds
current truth; comments hold the trail. A second concern found in review is its own issue, not a wider diff.

## Report

Say what failed, the root cause, the minimum fix, the proof you ran, which threads you replied to and resolved, and
every finding or failing check you deliberately left untouched.
