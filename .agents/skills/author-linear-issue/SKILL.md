---
name: author-linear-issue
description: >
  Author or correct a Linear project or issue, grounded in this repository's source. Trigger when writing a new issue or
  project, rewriting one whose premise turned out wrong, or applying the Linear mutations a triage proposed — anything
  that puts words *into* Linear. Read the governing code first and cite `file:line`; never propose a frontmatter key,
  workflow step, rule code, or module that already exists. To sweep the portfolio use `triage-projects`; to triage one
  issue use `triage-issue`.
---

# Author Linear issues grounded in the workspace

A Linear issue is somebody's description of work they intended to do. This repository is what actually got built. Those
diverge constantly, and the divergence runs one way: the repo is almost always further along than the backlog says.
Writing from issue text alone produces work that is already done.

## Where this sits

Three skills cover Linear, split by direction:

| Skill | Direction | Grounds in |
| -- | -- | -- |
| `triage-projects` | Read the portfolio | Linear + merged PRs on `main` |
| `triage-issue` | Read and plan one issue | Linear + merged PRs + docs, source, tests |
| **This one** | **Write into Linear** | **The source that governs the subject** |

The triage skills ask *has this issue shipped* and answer it from merge history correlated through Linear's GitHub
linkage, then corroborated PR body and branch evidence. This one asks *does this thing exist* and answers it from the
source. They are complementary: an issue can be unshipped by every PR-based check and still propose building something
that is already in the tree under another name. Run both checks when authoring from a triage.

## The rule

Before writing or updating any Linear issue that touches this system, read the code that governs it and cite `file:line`
in the issue body. An issue that asserts something is missing must have looked.

## Why this exists

A PDF-rendering project was written from Linear and Drive alone. Five of eight issues were materially wrong:

| Proposed | Already in the repo |
| -- | -- |
| Add a `document_class:` frontmatter key | `kind:` — `kind.rs:1` calls it "the single frontmatter discriminator" |
| Add a `render` workflow step | `generate_pdf`, `StepStatus::Implemented`, in `rules/src/workflow_steps.rs` |
| Build pleading paper | `pdf/src/pleading.rs` — 28-line frame, 24pt grid, three calibrations, tested |
| Build letterhead | `pdf/src/format.rs` — `OutputFormat::Letter` with a brand-agnostic `Letterhead` |
| Add a font fallback stack | Fonts are embedded via `include_bytes!`; there is no host to fall back to |

Every one was answerable with a single grep. The cost was not wasted effort — it was a backlog that told a contributor
to build things that exist, and that shipped a licence recommendation based on a mechanism the crate does not use.

## Grounding checklist

Work down this list before writing the issue. Stop early only when the question is plainly outside the workspace.

1. **Does the thing already exist?** Search for the noun before proposing it.

   ```bash
   rg -n 'document_class|render_step|letterhead' --type rust
   ```

2. **Frontmatter keys** — the vocabularies are closed and each has a rule. Never propose a new key without reading all
   four:

   | Key | Source | Rule |
   | -- | -- | -- |
   | `kind:` | `rules/src/kind.rs` (`Kind::ALL`, `VALID`) | `S103` |
   | `output:` | `rules/src/f109.rs` (`VALID`) | `N109` |
   | `jurisdiction:` | `rules/src/f110.rs` (`JURISDICTIONS`) | `N110` |
   | `workflow:` steps | `rules/src/workflow_steps.rs` (`WORKFLOW_STEPS`) | `N104` |

3. **Rule codes** — `rules/src/lib.rs` maps every `N`/`S`/`M`/`E`/`C` code to its description. Read it before claiming
   a rule is missing, and take the next free code rather than inventing one.

4. **The renderer** — `pdf/src/` is `lib.rs` (Typst entry, embedded fonts), `format.rs` (page chrome), `pleading.rs`
   (court paper), `acroform.rs` (government forms), `markdown.rs`, `passage.rs`.

5. **Is it wired?** Existing code is not reachable code. A module can be complete and referenced nowhere:

   ```bash
   rg -n 'pleading::|Variant' --type rust | rg -v '^pdf/src/pleading.rs'
   ```

   An orphaned module changes an issue from "build it" to "wire it" — a different, much smaller piece of work.

6. **What do the templates actually declare?** Intent and practice diverge here too.

   ```bash
   rg -n '^kind:|^output:|^jurisdiction:' templates --type md | sort | uniq -c | sort -rn
   ```

   This is how the two `kind: letter` templates rendering without letterhead were found — the defect, not the theory.

## Writing the issue

House style is `## Observed` / `## Work` / `## Done when`, with titles prefixed `01 —`. Beyond that:

- **`## Observed` states what is in the repo, with `file:line`.** Not what you assume, and not a restatement of the
  request. If you did not read it, do not assert it.
- **Quote the source when it makes the argument.** The `kind.rs` doc comment is a better reason not to add a second
  discriminator than any paraphrase of it.
- **Separate verified from unverified.** "Whether `N104` enforces this is unverified" is useful. A confident wrong
  claim is worse than an open question, because nobody re-checks it.
- **Prefer extending the existing seam.** `format.rs:5` names its own extension point; `kind.rs` uses exhaustive
  `match` so a new variant fails to compile until every site declares where it falls. Design with those, not beside
  them.

## When the repo is not the working directory

Sessions often start in a Google Drive matter folder holding documents and no code. The source is at `~/Navigator`. A
question about notation schema, rule codes, or rendering is answerable there — read it rather than reasoning from Linear
text or from the documents in front of you.

## Correcting a wrong issue

Rewrite the issue body and say so in it: "This issue originally proposed X. That was wrong, and `file:line` says why."
Leaving a superseded premise in place costs a contributor the same hour it cost you. Fix the title too when the work
changed shape — "Build pleading paper" and "Wire the existing pleading geometry" brief entirely differently.
