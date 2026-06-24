# Agent workflows

This page is the shared operating manual for humans and LLM agents. Codebase work has exactly two top-level actions:
**create a PR** or **review/update an existing PR**. Everything else on this page is a supporting check inside one of
those actions.

## The two actions

- **Create a PR** when the task asks for new code, docs, configuration, tests, or a branch that can merge into `main`.
- **Review/update a PR** when the task starts from an existing PR, review comment, CI result, requested change, or
  "view this PR" request.

Do not invent a third agent workflow. Preparing, authoring a Restate handler, creating a legal workflow, running
Markdown lint, checking GitOps, and consulting the councils are subroutines inside one of these two actions.

## Create a PR

The canonical ship path is [`gitops.md`](gitops.md): branch, push, open a PR, enable squash auto-merge. Do not commit
directly to `main`.

Before changing files:

1. Rebase or otherwise confirm the branch is current with `origin/main`.
2. Read [`CLAUDE.md`](../CLAUDE.md), [`AGENTS.md`](../AGENTS.md), and the most specific docs from
   [`index.md`](index.md).
3. Read [`glossary.md`](glossary.md) before using domain nouns.
4. Read [`access-model.md`](access-model.md) before touching roles, participation, OPA, sessions, or visibility.
5. Check the working tree with `git status --short --branch`; never overwrite user changes.
6. Pick the narrowest docs and code path that actually cover the task.

If the decision is architectural, legal-copy, or client-facing, use the relevant council in
[`agent-decision-councils.md`](agent-decision-councils.md) after reading the facts.

When a dirty tree is ready to land:

1. Survey every change: `git status --porcelain`, `git diff`, `git diff --staged`, and untracked files.
2. Group paths by concern. One commit should have one blast radius.
3. Run the gate before committing:

   ```bash
   cargo fmt
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

4. For Markdown changes, also run:

   ```bash
   cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes <path>
   ```

5. Stage explicit paths for each group, not `git add -A`.
6. Use Conventional Commit subjects; use the PR title as the squash-merge commit title.
7. Push, `gh pr create --base main`, then `gh pr merge --auto --squash`.

If the work should become multiple PRs, decide that before committing. Use the Engineering Council for real sequencing
questions.

## Review or update a PR

A PR review is not complete until every reviewer comment has been adjudicated against the real code and answered.

1. Read the PR metadata, diff, and changed files at the head commit.
2. Review for correctness first: regressions, missing tests, auth gaps, durability gaps, data loss, and user-visible
   behavior.
3. Collect every outstanding human and bot comment.
4. For each comment, either fix it and reply, or explain why it is not a bug and reply.
5. Resolve threads only after the reply exists.
6. Re-run the relevant gate and report anything skipped.

If the PR needs more commits, treat that as the update half of this same action: make the smallest change on the PR
branch, run the relevant gate, push, reply to the comment that motivated it, and leave auto-merge or reviewer state
clear in the PR.

## Supporting checks

### Markdown lint

Use the workspace CLI, not a separate Markdown linter:

```bash
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes <path>
```

`--markdown-only` avoids notation-template rules on ordinary docs. `--no-default-excludes` makes root files such as
`AGENTS.md` and `CLAUDE.md` visible to the checker.

### Legal workflow authoring

Use this path when adding a new matter type or extending a template's workflow. Do not solve legal workflows with a
one-off router handler when a template + questionnaire + workflow can express the matter.

1. Write the `.feature` first in `features/tests/features/`.
2. Create or edit the template in `notation_templates/<category>/<snake_case_name>.md`.
3. Add new questions to `store/seeds/Question.yaml`.
4. Compose the workflow from documented step prefixes in [`notation-authoring.md`](notation-authoring.md).
5. Add reusable `StepKind` and dispatch code only when the existing step library cannot express the work.
6. Put every external or non-deterministic side effect behind Restate durability.
7. Add tests in the same commit as the implementation.

The core rule is still: the Template declares; Restate runs.

### Restate handler authoring

The full architecture is [`durable-workflows.md`](durable-workflows.md). For Rust handler code, the one replay-safety
rule is load-bearing:

> Every non-deterministic act belongs inside `ctx.run(...).name("stable-name")`.

That includes clocks, randomness, UUIDs, database writes, object storage, network calls, and third-party APIs. The
handler body may replay; `ctx.run` journals the result so replay reuses it instead of re-executing the side effect.

Use terminal errors for invalid input that can never succeed later. Use retryable errors for infrastructure failures. Do
not use native `tokio::spawn`, `join_all`, or channels for journaled steps inside a Restate handler; use Restate SDK
sequencing/combinators or keep the steps sequential.

### GitOps and deploy

The branch-to-prod path is:

1. PR merges by squash into `main`.
2. `release-tag.yml` cuts a `YY.MM.DD` tag.
3. `deploy.yml` publishes all images to ghcr.io.
4. An operator rolls GKE onto the dated tag.

Read [`gitops.md`](gitops.md), [`gke-prod.md`](gke-prod.md), and [`cloud-operations.md`](cloud-operations.md) before
changing CI, release, deploy, cluster, or production secret behavior.

Always roll `navigator-web` and `workflows-service` together. A version skew between the public web surface and the
durable worker is a production risk.

### Maintenance support

- Dependency refresh: follow the Rust crate and web asset sections in [`rust-programming.md`](rust-programming.md) and
  the vendored asset rules in `web/public/VENDOR.toml`.
- ERD refresh: regenerate [`erd.md`](erd.md) and [`erd.svg`](erd.svg) together after schema changes.
- Government forms: use canonical issuing-authority sources and keep provenance in [`gov-forms.md`](gov-forms.md).
- Disk cleanup: measure first, reclaim safely, and do not delete Docker volumes unless the user approves the data loss.
