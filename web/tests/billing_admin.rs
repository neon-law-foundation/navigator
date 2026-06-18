#![allow(clippy::doc_markdown)]
//! Integration coverage for the recurring-billing admin surface:
//! `/portal/admin/coupons` and `/portal/admin/subscriptions`.
//!
//! Drives the real HTTP path (bearer auth, `?format=json` — the branch the
//! `navigator` CLI uses) against a seeded catalog. Covers:
//!
//!   1. Mint a coupon, then open a Nexus subscription with it — the
//!      subscription lands `pending` (retainer-gated) with the coupon's
//!      99% snapshotted onto it, and the coupon's redemption count advances.
//!   2. A discount above the product's list price is rejected (the
//!      below-only guardrail) before any row is written.
//!   3. Activating the project's pending subscriptions (what the signed
//!      retainer triggers) flips it to `active`, so billing can pick it up.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use store::seed;
use tower::ServiceExt;
use web::AppState;

const PROJECT_ID: &str = "01890000-0000-7000-8000-000000000001";

async fn build_app(tag: &str) -> (axum::Router, store::Db) {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("navigator-billing-admin-{tag}")))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&db, &storage).await.unwrap();
    let state: AppState = web::test_support::app_state(db.clone()).await;
    (
        web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR)),
        db,
    )
}

async fn post_json(app: &axum::Router, uri: &str, body: String) -> axum::http::Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("authorization", "Bearer dev")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn get_json(app: &axum::Router, uri: &str) -> axum::http::Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body_json(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn coupon_drives_a_pending_discounted_subscription() {
    let (app, db) = build_app("coupon-flow").await;

    // 1. Mint a 99%-off coupon scoped to Nexus.
    let resp = post_json(
        &app,
        "/portal/admin/coupons?format=json",
        "code=FRIEND99&discount_percent=99&product_code=nexus&max_redemptions=5".to_string(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let coupon = body_json(resp).await;
    assert_eq!(coupon["code"], "FRIEND99");
    assert_eq!(coupon["discount_percent"], 99);
    assert_eq!(coupon["redeemed_count"], 0);

    // 2. Open a Nexus subscription with the coupon, linked to a project.
    let body = format!(
        "product_code=nexus&contact_name=ALPS%20Consulting&contact_email=ami%40alps.example\
         &coupon=FRIEND99&project_id={PROJECT_ID}"
    );
    let resp = post_json(&app, "/portal/admin/subscriptions?format=json", body).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let sub = body_json(resp).await;
    // Retainer-gated: it starts pending, with the 99% snapshotted on.
    assert_eq!(sub["status"], "pending");
    assert_eq!(sub["discount_percent"], 99);
    assert_eq!(sub["product_code"], "nexus");
    assert_eq!(sub["contact_email"], "ami@alps.example");

    // The coupon's redemption count advanced.
    let listed = body_json(get_json(&app, "/portal/admin/coupons?format=json").await).await;
    let friend = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["code"] == "FRIEND99")
        .unwrap();
    assert_eq!(friend["redeemed_count"], 1);

    // 3. The signed retainer activates the project's pending subscriptions.
    let project = uuid::Uuid::parse_str(PROJECT_ID).unwrap();
    let activated = store::subscriptions::activate_pending_for_project(&db, project)
        .await
        .unwrap();
    assert_eq!(activated, 1);
    let subs = body_json(get_json(&app, "/portal/admin/subscriptions?format=json").await).await;
    assert_eq!(subs.as_array().unwrap()[0]["status"], "active");
}

#[tokio::test]
async fn a_discount_above_list_is_rejected() {
    let (app, _db) = build_app("above-list").await;
    // Nexus lists at $2,222 (222200 cents); a $9,999 flat discount is above
    // list and must be refused before any subscription row is written.
    let body = "product_code=nexus&contact_name=Over&contact_email=o%40e.example\
                &discount_amount_cents=999900"
        .to_string();
    let resp = post_json(&app, "/portal/admin/subscriptions?format=json", body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err = body_json(resp).await;
    assert!(
        err["error"].as_str().unwrap().contains("below list"),
        "got: {err}"
    );

    // Nothing was created.
    let subs = body_json(get_json(&app, "/portal/admin/subscriptions?format=json").await).await;
    assert!(subs.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_one_time_product_cannot_take_a_subscription() {
    let (app, _db) = build_app("non-recurring").await;
    // Northstar is a matter-close flat fee, not a recurring product.
    let body = "product_code=northstar&contact_name=X&contact_email=x%40e.example".to_string();
    let resp = post_json(&app, "/portal/admin/subscriptions?format=json", body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err = body_json(resp).await;
    assert!(
        err["error"].as_str().unwrap().contains("recurring"),
        "got: {err}"
    );
}
