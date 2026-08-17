//! `navigator dev build-webapp` — build the Dioxus client bundle (issue #641).
//!
//! Drives `dx` (the Dioxus CLI, itself a Rust binary — consistent with the
//! Rust-only invariant, exactly as this CLI already orchestrates `kubectl`,
//! `helm`, `docker`, and `kind`) to compile the `webapp` crate to
//! `wasm32-unknown-unknown` and copies the resulting bundle — `index.html`, the
//! wasm module, and the wasm-bindgen glue — into `server/public/dioxus`, where
//! `web` serves it same-origin and SSR-hydrates the `/dioxus-demo` page.
//!
//! The bundle is a build artifact: `images/Containerfile.web` runs this at image
//! build time and `server/public/dioxus` is gitignored, never committed.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use super::orchestrate::{require_tools, run, workspace_root};

/// Build the `webapp` wasm client bundle and stage it under
/// `server/public/dioxus`. `release` selects an optimized build (the deploy and CI
/// default); debug builds are faster for local iteration.
pub fn build(release: bool) -> Result<()> {
    require_tools(&["dx"]).context(
        "the Dioxus CLI `dx` is required to build the wasm client; install it with \
         `cargo install dioxus-cli --version 0.7.9 --locked`",
    )?;
    let root = workspace_root()?;

    let profile = if release { "release" } else { "debug" };
    eprintln!(
        "==> dx build --package webapp --platform web{}",
        if release {
            " --release --debug-symbols false"
        } else {
            ""
        }
    );
    let mut cmd = Command::new("dx");
    cmd.current_dir(&root)
        .arg("build")
        .arg("--package")
        .arg("webapp")
        .arg("--platform")
        .arg("web");
    if release {
        // `dx`'s own `--debug-symbols` defaults to TRUE, including under
        // `--release`, and it is the flag that decides whether wasm-opt is
        // invoked with `--debuginfo` or `--strip-debug`. Left at its default the
        // release bundle keeps the ~340 KB of DWARF that dx's ad-hoc
        // `wasm-release` profile preserves (that profile sets `strip=false`), and
        // binaryen then aborts re-emitting it — `UNREACHABLE executed at
        // DWARFEmitter.cpp:201`, SIGABRT. dioxus-cli treats a failed wasm-opt as
        // non-fatal and copies the UNOPTIMIZED module through, so the abort never
        // fails a build; it just silently ships 941 KB where 534 KB would do.
        //
        // Turning it off is what makes wasm-opt strip rather than rewrite, so the
        // `-Oz` pass we are already paying for actually lands. Release only: a
        // debug build wants the symbols, and its bundle is never served to the
        // public. The `name` section is unaffected — dx's separate `--keep-names`
        // already defaults to false and wasm-bindgen already runs with
        // `--remove-name-section`.
        cmd.arg("--release").arg("--debug-symbols").arg("false");
    }
    run(&mut cmd)?;

    let bundle = root
        .join("target/dx/webapp")
        .join(profile)
        .join("web/public");
    if !bundle.join("index.html").is_file() {
        anyhow::bail!(
            "dx did not produce a bundle at {} (no index.html)",
            bundle.display()
        );
    }

    let dest = root.join("server/public/dioxus");
    if dest.exists() {
        fs::remove_dir_all(&dest)
            .with_context(|| format!("clearing stale bundle at {}", dest.display()))?;
    }
    copy_dir_all(&bundle, &dest)?;
    eprintln!("==> staged Dioxus client bundle -> {}", dest.display());
    Ok(())
}

/// Recursively copy `src` into `dst`, creating `dst` and any parents.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("creating {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("copying {} -> {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}
