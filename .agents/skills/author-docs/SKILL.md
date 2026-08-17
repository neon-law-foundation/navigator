---
name: author-docs
description: >
  Audit every teaching surface — docs, inline comments, tests, and workshops — against the code the current branch
  changed, and write one report to `/tmp`. Trigger on a bare `/author-docs`, "check the docs against my changes", "did
  this branch make anything stale", or before opening a PR that renames or removes a public identifier — and it runs
  automatically as an advisory step inside `/create-pr`. One job, no flags: it reads the changed surfaces and writes a
  report; a human makes every edit and runs every gate. Reach for it when code changed and a teaching surface might now
  describe the past.
---

# Keeping the teaching surfaces in sync with the code

This repository explains the same concepts in five registers, and one of them is the truth:

- **the code** — the source of truth;
- **inline comments** — rustdoc `///` and `//`, the concept explained beside the code;
- **documentation** — `docs/*.md`, the concept explained to a reader;
- **tests** — the concept explained as an executable expectation;
- **workshops** — the decks in `server/content/workshops/`, the concept explained to a room.

The four teaching surfaces follow the code. **Drift** is a surface that describes something the code no longer does.
This skill reads the surfaces the branch touched, points at the drift, and writes one report; a human reads it and
decides each fix.

## Present tense, positive voice

Every surface earns its place by describing the present: the current rule, the live behavior, the shape that exists
today. So the skill flags **vestigial narration** — a comment, doc sentence, or test that explains a decision the repo
has already left behind ("used to", "no longer", "formerly", "replaced by") — as its own kind of drift. The fix is
usually to delete it: git history already holds the past, and a test that guards against a choice no one can make
anymore teaches a decision that stopped mattering. A test asserting a live invariant describes the present and stays.

## Citations live in the report

The report points at both sides so a human can verify. The docs themselves stay clean prose, because a citation pinned
into prose rots on the next edit. Where two surfaces must stay in sync, the durable tool is a **grounding test** that
fails when they diverge: the concept written twice, one copy keeping the other honest. Tests are the one surface that
both teaches and enforces, which is why the standing preference here is a guard over a corrected sentence.

## What it produces

One report under `/tmp`, built entirely from reads — `git` reads and file reads. Editing, fetching, rebasing, and
running the gate stay with the human, afterward. It complements `navigator validate`, the one Markdown gate, and
recommends running it.

## Invocation

`/author-docs` — or plain English: "check the docs against my changes", "did this branch make anything stale?". One job,
no flags: audit the surfaces the current branch changed and write one report. It also runs on its own as the advisory
drift step inside `/create-pr`. When the diff is empty, that is the answer: write the zero-finding report and stop.

## Step 1: confirm a task worktree

Run `git worktree list --porcelain` and match `pwd -P` to a **non-primary** `worktree` entry — the precondition
`AGENTS.md` → *Worktree-first code changes* puts on every code change, so the audit reads the branch's own tree. If it
does not match, say: **"This task was not started in a New Worktree. Please click New Worktree and start it again."**

## Step 2: read the base as it stands

The base is the **local `origin/main` ref as it already exists in this checkout** — fetching and rebasing belong to the
human (`AGENTS.md` → *Worktree-first code changes*). Report how far the branch point trails that ref, and note it
reflects the last local fetch, so its freshness is the author's to confirm. When the branch point is behind, add one
caveat to every finding: a rebase may change it.

## Step 3: read in order, across the four surfaces

`AGENTS.md` → *Ground every action* fixes the reading order: glossary, then the narrowest doc, then the code, then the
covering tests. Check each surface the branch touched:

- **Documentation** — `docs/*.md`; top-level pages carrying `publish: true` are public.
- **Inline comments** — rustdoc `///` and ordinary `//`. The Markdown gate scans `*.md` only and never opens Rust
  source, so these comments are the audit's to check — a reason to run inside `/create-pr`.
- **Tests** — where behavior is asserted; a test name or comment describing an old contract is drift, and a behavior
  with no covering test is a missing guard (Step 8).
- **Workshops** — the decks in `server/content/workshops/`. The loader's snippet-grounding test already pins code
  slides to the file they cite, so those fail the build on drift; check the prose slides, which it leaves to you.

