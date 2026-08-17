---
name: projects-outline
description: >
  Render the Linear portfolio as a Harvard outline — projects as headings, their issues as subheadings, one sentence
  each. Trigger when the user says "/projects-outline", "outline the projects", "show me the portfolio", "what's in
  Linear", or asks for a readable map of every project and issue. This is a READ-ONLY rendering: it never writes to
  Linear. To reconcile Linear against `main` use `triage-projects`; to write into Linear use `author-linear-issue`.
---

# Render the portfolio as a Harvard outline

One document, ordered so a reader can see the whole programme at a glance: every project as a heading, its issues
beneath it, one sentence apiece. The value is in the shape — which projects carry no issues, which carry one, where
Urgent work is parked in Backlog — so the ordering and the completeness matter more than the prose.

**This skill writes nothing.** Statuses are reported as found. If the outline surfaces something that should change in
Linear, say so at the end as an observation; applying it is `triage-projects` followed by `author-linear-issue`.

## 1. Pull the portfolio

Two calls, both paged to exhaustion — a truncated pull silently drops projects and reads as a complete map.

```text
list_projects  fields: ["id","name","status","lead","initiatives","priority","targetDate","updatedAt","teams"]
list_issues    fields: ["id","title","status","statusType","project","priority","assignee","gitBranchName","updatedAt"]
```

Page until `hasNextPage` is false. `list_projects` caps at 50 per page; `list_issues` at 250.

Do **not** request `description` on issues. The outline needs one sentence per issue, and issue bodies in this workspace
run to hundreds of lines each — requesting them turns a 70 KB response into one that cannot be read at all.

### Two failure modes, both hit in practice

**The issue response will exceed the tool's token ceiling.** At ~180 issues the response is ~72 KB and the harness saves
it to a file instead of returning it. That is expected, not an error. Parse the file rather than re-requesting with a
narrower window, which would drop issues from the outline:

```bash
python3 -c "
import json, collections
d = json.load(open('<saved-path>')); iss = d['issues']
print(d.get('hasNextPage'), len(iss))
"
```

**`project`, `assignee`, `status`, and `priority` come back as flat strings on `list_issues`, not objects.**
Subscripting one as a dict to read a `name` key yields nothing, and every issue then silently reports as having no
project — which reads as a real and alarming finding. It is not. Normalize before grouping:

```python
def s(v):
    return v if isinstance(v, str) else (v.get('name') if isinstance(v, dict) else '')
```

`get_issue` on a single issue returns the same fields, also as flat strings. Confirm against one known-good issue before
reporting that a field is empty across the portfolio.

## 2. Group and order

Group issues by project name. Then:

- **Omit `canceled` and `duplicate` issues.** They are noise in a map of current work — in this workspace that drops the
  four Linear onboarding placeholders along with genuinely retired work. State how many you dropped so the reader knows
  the outline is filtered rather than short.
- **Order projects by status: In Progress first, then Backlog, then Canceled/Completed if included at all.** Within a
  status band, order by issue count descending — the biggest live programme should be first.
- **Order issues within a project by numeric identifier**, not by status. Many projects here number their issues
  `01 —`, `02 —` in dependency order, and that sequence is information the outline should preserve. Sort on the integer
  after the team prefix, or `ENG-100` sorts before `ENG-20`.
- **Keep projects with zero issues.** An empty project is one of the most useful things the outline surfaces.

## 3. Write it

Harvard levels: Roman numerals for projects, capital letters for issues. Deeper levels are not needed — sub-issues are
rare enough to fold into the parent's sentence.

```markdown
**I. Project name** *(In Progress, High)*
- A. **ENG-18** *(Done)* — Provisioned a Surreal endpoint for both Neon Law deployments.
- B. **ENG-22** *(Backlog, High)* — Retires `DATABASE_URL`, the migration chain, and the Postgres testcontainer.
```

Rules for the sentence:

- **Expand the issue's own title; do not invent specifics.** A title is the only grounding you have for most issues, and
  a confidently wrong description is worse than a plain restatement because nobody re-checks it. If you read the full
  body during this session, use that detail and it will show — that asymmetry is fine and honest.
- **Present tense for open work, past tense for Done.** "Retires the product catalog" versus "Ported entities onto
  SurrealDB." The tense alone tells the reader the state before they reach the tag.
- **One sentence. No trailing clause about what it depends on**, unless the dependency is the point of the issue.
- Carry the status and priority in the tag, not the sentence. Omit priority when it is `No priority`.
- Where an issue was verified against source in this session, a short clause naming the file earns its place —
  "which today is only 57 lines of thin binary" is worth more than the title alone.

## 4. Close with what the shape reveals

Two or three observations the outline makes visible and a flat issue list does not. Look for:

- **Projects with no issues.** Scope that exists as an intention and nowhere else.
- **Projects with one issue, already Done**, still marked In Progress — finished, or missing their remaining work.
- **Urgent issues sitting in Backlog projects**, and Backlog issues under In Progress projects.
- **Projects whose status contradicts their issues** — a Backlog project with an In Progress issue and an open PR.
- **Whole initiatives with no target date**, if that is true of every project in the outline.

Do not turn this into a triage. Name what the shape shows, in a few sentences, and stop.

## Related

- [`triage-projects`](../triage-projects/SKILL.md) — reconciles Linear against merged PRs on `main`; use it when the
  question is *what drifted*, not *what exists*.
- [`author-linear-issue`](../author-linear-issue/SKILL.md) — the only skill that writes into Linear.
