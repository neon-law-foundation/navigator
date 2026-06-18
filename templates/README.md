# templates

Markdown notation templates — the blueprints that produce **notations** (filled-in instances) when assigned to a person
or entity. Every file in this tree is a markdown document with a YAML frontmatter block carrying `title`, `code`,
`respondent_type`, and the `questionnaire:` / `workflow:` state machines. The body is the legal prose with
`{{question_code}}` placeholders.

## Layout

```text
templates/
├── README.md                this file
├── llc/california.md        LLC blueprint
├── nest/nevada.md           Nevada entity formation (Nest) — stub body, real questionnaire + workflow
├── onboarding/retainer.md   Engagement-letter blueprint + workflow
├── trust/nevada.md          Trust blueprint
└── will/simple.md           Last will blueprint
```

The first directory level is the **category** (`trust`, `llc`, `will`, `onboarding`, …). The filename is the
**specifier** within that category, in `snake_case` so it mirrors the frontmatter `code` (`nevada.md`, `california.md`,
`simple.md`). Categories grow as new document types arrive; nothing else changes.

## Linting policy

Two passes, both via the workspace's own CLI:

- **Every file two or more levels deep** (`templates/<category>/<name>.md`) must pass the full Navigator ruleset —
  M-family Markdown rules, the S101 120-character line limit, **and** the F-family frontmatter rules that enforce
  template shape.

  ```bash
  cargo run -p cli --quiet -- validate templates
  ```

- **This `README.md`** is linted like every other workspace README — M-family rules + S101 only, F-family skipped:

  ```bash
  cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes templates/README.md
  ```

CI runs both passes; either failing breaks the build.

## Adding a new template

1. Pick or create a category directory under `templates/`.
2. Drop a markdown file named after the specifier, in `snake_case` (`templates/trust/wyoming.md`).
3. Frontmatter: `title`, stable `code` (e.g., `trusts__wyoming`), `respondent_type`, plus the `questionnaire:` and
   `workflow:` state machines.
4. Body: legal prose with `{{question_code}}` placeholders that reference codes declared in `questionnaire:`.
5. Run `cargo run -p cli -- validate templates` until it exits `0`.
