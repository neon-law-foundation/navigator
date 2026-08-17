//! The `tenant-server` binary: Navigator with no public face.
//!
//! The white-label shape, for a firm running Navigator behind its own marketing
//! site. The composition it serves lives in [`portal::tenant`], so this binary
//! and the tests that exercise its router cannot disagree.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    portal::hosting::run(portal::tenant::brand()).await
}
