//! Provenance guard for vendored government forms.
//!
//! The repository path is the public bucket path: each bundled blank PDF
//! lives at `notation_templates/<object_path>`, and the sibling markdown
//! template carries the catalog metadata.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("forms crate sits one level under the workspace root")
        .to_path_buf()
}

#[test]
fn vendored_forms_match_repo_paths() {
    let forms = forms::registry().expect("registry loads");
    assert!(!forms.is_empty(), "registry lists no forms");

    for form in &forms {
        assert!(
            form.meta.object_path.starts_with("forms/united_states/"),
            "{}: object path must be bucket-relative under forms/united_states",
            form.meta.code
        );
        assert!(
            form.meta
                .object_path
                .ends_with(&format!("{}.pdf", form.meta.code)),
            "{}: object path must end with the form code stem",
            form.meta.code
        );

        let disk_path = workspace_root()
            .join("notation_templates")
            .join(form.meta.object_path);
        let disk = std::fs::read(&disk_path).unwrap_or_else(|e| {
            panic!(
                "{}: cannot read canonical example {}: {e}",
                form.meta.code,
                disk_path.display()
            )
        });
        assert_eq!(
            disk, form.bytes,
            "{}: on-disk canonical example diverges from bundled bytes",
            form.meta.code
        );
        assert!(
            form.meta.origin_url.starts_with("https://") && form.meta.origin_url.contains(".gov"),
            "{}: origin_url must be an HTTPS government URL, got `{}`",
            form.meta.code,
            form.meta.origin_url
        );
    }
}
