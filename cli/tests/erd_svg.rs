//! Integration test guarding `docs/erd.svg`.
//!
//! The SVG renderer in `cli::erd` is deterministic by construction
//! (alphabetical `BTreeMap` iteration, integer-only arithmetic, no
//! timestamps, no random IDs). This test proves the full
//! pipeline — testcontainer Postgres → migrations → `pg_catalog`
//! introspection → SVG render — is also reproducible: a freshly
//! migrated schema must produce a byte-identical copy of the
//! committed `docs/erd.svg`.
//!
//! When this test fails the most likely cause is a schema-changing
//! migration without a matching `docs/erd.svg` refresh. Regenerate
//! and commit:
//!
//! ```text
//! set -a && source .devx/env && set +a
//! cargo run -p cli -- erd --format svg > docs/erd.svg
//! git add docs/erd.svg
//! ```
//!
//! See `docs/agent-workflows.md` for the maintenance workflow context.

use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use store::test_support::schema;

#[tokio::test]
async fn rendered_svg_matches_committed_docs_erd_svg() {
    let s = schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args(["erd", "--format", "svg", "--database-url"])
        .arg(&s.url)
        .output()
        .expect("run navigator erd --format svg");

    assert!(
        out.status.success(),
        "erd --format svg failed: stderr=\n{}",
        String::from_utf8_lossy(&out.stderr),
    );

    let rendered = String::from_utf8(out.stdout).expect("svg output must be utf-8");

    let committed_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/erd.svg");
    let committed = std::fs::read_to_string(&committed_path).unwrap_or_else(|e| {
        panic!(
            "could not read committed SVG at {}: {e}",
            committed_path.display()
        )
    });

    if rendered != committed {
        // Make the failure obvious. Don't dump the whole SVG (it's
        // ~30KB); show file sizes plus the first divergent line so
        // the operator can decide whether to refresh.
        let rendered_lines: Vec<&str> = rendered.lines().collect();
        let committed_lines: Vec<&str> = committed.lines().collect();
        let first_diff = rendered_lines
            .iter()
            .zip(committed_lines.iter())
            .enumerate()
            .find(|(_, (r, c))| r != c)
            .map_or_else(
                || {
                    format!(
                        "no line-level mismatch up to min(len); rendered.len={}, committed.len={}",
                        rendered_lines.len(),
                        committed_lines.len(),
                    )
                },
                |(i, (r, c))| format!("line {}: rendered={r:?}\n         committed={c:?}", i + 1),
            );
        panic!(
            "docs/erd.svg drifted from a freshly rendered schema.\n\
             rendered: {} bytes\n\
             committed: {} bytes\n\
             {first_diff}\n\n\
             To refresh:\n  set -a && source .devx/env && set +a\n  \
             cargo run -p cli -- erd --format svg > docs/erd.svg",
            rendered.len(),
            committed.len(),
        );
    }
}
