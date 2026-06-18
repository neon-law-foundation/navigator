# `navigator-lsp` — VS Code extension

Thin TypeScript shim around `vscode-languageclient` that registers `navigator-lsp` as the language server for `*.md`
files. The Rust binary does all the work; this extension just wires it into VS Code.

## Building

```bash
npm install
npm run build      # tsc → out/extension.js
npm run package    # writes navigator-lsp-<version>.vsix
code --install-extension navigator-lsp-*.vsix
```

## Configuration

- `navigator.lspPath` — absolute path or PATH-resolvable name of the `navigator-lsp` binary. Defaults to
  `navigator-lsp`; run `cargo install --path lsp` once and the default works.

For fix-on-save, set in your workspace `.vscode/settings.json`:

```json
{
  "[markdown]": {
    "editor.codeActionsOnSave": { "source.fixAll": "explicit" }
  }
}
```
