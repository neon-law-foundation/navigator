//! Provenance guard for vendored government forms.
//!
//! `templates/forms/FORMS.toml` is the single source of truth for every
//! official form we fill and file. This test recomputes the SHA-256 of each
//! form's bundled bytes, asserts it equals the recorded `sha256`, and
//! cross-checks the on-disk file at `templates/` + `object_path` — so the
//! ledger, the `include_bytes!` bundle, and the working tree can never
//! silently drift apart. Same shape as `web/tests/vendor_assets.rs`: a
//! convention enforced by a test, not by discipline.

use std::fmt::Write as _;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("forms crate sits one level under the workspace root")
        .to_path_buf()
}

fn hex_lower(digest: &[u8]) -> String {
    digest.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[test]
fn vendored_forms_match_ledger() {
    let forms = forms::registry().expect("FORMS.toml parses and joins to bundled bytes");
    assert!(
        !forms.is_empty(),
        "FORMS.toml lists no forms — did the [[form]] tables get dropped?"
    );

    for form in &forms {
        let actual = hex_lower(&Sha256::digest(form.bytes));
        assert_eq!(
            actual, form.meta.sha256,
            "{}: bundled bytes do not match the ledger sha256 — \
             re-vendor via the vendor-gov-forms skill, never hand-edit",
            form.meta.form_code
        );

        let disk_path = workspace_root()
            .join("templates")
            .join(&form.meta.object_path);
        let disk = std::fs::read(&disk_path).unwrap_or_else(|e| {
            panic!(
                "{}: cannot read canonical example {}: {e}",
                form.meta.form_code,
                disk_path.display()
            )
        });
        assert_eq!(
            hex_lower(&Sha256::digest(&disk)),
            form.meta.sha256,
            "{}: on-disk canonical example diverges from the ledger",
            form.meta.form_code
        );

        assert!(
            matches!(form.meta.fill.as_str(), "acroform" | "overlay" | "none"),
            "{}: fill must be acroform | overlay | none, got `{}`",
            form.meta.form_code,
            form.meta.fill
        );
        assert!(
            form.meta.source_url.starts_with("https://www.nvsos.gov/")
                || form.meta.source_url.contains(".gov/"),
            "{}: source_url must be the issuing authority's own domain, got `{}`",
            form.meta.form_code,
            form.meta.source_url
        );
    }
}
