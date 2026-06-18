#![allow(clippy::doc_markdown)]
//! Drift guard: the hand-curated OpenAPI document in
//! [`web::openapi::document`] must list exactly the `/api/*` paths
//! that [`web::api::routes`] registers. Without this test the doc
//! silently rots whenever a new route lands.
//!
//! `/openapi.json` and `/api/docs` are deliberately excluded — those
//! are meta-endpoints (the spec itself and the Swagger UI shell), not
//! part of the public API surface the document describes.

use std::collections::BTreeSet;

#[test]
fn openapi_paths_match_registered_api_routes() {
    let registered: BTreeSet<String> = web::api::documented_api_paths()
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let documented: BTreeSet<String> = web::openapi::documented_paths().into_iter().collect();
    assert_eq!(
        registered,
        documented,
        "OpenAPI document drift: paths registered in `api::routes()` (and listed in \
         `api::documented_api_paths`) must match the keys of `openapi::document()[\"paths\"]`. \
         Diff: only in routes = {:?}; only in doc = {:?}",
        registered.difference(&documented).collect::<Vec<_>>(),
        documented.difference(&registered).collect::<Vec<_>>(),
    );
}
