# Helix — `navigator-lsp`

Helix discovers language servers from `~/.config/helix/languages.toml`.

## Minimal config

```toml
[[language]]
name = "markdown"
language-servers = [{ name = "navigator-lsp", except-features = ["format"] }]

[language-server.navigator-lsp]
command = "navigator-lsp"
```

Restart Helix; open a `*.md` file with an obvious violation (a hard tab at column zero on a non-fenced line) and you
should see a diagnostic in the gutter.

## Code actions

- `<Space> a` (default `code_action` binding) lists the quick-fix actions for the violation under the cursor.
- `:format` is not provided — Navigator's rule set is autofix-driven via `source.fixAll`, which Helix doesn't currently
  auto-invoke. Run code actions manually for now.

## Coexistence with `marksman`

If you already have `marksman` registered, you can run both — append the new server rather than replacing:

```toml
[[language]]
name = "markdown"
language-servers = [
  { name = "marksman" },
  { name = "navigator-lsp", except-features = ["format"] },
]
```