Read `.agents/skills/`, `.claude/skills/`, and `.codex/skills/` as inputs only.

## Step 4: find candidates from what the diff removed

Drive the search from the diff. Extract what the branch removed or moved — workspace members, removed `pub` items,
schema fields, routes, question types, deleted files — and search each with `git grep -F -n` across the tracked sources:
`*.md`, Rust comment lines, test names, and `server/content/workshops/`. The needle is the thing that disappeared, so
every hit has a reason to exist before it is read. A token is a candidate because something concrete stopped existing,
not because it looks like a path.

## Step 5: resolve, with the code leading

- **R1 — the code leads; every surface follows.** When a surface describes something the code lacks, the surface is
  what changes.
- **R2 — declared duplication is checked, kept in step.** Some facts have several homes on purpose (the coverage floor,
  for one); the job is to keep the set consistent.
- **R3 — the narrowest surface owns the detail.** A subsystem fact lives in its subsystem doc; find the owner through
  `docs/index.md`.

## Step 6: assign one of three verdicts

- **Confirmed drift** — a surface contradicts a code fact and the successor is verified. Propose the minimum fix, and
  prefer a grounding test where the drift can recur.
- **Ambiguous** — a contradiction suspected but not settled from the tree. Escalate for a human to settle.
- **Not drift** — looks stale, reads correct. Record it with the reason so the next run has the answer.

Escalate to **ambiguous** — however clear the fix looks — when the fix is more than a substitution, when no live caller
settles the target, or when the finding touches legal copy (`templates/`, a questionnaire prompt, an engagement letter),
an architecture invariant, or a user-facing string. Choosing the right home inside a declared multi-home set is a
decision too.

## Step 7: point at both sides, by name

Every finding names the **claim** (the sentence, comment, test, or slide, quoted, and where it lives) and the **fact**
(what the code does, and where). Anchor each pointer to a stable name — a symbol, a rule code, a test name, a heading —
and treat a line number as a hint. These pointers live in the `/tmp` report; the docs stay as they are. A finding about
something absent records the search that looked for it, with the command and the hit count. A finding names both sides
to enter the report.

## Step 8: write exactly one report

Write to `/tmp/navigator-author-docs/<branch>-<base-sha7>.md`. **Every completed run writes a report, including a run
that finds nothing** — it records that the check happened and what it covered, so "checked, found nothing" stays
distinguishable from "never ran". Sections, in order: **Scope** (base ref, its age, the freshness note, diff stat);
**Coverage** (which surfaces were searched); **Confirmed drift**; **Ambiguous** (each with the question a human decides
and its route); **Not drift** (with reasons); **Missing guards** (recurrable drift that should land as a grounding
test); and a closing line stating that nothing was changed.

## Step 9: print two lines

Terminal output is the verdict counts and the report path:

```text
author-docs: 3 confirmed, 2 ambiguous, 4 not-drift
report: /tmp/navigator-author-docs/<branch>-<sha7>.md
```

## Where a human decides

The skill names the smallest useful bench and leaves convening it to a person, per `AGENTS.md` → *Cross-cutting rules*.
Routing follows the councils in [`docs/agent-decision-councils.md`](../../../docs/agent-decision-councils.md):

- `templates/`, questionnaire prompts, or legal copy → **Legal Council**.
- Architecture invariants, glossary definitions, or doc clarity → **Engineering Council**.
- Marketing pages, portal UI, or visitor-facing strings → **Client Council**.
- `LICENSE` → left to its owners.

## What to run after you decide

Print these into the report for the human to run. The skill leaves `navigator validate` to the human, so it reports the
Markdown lint state as unknown. A bare `validate .` walks the tree and skips dot-directories; a changed `.md` inside one
— a skill file under `.agents/`, say — is linted only when named by its own explicit path.

```bash
cargo run -p cli --quiet -- validate .
```

Then the guard covering the finding's subject, if one exists — and, when the approved edit touches Rust or runtime
configuration, the full workspace gate from `AGENTS.md` → *Create a pull request*.

## Two habits that keep this honest

- **Report zero when nothing drifted.** A quiet run is a real result; record the coverage and stop.
- **Prefer a guard to a fix.** Drift that can recur lands as a grounding test — a corrected sentence decays, a guard
  holds. This is how the surfaces stay in step, with the citation kept to the report.
