//! Provenance guard for vendored front-end assets.
//!
//! `web/public/VENDOR.toml` is the single source of truth for every
//! third-party CSS/JS/font we serve (Bootstrap, HTMX, Alpine, Bootstrap
//! Icons). This test recomputes the SHA-256 of each `served_path` and asserts
//! it equals the recorded `sha256`. If someone hand-edits a vendored blob, or
//! the `update-web-assets` skill writes new bytes without updating the
//! manifest, this fails — so the manifest can never silently drift from disk.
//!
//! Same shape as `store/tests/timestamp_convention.rs`: a convention enforced
//! by a test, not by discipline.

use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct Manifest {
    asset: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    served_path: String,
    sha256: String,
}

fn public_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("public")
}

#[test]
fn vendored_assets_match_manifest() {
    let public = public_dir();
    let manifest_path = public.join("VENDOR.toml");
    let raw = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest_path.display()));
    let manifest: Manifest =
        toml::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", manifest_path.display()));

    assert!(
        !manifest.asset.is_empty(),
        "VENDOR.toml lists no assets — did the [[asset]] tables get dropped?"
    );

    for asset in &manifest.asset {
        let path = public.join(&asset.served_path);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "{} ({}): cannot read served_path {}: {e}",
                asset.name,
                asset.served_path,
                path.display()
            )
        });
        let actual = hex_lower(&Sha256::digest(&bytes));
        assert_eq!(
            actual, asset.sha256,
            "{} ({}): on-disk SHA-256 does not match VENDOR.toml.\n  \
             expected {}\n  actual   {}\n\
             Refresh via the update-web-assets skill, or update the manifest if \
             this change is intentional.",
            asset.name, asset.served_path, asset.sha256, actual
        );
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
