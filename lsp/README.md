# navigator-lsp

Language Server Protocol entry point for Navigator's markdown rules. One binary, JSON-RPC over stdio, no telemetry. Any
LSP-aware editor (Neovim, Helix, VS Code, Zed, Emacs, Cursor) attaches by registering `navigator-lsp` against `*.md`.

## What it provides

- `textDocument/publishDiagnostics` — every rule violation as an LSP diagnostic (severity `warning`,
  `source: "navigator"`, `code:` is the rule code).
- `textDocument/codeAction` — quick-fix actions for any safe-by-construction rule, plus a single `source.fixAll`
  aggregate that mirrors `cli validate --fix`.
- `textDocument/hover` — when the cursor is over a violation, returns the rule description and the violation
  message in a markdown bubble.

Skipped on purpose (track separately if you need them): formatting, completion, go-to-definition, references, workspace
symbols.

## Rule set

`navigator-lsp` lints with the markdown-only subset of Navigator rules (every `M-` rule plus `S101` line-length and
`S102` line-packing). F-family frontmatter rules are excluded since the LSP runs against arbitrary `*.md` files, not
only Navigator notation. The CLI's `validate` walks the same rule set when invoked with `--markdown-only`.

## Autofix surface

`source.fixAll` applies every rule whose `Rule::fix()` returns `Some(TextEdit)`:

- `M009` — strip trailing whitespace.
- `M010` — replace hard tab with two spaces.
- `M012` — collapse multi-blank runs to one blank.
- `M018` — insert space after `#` in ATX heading.
- `M019` — collapse multi-space after `#` to one space.
- `M020` — insert space before closing `#` of closed ATX.
- `M021` — collapse multi-space before closing `#`.
- `M027` — collapse multi-space after blockquote `>`.

Diagnostic-only (stays for a human): F-family, `M024` duplicate heading, `M026` trailing punctuation, `M005` list
indent, `S101` long line (no safe break heuristic in v1).

Conflict resolution when two rules want to edit overlapping ranges in the same `source.fixAll` batch: sort by
`range.start` ascending; keep the rule with the lower code string; drop overlapping latercomers.

## Privacy

No telemetry. No network. Reads the buffer the editor sends, runs rules in-process, sends diagnostics back.

## Editor integration

See `docs/lsp/` (Neovim, Helix, Emacs, VS Code, Zed). Shortest possible config — Neovim:

```lua
vim.lsp.start({
  name = "navigator-lsp",
  cmd = { "navigator-lsp" },
})
```
