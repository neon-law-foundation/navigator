#![allow(clippy::doc_markdown)]
//! Integration tests for `PUT`/`DELETE /app/api/projects/{id}/participants/{role_id}/dri`
//! — the REST doors that designate or clear a matter participant's DRI marker.
//!
//! The write engine (`store::participation::update_participant`) is shared with
//! the lawyer workbench control, so these tests focus on what the REST adapters
//! add: the tier gate (LawyerSession → 401/403), the matter-scope check (a
//! `role_id` from another matter is a bare 404), the actor gate the command
//! enforces (a lawyer who does not already hold the marker is 403), the
//! emptiness rule (clearing the matter's last lawyer DRI is 422), and the live
//! 204s proving the marker actually moves.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use portal::session::SessionData;
use portal::{AppState, SessionStore};
use store::persons::Role;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "api-participant-dri-test-key";

struct Fixture {
    app: axum::Router,
    surreal: store::surreal::SurrealDb,
    project_id: Uuid,
    /// The seeded lawyer DRI and their participation row; the only holder the
    /// fixture starts with, so their session is the one authorised to move the
    /// marker and their own row is the one the emptiness rule protects.
    dri_id: Uuid,
    dri_role_id: Uuid,
    /// A second lawyer on the matter, holding no marker — the designation
    /// target, and the row a peer is added to or cleared from.
    peer_role_id: Uuid,
    /// A lawyer on the matter who is not a DRI: the actor the command refuses.
    nonholder_id: Uuid,
    /// A client-tier participant, to prove the tier gate rejects before scope.
    client_id: Uuid,
}

async fn build_fixture() -> Fixture {
    let surreal = mem_surreal().await;
    let project = store::test_support::seed_project(&surreal, "Matter").await;

    let dri = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role(
            "Accountable Lawyer",
            "dri@example.com",
            Role::Lawyer,
        ),
    )
    .await
    .unwrap();
    store::projects::designate_dri_in_surreal(
        &surreal,
        project.id,
        dri.id,
        store::projects::DriSide::Lawyer,
    )
    .await
    .unwrap();
    let dri_role = store::projects::participation_for_person(&surreal, dri.id, project.id)
        .await
        .unwrap()
        .expect("the DRI designation wrote a membership row");

    let peer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Peer Lawyer", "peer@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    let peer_role = store::projects::add_participation(&surreal, project.id, peer.id, "lawyer")
        .await
        .unwrap();

    let nonholder = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Other Lawyer", "other@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    store::projects::add_participation(&surreal, project.id, nonholder.id, "lawyer")
        .await
        .unwrap();

    let client = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Matter Client", "client@example.com", Role::Client),
    )
    .await
    .unwrap();

    let state = AppState {
        sessions: SessionStore::new(KEY),
        ..portal::test_support::app_state(surreal.clone()).await
    };
    Fixture {
        app: server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        surreal,
        project_id: project.id,
        dri_id: dri.id,
        dri_role_id: dri_role.id,
        peer_role_id: peer_role.id,
        nonholder_id: nonholder.id,
        client_id: client.id,
    }
}

/// A `Bearer` header for a session of `role` acting as `person_id`.
fn bearer(person_id: Uuid, role: Role) -> String {
    let mut session = SessionData::fresh("api-dri-sub", role);
    session.person_id = Some(person_id);
    format!("Bearer {}", SessionStore::new(KEY).encode(&session))
}

async fn dri_call(
    app: &axum::Router,
    method: &str,
    project_id: Uuid,
    role_id: Uuid,
    auth: Option<&str>,
) -> axum::http::Response<Body> {
    let mut req = Request::builder().method(method).uri(format!(
        "/app/api/projects/{project_id}/participants/{role_id}/dri"
    ));
    if let Some(auth) = auth {
        req = req.header("authorization", auth);
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Everyone carrying the matter's lawyer marker right now, sorted.
async fn lawyer_dri_holders(fixture: &Fixture) -> Vec<Uuid> {
    let mut ids = store::participation::holders(
        &fixture.surreal,
        fixture.project_id,
        store::projects::DriSide::Lawyer,
    )
    .await
    .unwrap();
    ids.sort();
    ids
}

#[tokio::test]
async fn a_holder_designates_and_then_clears_a_peer_via_the_api() {
    let fx = build_fixture().await;
    let dri = bearer(fx.dri_id, Role::Lawyer);
    assert_eq!(lawyer_dri_holders(&fx).await, vec![fx.dri_id]);

    // PUT designates the peer — the marker set grows to two.
    let designate = dri_call(&fx.app, "PUT", fx.project_id, fx.peer_role_id, Some(&dri)).await;
    assert_eq!(designate.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        lawyer_dri_holders(&fx).await.len(),
        2,
        "the peer joins the matter's accountable lawyers"
    );

    // DELETE clears the peer — back to the original single holder.
    let clear = dri_call(
        &fx.app,
        "DELETE",
        fx.project_id,
        fx.peer_role_id,
        Some(&dri),
    )
    .await;
    assert_eq!(clear.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        lawyer_dri_holders(&fx).await,
        vec![fx.dri_id],
        "the peer steps off and the original stays accountable"
    );
}

#[tokio::test]
async fn clearing_the_last_lawyer_dri_is_refused_422() {
    let fx = build_fixture().await;
    let dri = bearer(fx.dri_id, Role::Lawyer);

    let resp = dri_call(&fx.app, "DELETE", fx.project_id, fx.dri_role_id, Some(&dri)).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "a matter always keeps a lawyer DRI"
    );
    assert_eq!(
        lawyer_dri_holders(&fx).await,
        vec![fx.dri_id],
        "the refused clear moves nothing"
    );
}

#[tokio::test]
async fn a_lawyer_without_the_marker_is_forbidden() {
    let fx = build_fixture().await;
    let nonholder = bearer(fx.nonholder_id, Role::Lawyer);

    let resp = dri_call(
        &fx.app,
        "PUT",
        fx.project_id,
        fx.peer_role_id,
        Some(&nonholder),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "only a current holder governs that side's accountability"
    );
    assert_eq!(
        lawyer_dri_holders(&fx).await,
        vec![fx.dri_id],
        "a refused designation moves nothing"
    );
}

#[tokio::test]
async fn a_client_caller_is_forbidden_before_scope() {
    let fx = build_fixture().await;
    let client = bearer(fx.client_id, Role::Client);

    let resp = dri_call(
        &fx.app,
        "PUT",
        fx.project_id,
        fx.peer_role_id,
        Some(&client),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_anonymous_caller_is_unauthenticated() {
    let fx = build_fixture().await;

    let resp = dri_call(&fx.app, "PUT", fx.project_id, fx.peer_role_id, None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_role_from_another_matter_is_not_found() {
    let fx = build_fixture().await;
    let dri = bearer(fx.dri_id, Role::Lawyer);

    // A role id that does not name a row on this matter must not disclose
    // whether it exists elsewhere.
    let resp = dri_call(&fx.app, "PUT", fx.project_id, Uuid::now_v7(), Some(&dri)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
