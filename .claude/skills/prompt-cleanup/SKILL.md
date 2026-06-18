---
name: prompt-cleanup
description: >
  Reconcile the gitignored `prompts/` drafts against the git history — review every kickoff prompt, judge which phases
  have actually shipped (the commit log is the source of truth, not the prompt's own optimism), delete the prompts whose
  work is fully landed, and recommend which prompt to run next based on the ordering and dependencies the prompts
  themselves declare. Handles multi-prompt files (one file holding "Prompt 1 / Prompt 2 / …") by striking only the
  finished prompts and keeping the file when work remains. Deletes a prompt's paired `*-council-review.md` alongside it.
  Trigger when the user says "prompt cleanup", "clean up prompts", "prune the prompts folder", "which prompt is next",
  "what should I work on next", or after shipping a chunk of work that one of the prompts described. Conservative by
  default: when completion is ambiguous, it reports and asks rather than deleting.
---

# prompt-cleanup

Keep `prompts/` honest. The directory is the firm's local backlog of self-contained kickoff briefs (gitignored — see
CLAUDE.md "Local-only convention: `prompts/`"). Work gets done and committed, but the prompt that kicked it off lingers.
This skill reconciles the backlog against `git log`, retires what's finished, and tells the user what to pick up next.

Three moves, in order: **review → delete finished → recommend next.** Never delete before reviewing, never recommend
before deleting (a stale "done" prompt pollutes the recommendation).

## When to invoke

- The user says "prompt cleanup", "clean up the prompts", "prune prompts", or asks "which prompt should I do next /
  what's next".
- Right after a `power-push` or a run of commits that completed something a prompt described — the backlog is now stale.
- Periodically, as backlog hygiene.

Skip if `prompts/` is empty or absent (`ls prompts/` → nothing) — say so and stop.

## Step 1 — Review

Read the whole backlog and the recent history together.

```bash
ls -la prompts/
git -C . log --oneline -60          # widen with -120 if a prompt describes long-running work
```

Read **every** file in `prompts/` in full (they are short). For each, extract:

- **The done-criteria.** Most briefs state phases ("Phase 0 / Phase A / Prompt 1…") with explicit done-criteria or a
  "what ships" list. Those criteria — not the topic — are what you check against the log.
- **Declared ordering / dependencies.** Many files say "in order", "value-first", "Phase 0 first", or name a
  prerequisite. Capture it; it drives Step 3.
- **Multi-prompt files.** A single file (e.g. `notation-roadmap-prompts.md`) can hold several independent prompts under
  `## Prompt N` headings. Treat each prompt as its own unit of completion.
- **Paired council reviews.** Per the firm convention, a brief may have a sibling `<slug>-council-review.md`
  (gitignored draft of a `/council` pass). It lives and dies with its prompt.

## Step 2 — Judge completion against the log

The **commit log is the source of truth.** A prompt is *finished* only when every one of its done-criteria maps to
landed commits — match by the files, traits, routes, and behaviors the prompt names, against commit subjects and, when a
subject is ambiguous, the actual diff:

```bash
git log --oneline --all -- <path/the/prompt/names>     # did the files it targets get touched?
git log -p -1 <sha>                                    # confirm a suspicious commit really does what the prompt asked
git log --grep '<keyword from the done-criteria>' --oneline
```

Classify each prompt (or each `## Prompt N` within a multi-prompt file) into exactly one bucket:

- **DONE** — every phase/done-criterion is represented by landed commits. Eligible for deletion.
- **PARTIAL** — some phases shipped, others have no commits. Keep the file; note exactly which phases are done so the
  user can trim the brief.
- **NOT STARTED** — no matching commits. Keep.

Be conservative. The brief's own phase labels (a prompt that *plans* Phase 0–2) are not evidence those phases shipped —
only commits are. If a done-criterion is fuzzy or you can't tie it to a commit with confidence, classify **PARTIAL or
NOT STARTED, not DONE.** When genuinely uncertain whether something is finished, surface it and ask — do not delete on a
guess.

## Step 3 — Delete the finished prompts

Only after the review is presented. Deletion is sanctioned by CLAUDE.md — a finished prompt's durable value is already
in the code/docs; the brief is meant to be discarded. The user asked for this, so deleting DONE prompts does not need a
re-confirm; deleting anything you marked ambiguous does.

- Delete each **DONE** file and its paired `*-council-review.md` together:

  ```bash
  rm prompts/<finished>.md prompts/<finished>-council-review.md   # omit the pair arg if none exists
  ```

- For a multi-prompt file with **some** prompts DONE and others not: do **not** `rm` the file. Edit out the finished
  `## Prompt N` sections (and update any "in order" numbering/intro), leaving the unfinished prompts intact.
- Leave **PARTIAL** and **NOT STARTED** files untouched on disk (you may note suggested trims, but let the user decide).

`prompts/` is gitignored, so there is nothing to commit — the deletions are purely local. Do **not** `git add` or commit
anything here.

## Step 4 — Recommend what's next

From what remains (PARTIAL + NOT STARTED), recommend the next prompt to run. Rank by, in order:

1. **Declared dependency order** — if a prompt says "do X first" or is "Prompt 2 of an ordered list", respect it; don't
   recommend a prompt whose prerequisite hasn't shipped.
2. **Unblocks the most / value-first** — the firm's standing bias (see the briefs' own "value-first" framing and
   `web/content/marketing/mission.md`): prefer the prompt that removes a production dead-end or ships client-facing
   value.
3. **Smallest finish-line** — a PARTIAL prompt with one phase left often beats starting a fresh brief.

Name the single best next prompt and one runner-up, each with a one-line why and its first concrete step. If two are
genuinely independent and equal, say so rather than inventing a ranking.

## Output shape

Render a compact report — no preamble:

```text
Reviewed N prompts against the last M commits.

DONE (deleted)
  • esignature-e2e-and-production.md — Phases 0–2 all landed (a1e7273, 3fe6ed7, …)

PARTIAL (kept)
  • notation-roadmap-prompts.md — Prompt 1 shipped (df90ac2); Prompts 2–4 open

NOT STARTED (kept)
  • xero-integration.md
  • northstar-estate-flow.md

Next up →  northstar-estate-flow.md
  Why: client-facing value, no unshipped prerequisite. First step: write the Phase A comment-only review .feature.
Runner-up →  notation-roadmap-prompts.md (Prompt 2)
```

Keep the evidence (commit SHAs) attached to every DONE claim so the user can audit the deletions.
