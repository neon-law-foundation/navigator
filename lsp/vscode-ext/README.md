# Navigator LSP for VS Code

This package is the thin VS Code client for `navigator-lsp`. It registers the Rust language server for Markdown and
exposes the binary path setting; all validation and fixes remain in the server.

It serves VS Code users who want Navigator diagnostics and fix-on-save without duplicating rule logic in TypeScript.
Keeping the extension deliberately small ensures VS Code reports the same results as other editors and CI.

See the [editor integration guide](../../docs/lsp/README.md) and the [`lsp` crate](../README.md).
