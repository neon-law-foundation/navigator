---
name: markdown-lint
description: >
  Lint every `.md` file in the workspace with the navigator CLI (M-family rules + S101 120-char line limit). Trigger
  when adding or editing any Markdown file (READMEs, `docs/`, `CLAUDE.md`, blog posts under `web/content/`) and before
  committing `.md` changes. Dogfood the workspace's own binary; never hand-roll a different linter.
---

# Markdown linting via the navigator CLI

Every `.md` file in this repo must pass the navigator CLI's markdown
rule set. We dogfood our own linter so the rule definitions, exit
codes, and CI behavior stay coherent.

## The canonical command

```bash
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes <path>
```

What each piece does:

- `--markdown-only` — runs the M-family Markdown rules and S101 (line
  length), but skips the F-family (Navigator notation frontmatter).
  Without this, every README would fail with bogus F101/F102/F103
  violations.
- `--no-default-excludes` — validates files normally skipped by name
  (`README.md`, `CLAUDE.md`, `LICENSE.md`, `CODE_OF_CONDUCT.md`,
  `ERD.md`) and directories (`AgentDocumentation`, `workshops`,
  `Blog`). For prose docs you want these in scope.
- `<path>` — either a file or a directory. The walker recurses.

## Lint every workspace README in one pass

```bash
for d in rules store views workflows cloud web cli compass mcp; do
  cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes "$d"
done
```

Exit `0` on every iteration means clean. Otherwise the violating
file, line, rule code, and message print to stdout.

## Common rules that fire

- **S101** — line longer than 120 characters. Reflow the paragraph;
  don't fight the limit.
- **M026** — heading ends with trailing punctuation `.`. Drop the
  period from `## Headings.` (watch for false positives: bash
  `# comment.` lines inside fenced code blocks trip the same rule).
- **M038** — inline code span has leading or trailing whitespace.
  Usually means the span got broken across two lines; keep code
  spans on a single line.
- **M040** — fenced code block is missing a language tag. Add one
  (`bash`, `rust`, `text`, `yaml`, …) right after the opening fence.
- **M031** — fenced code block must have a blank line before it.
  Common when a code block is nested inside a list item.
- **M060** — table column alignment is inconsistent within a table.

## When to run it

- Before committing any change that touches a `.md` file.
- When you create a new README.
- As part of CI for the docs surface (not currently wired in
  `ci.yml`, but on the roadmap).

## What NOT to do

- Don't reach for `markdownlint`, `mdformat`, or any non-Rust linter.
  We standardize on the in-house `cli` — that's the whole point of
  dogfooding. See [[rust-best-practices]] for the Rust-only stance.
- Don't run plain `cargo run -p cli -- validate <path>` on a README.
  Without `--markdown-only` it fails with F-family complaints about
  missing frontmatter; without `--no-default-excludes` it silently
  skips the file.
- Don't disable a rule by editing `cli/src/main.rs`. If a rule is
  wrong, fix it in `rules/src/<code>.rs` with a test.
