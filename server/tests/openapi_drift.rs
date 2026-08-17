#![allow(clippy::doc_markdown)]
//! Drift guard: the hand-curated OpenAPI document in
//! [`portal::openapi::document`] must describe exactly the `/app/api/*`
//! operations that [`portal::api::routes`] registers — matched at
//! `(HTTP method, path)` granularity, not path-only. Without this test
//! the doc silently rots whenever a new route or method lands.
//!
//! The path-only predecessor could not see method drift: a `PUT` added
//! to an already-listed path, or an undocumented alias sharing no path
//! key, slipped straight through. Comparing the exact `(method, path)`
//! set closes that gap. Crucially, `api::documented_api_operations()` is
//! *derived from the same table* `api::routes()` builds the router from,
//! so it cannot omit a registered route — an entirely new undocumented
//! path therefore fails this comparison against the document, not just a
//! new method on an existing path. The complementary
//! `web/tests/routes.rs::api_router_operations_match_openapi_document`
//! probes the *live* router as a second, runtime check.
//!
//! `/app/api/openapi.json` and `/app/api` are deliberately excluded — those are
//! documentation surfaces (the spec itself and the Swagger UI shell)
//! mounted outside the API gate by `api::doc_routes`, not part of the
//! public API surface the document describes.

use std::collections::BTreeSet;

#[test]
fn openapi_operations_match_registered_api_routes() {
    let registered: BTreeSet<(String, String)> = portal::api::documented_api_operations()
        .iter()
        .map(|(method, path)| ((*method).to_string(), (*path).to_string()))
        .collect();
    let documented: BTreeSet<(String, String)> = portal::openapi::documented_operations()
        .into_iter()
        .collect();
    assert_eq!(
        registered,
        documented,
        "OpenAPI document drift: the (method, path) operations registered in `api::routes()` \
         (and listed in `api::documented_api_operations`) must match the operations declared in \
         `openapi::document()[\"paths\"]`. \
         Only in routes = {:?}; only in doc = {:?}",
        registered.difference(&documented).collect::<Vec<_>>(),
        documented.difference(&registered).collect::<Vec<_>>(),
    );
}
