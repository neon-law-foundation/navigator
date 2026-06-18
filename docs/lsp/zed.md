# Zed — `navigator-lsp`

The bundled extension lives at [`lsp/zed-ext/`](../../lsp/zed-ext/). It's a pure-Rust Zed extension targeting
`wasm32-wasip1`; it declares the markdown language association and points at the `navigator-lsp` binary on `$PATH`.

## Download a prebuilt binary

You don't have to build `navigator-lsp` yourself. The [`/lsp`](https://www.neonlaw.com/lsp) page serves a prebuilt
binary for each platform straight from the public assets bucket. Grab the one for your machine, make it executable, and
put it on your `$PATH`:

```bash
# macOS · Apple Silicon shown; swap the triple for your platform
curl -fL -o navigator-lsp \
  https://storage.googleapis.com/YOUR_PROJECT_ID-assets/lsp/aarch64-apple-darwin/navigator-lsp
chmod +x navigator-lsp
mv navigator-lsp /usr/local/bin/
```

Supported triples: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`. The bucket host is `NAVIGATOR_ASSET_BASE_URL` in the deployer's `.env`; on neonlaw.com it
is the `<project>-assets` bucket. With the binary on `$PATH`, the extension below finds it automatically.

### Publishing the binaries (maintainers)

`cli lsp publish` pushes the binaries to the assets bucket. Cross-build each target into the layout the publisher
expects (`<dir>/<triple>/navigator-lsp`), then publish:

```bash
# Build per target (host arch builds with plain cargo; cross targets via `cross`).
for triple in aarch64-apple-darwin x86_64-apple-darwin \
              x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
  cargo build --release -p lsp --target "$triple"
  mkdir -p "target/lsp-dist/$triple"
  cp "target/$triple/release/navigator-lsp" "target/lsp-dist/$triple/navigator-lsp"
done

# Publish whatever got built (missing triples are skipped, not an error).
# Auth is ADC; NAVIGATOR_ASSETS_BUCKET names the public bucket.
cargo run -p cli -- lsp publish --dir target/lsp-dist
```

Each binary lands at `lsp/<triple>/navigator-lsp` with a bounded (one-hour) `Cache-Control`, so a re-publish is picked
up shortly. The download buttons on `/lsp` resolve to exactly these keys via `views::assets::asset_url`, so the upload
path and the link can never drift.

## Build + sideload

Zed reads extensions from `~/Library/Application Support/Zed/extensions/` on macOS or `~/.local/share/zed/extensions/`
on Linux. Use Zed's "Install Dev Extension" action from the command palette:

```text
zed: install dev extension
```

Then point it at `lsp/zed-ext/` in this repository. The first build compiles the extension to wasm; subsequent restarts
reuse the cached build.

## If install fails with "failed to run rustc"

Zed compiles a dev extension with its own toolchain, but a Zed launched from the Dock or Finder does not inherit your
shell `$PATH` — so it cannot find `rustc`/`cargo` when they live in rustup's `~/.cargo/bin`. The install then dies with
`failed to compile Rust extension: failed to run rustc: No such file or directory`. Pick one fix:

- **No sudo, per launch.** Quit Zed fully (`Cmd-Q`, not just the window), then start it from a terminal that already
  has Rust on `$PATH` so Zed inherits it:

  ```bash
  /Applications/Zed.app/Contents/MacOS/zed
  ```

- **One-time, permanent.** Symlink the rustup shims into `/usr/local/bin` (on the default GUI `$PATH`), then re-run the
  install from the Dock — no relaunch needed:

  ```bash
  sudo ln -sf ~/.cargo/bin/rustc /usr/local/bin/rustc
  sudo ln -sf ~/.cargo/bin/cargo /usr/local/bin/cargo
  ```

Until the extension actually installs, Zed flags the `navigator-lsp` entry under `lsp` in your settings as an unknown
language server ("property not allowed") — that warning is downstream of the failed install, not a settings bug, and
clears once the install succeeds.

## Configuration

The extension assumes `navigator-lsp` is on `$PATH`. Override via the Zed user settings (`Cmd-,` → "Open Settings"):

```json
{
  "lsp": {
    "navigator-lsp": {
      "binary": { "path": "/absolute/path/to/navigator-lsp" }
    }
  }
}
```

## Fix-on-save

Zed honors `source.fixAll` automatically when configured per-language. Note that `code_actions_on_format` is a _map_ of
action to boolean, not an array — pass a list and Zed rejects the whole settings file with the parse error "invalid
type: sequence, expected a map".

```json
{
  "languages": {
    "Markdown": {
      "code_actions_on_format": { "source.fixAll": true },
      "format_on_save": "on"
    }
  }
}
```
