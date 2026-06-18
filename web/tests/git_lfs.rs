//! HTTP-level tests for the Git LFS batch API + transfer endpoints.
//!
//! Driven via `tower::ServiceExt::oneshot` (no socket, no `git-lfs`
//! binary): they prove the batch response shape, the PAT/ACL gate, and
//! the object round-trip through `cloud::StorageService` (the Fs backend
//! in tests). The wire protocol with a real `git-lfs` client is a
//! follow-up; here we validate the server contract.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use base64::Engine as _;
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue};
use sha2::{Digest, Sha256};
use store::entity::person::Role;
use store::entity::{git_access_token, person, project};
use store::test_support::pg;
use store::Db;
use tower::ServiceExt;
use uuid::Uuid;
use web::AppState;

async fn state(db: Db) -> AppState {
    AppState {
        // LFS objects collide across tests, so each gets its own dir.
        storage: std::sync::Arc::new(
            cloud::FsStorage::new(
                std::env::temp_dir().join(format!("navigator-lfs-test-{}", Uuid::now_v7())),
            )
            .await
            .unwrap(),
        ),
        ..web::test_support::app_state(db).await
    }
}

fn basic(pat: &str) -> String {
    let cred = base64::engine::general_purpose::STANDARD.encode(format!("git:{pat}"));
    format!("Basic {cred}")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn lfs_batch_upload_then_download_round_trip() {
    let db = pg().await;

    let admin = person::ActiveModel {
        name: ActiveValue::Set("Nick".into()),
        email: ActiveValue::Set("nick@neonlaw.com".into()),
        role: ActiveValue::Set(Role::Admin),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    let proj = project::ActiveModel {
        name: ActiveValue::Set("Matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    store::git_access_tokens::mint(
        &db,
        admin,
        None,
        git_access_token::SCOPE_WRITE,
        "write-pat",
        Utc::now() + Duration::hours(1),
    )
    .await
    .unwrap();

    let app = web::build_router(state(db).await, std::path::Path::new("/tmp/lfs-public"));

    let content = b"%PDF-1.7 a signed will";
    let oid = sha256_hex(content);
    let base = format!("/projects/{proj}.git/info/lfs/objects");

    // 1. Batch upload: announce the object → expect an upload action.
    let batch_body = format!(
        r#"{{"operation":"upload","transfers":["basic"],"objects":[{{"oid":"{oid}","size":{}}}]}}"#,
        content.len()
    );
    let resp = app
        .clone()
        .oneshot(
            Request::post(format!("{base}/batch"))
                .header(header::AUTHORIZATION, basic("write-pat"))
                .header(header::CONTENT_TYPE, "application/vnd.git-lfs+json")
                .body(Body::from(batch_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["transfer"], "basic");
    let action_href = json["objects"][0]["actions"]["upload"]["href"]
        .as_str()
        .expect("upload action href");
    assert!(
        action_href.ends_with(&format!("{base}/{oid}")),
        "href: {action_href}"
    );

    // 2. PUT the object bytes.
    let resp = app
        .clone()
        .oneshot(
            Request::put(format!("{base}/{oid}"))
                .header(header::AUTHORIZATION, basic("write-pat"))
                .body(Body::from(content.to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "upload must succeed");

    // 3. Batch download → expect a download action.
    let batch_body = format!(
        r#"{{"operation":"download","transfers":["basic"],"objects":[{{"oid":"{oid}","size":{}}}]}}"#,
        content.len()
    );
    let resp = app
        .clone()
        .oneshot(
            Request::post(format!("{base}/batch"))
                .header(header::AUTHORIZATION, basic("write-pat"))
                .header(header::CONTENT_TYPE, "application/vnd.git-lfs+json")
                .body(Body::from(batch_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["objects"][0]["actions"]["download"]["href"].is_string());

    // 4. GET the object → bytes come back intact.
    let resp = app
        .clone()
        .oneshot(
            Request::get(format!("{base}/{oid}"))
                .header(header::AUTHORIZATION, basic("write-pat"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let got = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&got[..], content);
}

#[tokio::test]
async fn lfs_rejects_unauthenticated_and_bad_oid() {
    let db = pg().await;
    let proj = project::ActiveModel {
        name: ActiveValue::Set("Matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    let admin = person::ActiveModel {
        name: ActiveValue::Set("Nick".into()),
        email: ActiveValue::Set("nick@neonlaw.com".into()),
        role: ActiveValue::Set(Role::Admin),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    store::git_access_tokens::mint(
        &db,
        admin,
        None,
        git_access_token::SCOPE_WRITE,
        "write-pat",
        Utc::now() + Duration::hours(1),
    )
    .await
    .unwrap();

    let app = web::build_router(state(db).await, std::path::Path::new("/tmp/lfs-public"));
    let base = format!("/projects/{proj}.git/info/lfs/objects");
    let oid = sha256_hex(b"some content");

    // No Authorization header → 401.
    let resp = app
        .clone()
        .oneshot(
            Request::get(format!("{base}/{oid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Authed PUT whose bytes don't match the claimed oid → 400.
    let resp = app
        .clone()
        .oneshot(
            Request::put(format!("{base}/{oid}"))
                .header(header::AUTHORIZATION, basic("write-pat"))
                .body(Body::from(b"different content".to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
