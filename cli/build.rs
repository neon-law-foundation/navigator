//! Bake the published release tag into the `navigator` binary.
//!
//! `deploy.yml` builds the downloadable CLI from a `YY.MM.DD` git tag and
//! exposes it to `cargo build` as `NAVIGATOR_RELEASE_TAG`. We capture that at
//! build time and re-export it as `NAVIGATOR_CLI_VERSION`, which `main.rs`
//! reads with `env!`. This is what makes a *downloaded* release binary report
//! its release with no environment set — the runtime `NAVIGATOR_RELEASE_TAG`
//! override in `main.rs` still wins when present. On a plain local build the
//! tag is unset and we fall back to the workspace crate version (`0.1.0`).

use std::env;

fn main() {
    // Rebuild when the release tag changes so a re-tag re-bakes the version.
    println!("cargo:rerun-if-env-changed=NAVIGATOR_RELEASE_TAG");

    let version = match env::var("NAVIGATOR_RELEASE_TAG") {
        Ok(tag) if !tag.trim().is_empty() => tag.trim().to_string(),
        // CARGO_PKG_VERSION is always set for a build script.
        _ => env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is set by cargo"),
    };

    println!("cargo:rustc-env=NAVIGATOR_CLI_VERSION={version}");
}
