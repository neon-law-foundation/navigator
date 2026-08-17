# Rules

`rules` is Navigator's pure validation engine for Markdown, Notations, event content, frontmatter, and structural
conventions. It returns stable, coded diagnostics and safe edits without requiring a database or network access.

It serves the CLI, language server, CI, and any other surface that evaluates authored work. One reusable engine is
necessary so attorneys and engineers receive the same result while editing, reviewing, and shipping a file.

Rule implementations and tests live together in this crate. See [Notation authoring](../docs/notation-authoring.md) and
[`navigator-lsp`](../lsp/README.md) for the principal consumer workflows.
