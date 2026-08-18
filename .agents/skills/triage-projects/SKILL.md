---
name: triage-projects
description: >
  Portfolio-level Linear triage grounded in two databases of record: the Linear workspace (planning truth) and the
  single `main` branch (shipped truth). Pull every project, initiative, and recently updated issue from Linear, map
  merged PRs to issues through Linear's GitHub linkage first and corroborated repository evidence second, and report
  drift both ways — issues shipped on `main` but still open, closed issues with no landed evidence, merged PRs no issue
  tracks. Then report inconsistencies, a health check, the direction the priorities actually encode, and which ready
  issues can run as parallel worktree lanes. Trigger when the user asks to "triage projects", "triage Linear", "what's
  changed since", "what's closed", "health check the backlog", "are Linear and main in sync", or "what can we do in
  parallel". For one specific issue use `triage-issue`; to write a new issue or rewrite a wrong one use
  `author-linear-issue`.
---

# `/triage-projects` — portfolio triage grounded in Linear and `main`

Triage is a reconciliation, not a reading. Linear records what the team believes; `main` records what actually shipped.
Every finding in the report must cite one of those two sources — never memory, never a stale export, never a diff alone.
The deliverable is the report; Linear mutations are shared team state and happen only after the user picks. Write the
ones they pick with [`author-linear-issue`](../author-linear-issue/SKILL.md) — an issue body must be grounded in the
source it describes, not in this report alone.

## 1. Ground in the Linear database

Pull the live state through the Linear MCP tools, paging every listing until `hasNextPage` is false:

- `list_teams`, `list_initiatives` (request `health` and `status`), and `list_projects` (request `status`, `lead`,
  `initiatives`, `priority`, `targetDate`, `updatedAt`).
- `list_issues` with an `updatedAt` window and fields `title`, `status`, `statusType`, `project`, `completedAt`,
  `assignee`, `priority`, `team`. Default the window to `-P14D`; when the user names a boundary ("since Friday", "since
  the last triage"), use that instead. A truncated pull silently hides drift, so page to the end before concluding
  anything.

## 2. Ground in `main`

One branch ships everything, so the merge history of `main` is the complete shipped record:

```bash
git fetch origin main && git log --oneline origin/main | head -40
```

For every repository in scope, list its merged pull requests. Derive `<owner>/<repo>` from that checkout's `origin`; do
not silently assume that every connected repository is `navigator`.

```bash
gh pr list -R <owner>/<repo> --state merged \
    --json number,title,headRefName,body,url,mergedAt --limit 100
```

Correlate each PR before drawing a drift conclusion, in this order:

1. Call Linear's `get_diff` with the PR `url`. A returned linked issue is canonical; record the Linear issue id, PR
   number, repository, and merge date.
2. If no Linear diff resolves, inspect the PR body for an issue reference, then fetch that issue. Branch names and PR
   titles that contain `ENG-NN` are candidates, not proof.
3. Corroborate a candidate with the issue's `gitBranchName` or another explicit Linear/GitHub link. If the branch does
   not match, report it as an ambiguous reference rather than attributing the merge to the issue.
4. Only after all three checks fail may the PR be called **untracked**. A PR without an issue id in its branch name is
   not untracked by itself; integrations and closing keywords routinely carry the association elsewhere.

If Linear does not expose a diff for a connected repository, say so explicitly and retain the fallback's confidence
level in the report. In-flight work is visible too:

```bash
git worktree list --porcelain
```

A locked task worktree on an `eng-NN` branch is live WIP whatever Linear says.

## 3. Reconcile shipped against believed

Walk the mapping in both directions and table every mismatch:

- A correlated merged PR names an issue that is not `completed` → **stale-open drift**. Cite the PR number, repository,
  merge date, and correlation source (Linear diff, body, or corroborated branch).
- An issue is `completed` or `canceled` → confirm a merged PR or an explicit rationale backs it.
- A PR with no Linear diff, issue-body reference, or corroborated branch/title reference → **untracked work**. List
  its failed correlation checks; recurring untracked merges mean planning is happening outside the database.
- An issue's scope is only partly covered by the merged PR → say which acceptance criteria remain, not just "open".

## 4. Inconsistencies, health, and direction

Check each of these because each one misleads a future triage if left standing:

- Project `status` contradicting its issues (a Backlog project with completed issues and merged PRs is In Progress in
  fact).
- Projects with no lead, no initiative link, or no target date; initiatives with no health or status updates.
- Invisible WIP: active worktrees or assigned agents against issues still marked Backlog.
- Priority inflation: count the open Urgent issues; a solo team cannot have ten urgent things.
- Duplicates and overlaps (two issues describing one change), onboarding placeholder issues, and doc drift between the
  repo's workflow docs and where planning actually lives.

Close with a health snapshot (open/completed/canceled counts, WIP, staleness) and state the direction the open
priorities actually encode — then name where that disagrees with the documented roadmap, if it does.

## 5. Propose parallel lanes

Group the ready issues into lanes a separate worktree can each carry, split by blast radius: two lanes may run in
parallel only when their file and crate footprints do not overlap. Name the collision when they do (two projects both
rewriting routing must serialize). One worktree per lane, per [`CLAUDE.md`](../../../CLAUDE.md).

## Report structure

Use this shape so consecutive triages compare cleanly:

```markdown
## Drift (shipped vs believed)   — table: issue, status, evidence, proposed action
## Untracked merges              — PRs with no correlation after the full lookup
## Correlation gaps               — repositories or PRs where Linear linkage was unavailable or ambiguous
## Inconsistencies               — bullets, each with its citation
## Health                        — counts, WIP, staleness, priority load
## Direction                     — what the priorities encode; disagreements with docs
## Parallel lanes                — lane → issues → blast radius → collisions
## Proposed Linear mutations     — numbered; apply only the ones the user picks
```
