---
name: create-pr
description: >
  One command — `/create-pr` — that turns a pile of uncommitted working-tree changes into a clean, reviewable pull
  request against `main`. It reads every change (staged, unstaged, and untracked), groups the files into logical units
  by blast radius (one concern per commit), writes a Conventional Commit for each group, then branches (if still on
  `main`), pushes, opens the PR with `gh pr create`, and enables auto-merge. Trigger when the user says "/create-pr",
  "create a PR", "open a pull request", "commit and PR these changes", "group these into commits and ship them", or has
  a dirty working tree they want landed. This is the COMMIT-GROUPING + PR front door; the build-and-deploy-to-prod flow
  is the separate [[power-push]] skill (run /create-pr first, let it merge, then power-push from `main`). Honors the
  workspace gate (`cargo fmt` + `clippy` + `cargo test`, plus markdown lint on any `.md`) before the first commit.
---

# `/create-pr` — group changes into conventional commits, open a PR

The job: take whatever is sitting in the working tree and land it as a well-formed pull request — **not** one giant
commit, but a small set of logically-grouped Conventional Commits, each with its own blast radius, on a topic branch,
opened against `main` with auto-merge enabled. This skill owns the path from *dirty tree* to *PR exists and is set to
merge*. It does **not** build images or deploy — that is [[power-push]], which runs from `main` after this PR lands.

This is the canonical branch → PR → auto-merge flow from `CLAUDE.md`, made into one entry point. `main` is merge-only:
it advances solely through PRs. You never commit to `main` directly and you never babysit the merge — GitHub squash-
merges the PR the moment `ci.yml` goes green.

## The whole flow, in order

1. **Survey** every change (staged + unstaged + untracked).
2. **Group** the files into logical units — one concern per commit.
3. **Gate** the workspace once (`fmt` + `clippy` + `test`, markdown lint if any `.md` changed).
4. **Branch** off `main` (or carry existing work onto a topic branch).
5. **Commit** each group as a Conventional Commit, staging only that group's paths.
6. **Push** the branch and **open the PR** against `main`.
7. **Enable auto-merge** (`--squash`) and report the PR URL.

Each step assumes the prior one. Do them in this order.

## Step 1 — Survey the changes

See the full picture before grouping. Untracked files are easy to miss — include them.

```bash
git status              # the human-readable overview (tracked + untracked)
git status --porcelain  # stable, parseable: XY <path> per line
git diff                # unstaged content changes
git diff --staged       # already-staged content changes
git diff --stat HEAD    # one-line-per-file churn summary
```

For untracked files, read enough of each to know what it is — you are about to author a commit message that claims to
know. Do not assume from the filename.

## Step 2 — Group into logical units (one concern per commit)

This is the judgment call the skill exists for. Partition the changed paths into the **smallest set of coherent
commits** such that each commit is one reviewable concern with a single blast radius. Reviewers read commits; a clean
grouping is the deliverable.

Heuristics for "same commit":

- **Same concern, different files.** A new route handler in `web/src/`, its view in `views/src/`, and the test that
  covers it are *one* feature — they ship together (TDD: the test lands in the same commit as the code it covers).
- **A migration and the entity/code that depends on it** move together — a half-applied schema split is not reviewable.
- **Generated-with-its-source.** `docs/erd.svg` + `docs/erd.md` after a migration; a vendored asset + its
  `VENDOR.toml` hash.

Heuristics for "split apart" (different blast radius → different commit):

- **Crate bump vs. vendored-asset swap** — a `Cargo.lock` change and a minified-JS change verify differently; never the
  same commit (this is exactly why [[update]] keeps them separate).
- **Refactor vs. behavior change** — a rename/move with no behavior change is its own commit, so the diff that *does*
  change behavior stays small and legible.
- **Unrelated fixes** — two bugs in two subsystems are two commits even if you found them in one sitting.
- **CI/workflow/tooling changes** (`.github/workflows/`, `.claude/skills/`) are usually their own `ci:`/`chore:` commit,
  separate from product code.
- **Docs-only** edits (`*.md`, `docs/`) split from code unless the doc *documents that exact code change*.

When the right grouping is genuinely ambiguous (e.g. one sprawling change that could be one commit or three), state the
proposed grouping to the user in a sentence or two and proceed with the most defensible split — don't stall. For a
design-level "one bundle or three PRs?" call, that is a [[council]] question; this skill handles the commit grouping
*within* one PR.

