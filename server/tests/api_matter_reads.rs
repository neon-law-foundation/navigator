#![allow(clippy::doc_markdown)]
//! Integration tests for the remaining #866 read doors:
//! `GET /app/api/projects/{id}/documents`, `/app/api/projects/{id}/conversation`,
//! `/app/api/expunge-requests`, `/app/api/notations/{id}/review-documents`.
//!
//! Focus: the client-lens filtering on the client-readable doors (documents and
//! conversation hide internal work product, #782), the lawyer-tier gate on the
//! firm-tool doors (expunge queue, review drafts), and the matter-scope 404.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use portal::session::SessionData;
use portal::{AppState, SessionStore};
use store::persons::Role;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "api-matter-reads-test-key";

struct Fixture {
    app: axum::Router,
    project_id: Uuid,
    notation_id: Uuid,
    lawyer: String,
    client: String,
    outsider: String,
}

fn bearer(person_id: Uuid, role: Role) -> String {
    let mut s = SessionData::fresh("api-mr-sub", role);
    s.person_id = Some(person_id);
    format!("Bearer {}", SessionStore::new(KEY).encode(&s))
}

#[allow(clippy::too_many_lines)]
async fn build_fixture() -> Fixture {
    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("nav-api-mr-{}", Uuid::now_v7())))
            .await
            .unwrap(),
    );
    let project = store::projects::create(
        &surreal,
        &store::projects::NewProject {
            code: format!("matter-{}", Uuid::now_v7()),
            name: "Matter".into(),
            status: "open".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let lawyer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Lawyer", "lawyer@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    store::projects::add_participation(&surreal, project.id, lawyer.id, "lawyer")
        .await
        .unwrap();
    let client = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Client", "client@example.com", Role::Client),
    )
    .await
    .unwrap();
    store::projects::add_participation(&surreal, project.id, client.id, "client")
        .await
        .unwrap();
    let outsider = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Outsider", "outsider@example.com", Role::Lawyer),
    )
    .await
    .unwrap();

    // One client-visible document and one internal, to prove the client-lens filter.
    for (name, visibility) in [
        ("client-note.pdf", store::documents::visibility::CLIENT),
        ("work-product.pdf", store::documents::visibility::INTERNAL),
    ] {
        let args = store::documents::IngestArgs {
            project_id: project.id,
            source: store::documents::source::UPLOAD,
            filename: name,
            kind: "unclassified",
            content_type: "application/pdf",
            description: None,
            secondary_storage_key: None,
            visibility,
        };
        portal::matter_documents::record_document(
            &surreal,
            &storage,
            repos::Author {
                name: "Lawyer",
                email: "lawyer@example.com",
            },
            &args,
            name.as_bytes(),
        )
        .await
        .unwrap();
    }

    // One conversation message.
    store::communications::ingest(
        &surreal,
        &store::communications::IngestArgs {
            project_id: project.id,
            channel: store::communications::channel::PORTAL_MESSAGE,
            direction: store::communications::direction::INBOUND,
            author_person_id: Some(client.id),
            counterparty: None,
            subject: None,
            body: "hello",
            source_ref: None,
            asset_id: None,
            occurred_at: "2026-08-14T00:00:00Z",
        },
    )
    .await
    .unwrap();

    // A notation, a review draft on it, and a pending expunge request.
    let tmpl = store::templates::save_version(
        &surreal,
        None,
        "test__mr_walk",
        store::templates::Version {
            title: "MR walk".into(),
            respondent_type: "person".into(),
            asset_id: None,
            form_code: None,
            kind: None,
            source_commit_sha: None,
        },
    )
    .await
    .unwrap()
    .into_model();
    let notation_id = store::notations::create(
        &surreal,
        &store::notations::NewNotation::new(tmpl.id, client.id, project.id, "BEGIN"),
    )
    .await
    .unwrap()
    .id;
    store::review_documents::upsert_draft(
        &surreal,
        &store::review_documents::NewReviewDocument {
            notation_id,
            kind: "will",
            title: "Will",
            body_html: "<p>draft</p>",
        },
    )
    .await
    .unwrap();
    // A pending expunge request against the client document.
    let doc = store::assets::for_project(&surreal, project.id)
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    store::expunge_requests::create(
        &surreal,
        &store::expunge_requests::NewExpungeRequest {
            project_id: project.id,
            asset_id: doc.id,
            requested_by_person_id: client.id,
            note: None,
        },
    )
    .await
    .unwrap();

    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage,
        ..portal::test_support::app_state(surreal.clone()).await
    };
    Fixture {
        app: server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        project_id: project.id,
        notation_id,
        lawyer: bearer(lawyer.id, Role::Lawyer),
        client: bearer(client.id, Role::Client),
        outsider: bearer(outsider.id, Role::Lawyer),
    }
}

async fn get(fx: &Fixture, path: &str, auth: Option<&str>) -> axum::http::Response<Body> {
    let mut req = Request::builder().method("GET").uri(path);
    if let Some(auth) = auth {
        req = req.header("authorization", auth);
    }
    fx.app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn array_len(resp: axum::http::Response<Body>) -> usize {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .unwrap()
        .as_array()
        .unwrap()
        .len()
}

#[tokio::test]
async fn documents_are_client_lens_filtered() {
    let fx = build_fixture().await;
    let path = format!("/app/api/projects/{}/documents", fx.project_id);

    let firm = get(&fx, &path, Some(&fx.lawyer)).await;
    assert_eq!(firm.status(), StatusCode::OK);
    assert_eq!(array_len(firm).await, 2, "the firm sees both documents");

    let client = get(&fx, &path, Some(&fx.client)).await;
    assert_eq!(client.status(), StatusCode::OK);
    assert_eq!(
        array_len(client).await,
        1,
        "the client sees only the client-visible one"
    );

    assert_eq!(
        get(&fx, &path, Some(&fx.outsider)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&fx, &path, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn conversation_is_readable_by_the_matter() {
    let fx = build_fixture().await;
    let path = format!("/app/api/projects/{}/conversation", fx.project_id);

    assert_eq!(
        get(&fx, &path, Some(&fx.lawyer)).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        get(&fx, &path, Some(&fx.client)).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        get(&fx, &path, Some(&fx.outsider)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&fx, &path, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn expunge_queue_is_lawyer_only() {
    let fx = build_fixture().await;
    let path = "/app/api/expunge-requests";

    let firm = get(&fx, path, Some(&fx.lawyer)).await;
    assert_eq!(firm.status(), StatusCode::OK);
    assert_eq!(array_len(firm).await, 1, "the pending request is queued");

    assert_eq!(
        get(&fx, path, Some(&fx.client)).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get(&fx, path, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn review_documents_are_lawyer_only_and_scoped() {
    let fx = build_fixture().await;
    let path = format!("/app/api/notations/{}/review-documents", fx.notation_id);

    let firm = get(&fx, &path, Some(&fx.lawyer)).await;
    assert_eq!(firm.status(), StatusCode::OK);
    assert_eq!(array_len(firm).await, 1, "the review draft is listed");

    assert_eq!(
        get(&fx, &path, Some(&fx.client)).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get(&fx, &path, Some(&fx.outsider)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&fx, &path, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}
