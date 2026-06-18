# VS Code — `navigator-lsp`

The bundled extension lives at [`lsp/vscode-ext/`](../../lsp/vscode-ext/). It's intentionally tiny — about fifty lines
of TypeScript that registers `navigator-lsp` as a language client for `*.md`.

## Build + sideload

```bash
cd lsp/vscode-ext
npm install
npm run package      # writes navigator-lsp-<version>.vsix
code --install-extension navigator-lsp-*.vsix
```

VS Code expects `navigator-lsp` on `$PATH`. Run `cargo install --path lsp` once, or set the absolute binary path via the
`navigator.lspPath` setting (added by the extension).

## Fix-on-save

Add to your workspace `.vscode/settings.json`:

```json
{
  "[markdown]": {
    "editor.codeActionsOnSave": {
      "source.fixAll": "explicit"
    }
  }
}
```

Now every save runs `source.fixAll` and every safe-by-construction rule (M009 trailing whitespace, M010 hard tabs, M012
multi-blank, M018-M021 ATX spacing, M027 blockquote spacing) cleans itself up.
