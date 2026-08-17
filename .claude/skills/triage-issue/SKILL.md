---
name: triage-issue
description: >
  Triage one Linear issue — action 2 of the five codebase actions — grounded in the Linear database and the single
  `main` branch. Read the issue from its opening body through every comment, verify a merged PR has not already shipped
  it, reconcile the ask with the glossary, docs, source, and covering tests at `origin/main`, then comment a test-driven
  plan naming the minimum implementation and exact blast-radius files. Trigger when the user asks to "triage ENG-NN",
  "triage this issue", "is this issue still valid", "has this already shipped", or "plan this issue". For the whole
  portfolio at once use `triage-projects`; to write a new issue or rewrite a wrong one use `author-linear-issue`.
---

# `/triage-issue` — triage one Linear issue

The authoritative procedure is [`Triage an issue`](../../../docs/agent-workflows.md#triage-an-issue). This skill adds
the Linear grounding mechanics; keep durable detail in that doc. Triage ends at the plan comment — implementation is
action 3 and starts in its own New Worktree.

## 1. Read the whole issue

Fetch it with `get_issue` (pass `includeRelations: true`) and `list_comments`, and follow how the request evolved from
the opening body through the last comment. Relations matter: a blocking issue or a named duplicate changes the verdict
before any code is read.

## 2. Verify it has not already shipped

The single `main` branch is the shipped record, and stale-open drift is common — an issue can sit in Backlog with its
change already merged. Check before planning, not after:

```bash
git fetch origin main
```

```bash
GH_HOST=github.com gh pr list -R <owner>/<repo> --state merged \
    --json number,title,headRefName,body,url,mergedAt --limit 100
```

Derive `<owner>/<repo>` from the checkout's `origin`, and repeat for every repository the issue explicitly links. For
each candidate PR, call Linear's `get_diff` with its GitHub `url`: a linked issue returned there is the canonical
association. If no diff resolves, inspect the body for an issue reference; branch names and titles containing `ENG-NN`
are fallback candidates only. Fetch the candidate issue and corroborate it with `gitBranchName` or another explicit
Linear/GitHub link before treating the merge as evidence for this issue. Record when Linear exposes no diff so the
verdict does not overclaim a branch-name heuristic.

When no correlated PR matches, grep the merged titles and the relevant source on `origin/main` for the change itself.

A PR-based check proves no *issue* shipped the work. It does not prove the *thing* is absent — a closed vocabulary or a
complete-but-unwired module answers to no branch name. When the issue proposes new structure (a frontmatter key, a
workflow step, a rule code, a module), also run the source checks in
[`author-linear-issue`](../author-linear-issue/SKILL.md) before calling it valid.

## 3. Reconcile with the evidence

Read [`docs/glossary.md`](../../../docs/glossary.md), the narrowest doc from [`docs/index.md`](../../../docs/index.md),
the current source, and the covering tests at `origin/main`. Reproduce the current behavior where practical using the
local KIND loop. When an unknown still blocks a grounded scope, run the smallest throwaway Rust spike and record the
command, observation, and conclusion — a spike proves a fact; it is not the implementation.

## 4. Adjudicate

Every issue lands in exactly one verdict, each with its evidence:

- **Still valid** — proceed to the plan.
- **Already shipped** — propose completion, citing the merged PR, its correlation source, and what proves the behavior.
- **Duplicate or superseded** — name the surviving issue and propose cancellation.
- **Blocked on a decision** — name the decision, the decider, and the smallest spike that would unblock it.

## 5. Comment the test-driven plan

For a valid issue, post the plan on the Linear issue with `save_comment` so a future worktree starts grounded without
this conversation:

```markdown
## Triage plan
**Verdict:** still valid — <one-line grounding>
**Covering tests:** <the test(s) that land with the change>
**Minimum implementation:** <smallest change satisfying the evidence>
**Blast radius:** <exact files a worktree should touch>
**Collisions:** <in-flight lanes touching the same files, or "none">
```

Name real files and real tests — a plan that says "update the relevant handler" forces the next agent to re-triage.
