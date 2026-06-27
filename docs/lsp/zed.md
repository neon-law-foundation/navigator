# Zed — `navigator-lsp`

**Navigator LSP** works on every Markdown document — and enforces a stricter superset of Markdown for notation
templates. It ships as a published Zed extension: installing it pulls the matching `navigator-lsp` binary from the
latest [navigator GitHub Release](https://github.com/neon-law-foundation/navigator/releases) automatically, so there is
nothing else to install.

## Install from Zed's extension store

1. Open the command palette (`Cmd-Shift-P` on macOS, `Ctrl-Shift-P` on Linux/Windows) and run `zed: extensions`.
2. Search for **Navigator LSP** and click **Install**.
3. Open any Markdown file — diagnostics appear as you type, and notation templates get the extra frontmatter rules on
   top.

On first run the extension downloads the prebuilt `navigator-lsp` from the GitHub Release and caches it per version;
later releases are picked up automatically. The prebuilt binary today is **Apple Silicon macOS** only. On any other
platform the extension reports a clear error — install the binary yourself
([below](#build-the-binary-yourself-contributors)) or point Zed at one with an explicit [settings path](#configuration).

The rest of this page is for contributors and maintainers — building the binary by hand, sideloading the extension as a
dev build, and the publishing pipeline.

## Build the binary yourself (contributors)

The bundled extension lives at [`lsp/zed-ext/`](../../lsp/zed-ext/) — a pure-Rust Zed extension targeting
`wasm32-wasip2`. It resolves the server most-specific first: an explicit settings `binary.path`, then a `navigator-lsp`
already on `$PATH`, then the downloaded Release binary. So a `cargo install` or a curled binary on `$PATH` always wins
over the download — handy when iterating on the LSP itself:

```bash
cargo install --path lsp        # builds + installs navigator-lsp onto your $PATH
```

The [`/lsp`](https://www.neonlaw.com/lsp) page also serves a prebuilt binary for each platform straight from the public
assets bucket, if you'd rather not compile:

```bash
# macOS · Apple Silicon shown; swap the triple for your platform
curl -fL -o navigator-lsp \
  https://storage.googleapis.com/YOUR_PROJECT_ID-assets/lsp/aarch64-apple-darwin/navigator-lsp
chmod +x navigator-lsp
mv navigator-lsp /usr/local/bin/
```

Supported triples: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc`. The Windows binary is named `navigator-lsp.exe`; the others
are named `navigator-lsp`. The bucket host is `NAVIGATOR_ASSET_BASE_URL` in the deployer's `.env`; on neonlaw.com it is
the `<project>-assets` bucket.

### Publishing the binaries (maintainers)

`cli lsp publish` pushes the binaries to the assets bucket. Cross-build each target into the layout the publisher
expects (`<dir>/<triple>/<binary_name>`), then publish:

```bash
# Build per target (host arch builds with plain cargo; cross targets via `cross`).
for triple in aarch64-apple-darwin x86_64-apple-darwin \
              x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
              x86_64-pc-windows-msvc; do
  cargo build --release -p lsp --target "$triple"
  mkdir -p "target/lsp-dist/$triple"
  binary="navigator-lsp"
  if [ "$triple" = "x86_64-pc-windows-msvc" ]; then
    binary="navigator-lsp.exe"
  fi
  cp "target/$triple/release/$binary" "target/lsp-dist/$triple/$binary"
done

# Publish whatever got built (missing triples are skipped, not an error).
# Auth is ADC; NAVIGATOR_ASSETS_BUCKET names the public bucket.
cargo run -p cli -- lsp publish --dir target/lsp-dist
```

Each binary lands at `lsp/<triple>/<binary_name>` with a bounded (one-hour) `Cache-Control`, so a re-publish is picked
up shortly. The download buttons on `/lsp` resolve to exactly these keys via `views::assets::asset_url`, so the upload
path and the link can never drift.

## Install the wasm target first

Zed compiles a dev extension to a WebAssembly target, so that target must be installed under your Rust toolchain or the
install dies with `failed to compile Rust extension` and (in `~/Library/Logs/Zed/Zed.log`) `can't find crate for core …
the wasm32-wasip2 target may not be installed`. Add it once:

```bash
rustup target add wasm32-wasip2
```

The exact target name tracks the Zed version — current Zed builds against `wasm32-wasip2`; older releases used
`wasm32-wasip1`. If the log names a different triple than the one you installed, add that one too. The real error is
only in the Zed log; the UI shows just the one-line `failed to compile Rust extension`.

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

A `binary.path` override wins over both the `$PATH` lookup and the Release download — set it to pin a specific build, or
on a platform without a prebuilt binary. Override via the Zed user settings (`Cmd-,` → "Open Settings"):

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

## Publishing the extension (maintainers)

The extension is published like the Homebrew tap — automatically, every release. The flow has three repos:

1. **`lsp/zed-ext/`** in this repo is the source of truth. Edit `src/lib.rs` / `extension.toml` here; CI gates it with a
   `wasm32-wasip2` build (the `zed-ext` job in `ci.yml`), since it is not a workspace member and `cargo test` never
   touches it.
2. **`neon-law-foundation/zed-navigator-lsp`** is the published extension repo — the git submodule Zed's registry
   tracks. The `zed-extension` job in [`deploy.yml`](../../.github/workflows/deploy.yml) syncs `lsp/zed-ext/` into it
   each release, stamps the release version (the `YY.MM.DD` tag normalized to semver — `26.06.27` → `26.6.27`), and
   pushes a `v<semver>` tag.
3. **`zed-industries/extensions`** is the public Zed registry. The `zed-navigator-lsp` repo's own `release.yml` runs
   [`huacnlee/zed-extension-action`](https://github.com/huacnlee/zed-extension-action) on that `v*` tag, which opens the
   version-bump PR (submodule pointer + `extensions.toml` version) against the registry. The first submission is
   human-reviewed by Zed maintainers; subsequent version bumps are auto-merged.

Only **tag pushes** publish the extension — a manual `workflow_dispatch` deploy publishes images and binaries but skips
the Zed bump, because its extra hour digit cannot be expressed as a strictly-increasing semver under the date scheme.

### One-time setup

- Create the public **`neon-law-foundation/zed-navigator-lsp`** repo (dual Apache-2.0 / MIT `LICENSE` files — Zed
  requires a license), seeded from the scaffold this change produced.
- Fork **`zed-industries/extensions`** → **`neon-law-foundation/extensions`** (public; HTTPS submodules only).
- Add the `zed-navigator-lsp` repo to the `GHCR_CLEANUP_PAT` gitops PAT's `contents:write` scope so `deploy.yml` can
  push to it, and add a `COMMITTER_TOKEN` (`repo` + `workflow` scope) secret in the `zed-navigator-lsp` repo for the
  registry-PR action.
- Submit the **first** registry PR to `zed-industries/extensions` by hand (it sets the `extensions.toml` entry, with the
  `path = "."` submodule root); the action keeps it current after that.
