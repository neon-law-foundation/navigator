# Navigator LSP

`lsp` builds `navigator-lsp`, the local Language Server Protocol adapter for Navigator's Markdown and Notation rules. It
publishes diagnostics, safe quick fixes, fix-all actions, and contextual rule help over standard input and output.

It serves editor integrations while keeping the `rules` crate as the single source of validation truth. The server has
no network access or telemetry, which lets confidential drafting receive immediate feedback without leaving the
workstation.

See [editor integration](../docs/lsp/README.md) for setup and [`rules`](../rules/README.md) for the rule engine.
