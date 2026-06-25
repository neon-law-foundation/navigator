# `navigator-lsp` editor integration

`navigator-lsp` is one binary, JSON-RPC over stdio, no telemetry. Configure any LSP-aware editor to launch it for `*.md`
files in a Neon Law Navigator workspace and you get red squiggles + fix-on-save tied to the same rule engine as `cli
validate`.

Per-editor snippets live alongside this README:

- [Neovim](./neovim.md) — built-in `vim.lsp.start`. [Helix](./helix.md) — language servers in `languages.toml`.
  [Emacs](./emacs.md) — `eglot` or `lsp-mode`. [VS Code](./vscode.md) — sideload the bundled extension under
  `lsp/vscode-ext/`. [Zed](./zed.md) — sideload the bundled extension under `lsp/zed-ext/`.

## Install the binary

```bash
cargo install --path lsp
# OR for one-off use without `cargo install`:
cargo build --release -p lsp
# binary at: target/release/navigator-lsp
```

Pointing your editor at the absolute `target/release/navigator-lsp` path is fine while iterating; once you have run
`cargo install`, `navigator-lsp` lives on `$PATH` and the snippets below work as-is.

## Coexistence with `marksman` / `markdown-oxide`

`navigator-lsp` covers `navigator`-style rules (M-family, S-family). If you already run a general markdown LSP like
`marksman` or `markdown-oxide`, their diagnostics don't overlap — most editors will happily run both. Some editors
complain about duplicate "Markdown" servers; in that case disable the other server inside Neon Law Navigator workspaces.

## Verifying it works

Open a markdown file with an obvious violation (a hard tab at column zero on a non-fenced line) and look for an `M010`
diagnostic. Then run your editor's "fix all" or `source.fixAll` action — the tab should turn into two spaces.
