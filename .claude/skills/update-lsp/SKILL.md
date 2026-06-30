---
name: update-lsp
description: >
  Rebuild the local `navigator-lsp` server binary from the current tree and put it on `$PATH` so Zed (and any editor
  that resolves the server from `$PATH`) picks up your local changes. Trigger when the user says "/update-lsp", "update
  the LSP", "rebuild the LSP for Zed", "refresh navigator-lsp", or after editing anything under `lsp/src/` and wanting
  to try it live. This refreshes the SERVER BINARY only; it does not rebuild the `lsp/zed-ext/` wasm extension (that
  ships via the Zed registry). Full Zed integration reference: `docs/lsp/zed.md`.
---

# Update the local navigator-lsp binary for Zed

The `lsp/zed-ext/` extension resolves the language server **most-specific-first**: an explicit `binary.path` in Zed
settings → a `navigator-lsp` already on `$PATH` → the downloaded GitHub Release binary. So installing a fresh build onto
`$PATH` makes Zed use your local code without touching the published extension.

## The canonical command

```bash
cargo install --path lsp --force
```

This rebuilds the `lsp` crate in release mode and replaces `~/.cargo/bin/navigator-lsp`. The `--force` is required —
without it `cargo install` no-ops when the binary already exists.

## After it installs, tell the user to restart Zed

The server binary is launched fresh per language-server start, so a swap on `$PATH` is picked up by **restarting the
language server**, not reinstalling the extension. Tell the user to either:

- Command palette (`Cmd-Shift-P`) → **`editor: restart language server`**, or
- Restart Zed entirely (`Cmd-Q` then relaunch).

## Two things to verify / flag, not assume

- **GUI-launched Zed does not inherit shell `$PATH`.** A Zed started from the Dock/Finder may not find
  `~/.cargo/bin/navigator-lsp`. The reliable fix is an explicit settings override (mention it if the user reports the
  server not loading):

  ```json
  { "lsp": { "navigator-lsp": { "binary": { "path": "/Users/<you>/.cargo/bin/navigator-lsp" } } } }
  ```

- **The dev extension must already be installed in Zed.** This skill only refreshes the server binary, not the
  extension. If the extension was never sideloaded, the `navigator-lsp` server is not wired into Zed at all — point the
  user at Zed's `zed: install dev extension` action → `lsp/zed-ext/` (see `docs/lsp/zed.md`).

## What this is NOT

- It does not rebuild or re-sideload the wasm extension (`lsp/zed-ext/`). Editing `lsp/zed-ext/src/lib.rs` or
  `extension.toml` needs a `wasm32-wasip2` build and a re-`install dev extension` — out of scope here.
- It does not publish anything. Cross-building per-triple and `cli lsp publish` to the assets bucket is the maintainer
  release path documented in `docs/lsp/zed.md`.