## Step 3 — Gate before the first commit

Run the workspace gate from `CLAUDE.md` once, up front, so every commit on the branch is green:

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

If any `.md` file is in the change set, also lint it (dogfood the workspace binary — see [[markdown-lint]]):

```bash
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes <path>
```

`cargo fmt` may itself modify files — fold those formatting tweaks into whichever commit owns the touched file. If the
gate fails, fix it before committing; do not open a PR on a red tree. `cargo test` takes its Postgres from
`testcontainers`, so it does not need the KIND loop — but Docker must be running.

## Step 4 — Branch off `main`

Never commit to `main`. If you are on `main`, cut a topic branch first; the uncommitted work travels with you:

```bash
git rev-parse --abbrev-ref HEAD            # where am I?
git switch -c <kebab-topic>                # e.g. git switch -c add-create-pr-skill
```

Pick a short, descriptive kebab-case branch name from the dominant concern of the change set. If already on a non-`main`
topic branch with related work, stay on it.

## Step 5 — Commit each group as a Conventional Commit

Stage **only** the paths for the current group, then commit. Repeat per group, in dependency order (a migration before
the code that needs it; a refactor before the behavior change built on it).

```bash
git add <paths-for-this-group>
git commit -m "$(cat <<'EOF'
<type>(<scope>): <imperative subject, <=72 chars>

<optional body: the why, not the what — wrap at 72 columns>

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

Stage by explicit path (not `git add -A`) so each commit captures exactly its group. Verify the split between commits
with `git status --porcelain` — paths should drain group by group until the working tree is finally clean.

### Conventional Commit grammar

`<type>(<optional scope>): <subject>` — subject in the **imperative mood** ("add", "fix", "remove", not "added" /
"adds"), no trailing period, lower-case start. Types used in this workspace:

| type | when |
| --- | --- |
| `feat` | a new capability or user-visible behavior |
| `fix` | a bug fix |
| `refactor` | behavior-preserving restructuring (rename, move, extract) |
| `docs` | docs / prose / README / `CLAUDE.md` only |
| `test` | tests added or changed in isolation (usually folded into `feat`) |
| `chore` | tooling, deps, skills, housekeeping (`chore(deps): …`) |
| `ci` | `.github/workflows/` and CI plumbing |
| `perf` | a performance improvement |
| `style` | formatting only, no code change |
| `build` | build system / Dockerfiles / Cargo manifests (non-dep) |

Scope is the crate or area — `web`, `store`, `cli`, `views`, `deps`, `mcp`, etc. Keep it to the one thing the commit
touches. Use `!` after the type/scope (e.g. `feat(store)!:`) or a `BREAKING CHANGE:` body trailer for a breaking change.

End every commit message with the workspace trailer:

```text
Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
```

## Step 6 — Push and open the PR against `main`

```bash
git push -u origin <branch>
gh pr create --base main --title "<headline>" --body "$(cat <<'EOF'
## Summary
- <one bullet per logical commit / concern>

## Test plan
- <how it was verified: cargo test --workspace, KIND run, browser, etc.>

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

The PR title is the headline for the whole change (often the dominant commit's subject). The body's Summary should read
as one bullet per commit so a reviewer sees the grouping at a glance. The Test plan states what you actually ran — if
you didn't run something, say so (the "no assumptions" rule from `CLAUDE.md`).

## Step 7 — Enable auto-merge and report

```bash
gh pr merge --auto --squash
gh pr view --web    # optional: open it; or just print the URL gh pr create returned
```

Auto-merge is a GitHub-native setting (not a fourth workflow). GitHub squash-merges the moment the PR's `ci.yml` run is
green. Report the PR URL and the commit grouping you landed; do not wait around for the merge.

## Boundaries

- **Stops at "PR open + auto-merge on."** Building/pushing images and rolling out to prod is [[power-push]], run from
  `main` after this PR merges. `/create-pr` never deploys.
- **One PR.** This skill groups changes into commits **within a single PR**. Splitting work across *multiple* PRs is a
  sequencing decision — surface it (a [[council]] "one bundle or three?" call) rather than guessing.
- **Never touches `main` directly** and never force-pushes a shared branch.
- **Honors the gate.** No PR opens on a red `fmt`/`clippy`/`test`/markdown tree.
