# Test assets

## `axe.min.js`

[axe-core](https://github.com/dequelabs/axe-core) v4.10.2 — the accessibility rules engine, vendored verbatim.

- **Test-only.** It is injected into the page by `web/tests/accessibility_e2e.rs` over WebDriver at test time. It is
  **never** linked from the app layout (`views/src/layout.rs`) and is **never** served to users or shipped in the
  container image.
- **License.** axe-core is MPL-2.0. It is used unmodified as a separate file, so the MPL's file-level terms are
  satisfied; it does not affect the workspace's own `MIT OR Apache-2.0` license.
- **Refresh.** Re-vendor with a pinned version:

  ```sh
  curl -sS -o web/tests/assets/axe.min.js \
    https://cdn.jsdelivr.net/npm/axe-core@4.10.2/axe.min.js
  ```
