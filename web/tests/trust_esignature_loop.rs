#![allow(clippy::doc_markdown)]
//! End-to-end integration test for the Nevada trust riding the
//! **generalized** (non-retainer-specific) e-signature send path,
//! driven through the **real** `DocuSignSignatureProvider` against a
//! mocked DocuSign HTTP endpoint (wiremock).
//!
//! Proves the prerequisite of Phase 1.3: the same walker + post-intake
//! drive that ships the retainer now carries `trusts__nevada` with no
//! retainer-specific wiring. The loop:
//!   1. Walk the trust questionnaire (trustee_name, trust_property) to
//!      the end. The post-intake drive resolves the trust's workflow
//!      spec + generic storage keys from the template code, renders the
//!      trust instrument with anchored settlor + attorney signature
//!      blocks, and calls the real DocuSign provider.
//!   2. The captive settlor's identity comes from the bound Person row
//!      (the trust questionnaire never asks for an email), proving the
//!      manifest fallback. The notation parks at
//!      sent_for_signature__pending with the returned envelope id.
//!   3. A validly-signed completion callback advances it to END and
//!      archives the signed PDF + Certificate of Completion under the
//!      generic per-notation keys.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, EntityTrait};
use store::{entity, seed};
use tower::ServiceExt;
use web::signature::DocuSignSignatureProvider;
use web::webhook_auth::sign_hmac_sha256_b64;
use web::AppState;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use workflows::{InMemoryRuntime, StateMachineRuntime};

const TEMPLATE_CODE: &str = "trusts__nevada";
const HMAC_KEY: &str = "trust-loop-hmac-key";
const ENVELOPE_ID: &str = "env-trust-1";
const SETTLOR_EMAIL: &str = "capricorn@example.com";

fn completion_body() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "event": "envelope-completed",
        "data": {
            "envelopeId": ENVELOPE_ID,
            "envelopeSummary": { "status": "completed" },
        },
    }))
    .unwrap()
}

fn urlencoding(s: &str) -> String {
    s.replace(' ', "%20").replace('@', "%40")
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn full_trust_signature_loop_reaches_end_through_generalized_path() {
    // 1. Mock DocuSign's envelope-create + document-download endpoints.
    let docusign = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2.1/accounts/acct-guid/envelopes"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(serde_json::json!({"envelopeId": ENVELOPE_ID, "status": "sent"})),
        )
        .mount(&docusign)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v2.1/accounts/acct-guid/envelopes/{ENVELOPE_ID}/documents/combined"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF-signed-trust".to_vec()))
        .mount(&docusign)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v2.1/accounts/acct-guid/envelopes/{ENVELOPE_ID}/documents/certificate"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF-trust-certificate".to_vec()))
        .mount(&docusign)
        .await;

    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-trust-esignature-loop-storage"))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&db, &storage).await.unwrap();
    let storage_handle = storage.clone();
    let tmpl = entity::template::Entity::find()
        .filter(entity::template::Column::Code.eq(TEMPLATE_CODE))
        .one(&db)
        .await
        .unwrap()
        .expect("seed inserts trusts__nevada");
    // The settlor's identity lives on the Person row; the trust
    // questionnaire never captures an email, so the captive recipient
    // must fall back to this row.
    let settlor = entity::person::ActiveModel {
        name: ActiveValue::Set("Capricorn".into()),
        email: ActiveValue::Set(SETTLOR_EMAIL.into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let proj = entity::project::ActiveModel {
        name: ActiveValue::Set("Capricorn trust".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let nid = entity::notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(settlor.id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(proj.id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;

    let runtime = Arc::new(InMemoryRuntime::new());
    let provider = DocuSignSignatureProvider::new(
        docusign.uri(),
        "acct-guid",
        "TOKEN",
        "signer@example.com",
        "Signer",
    );
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(runtime.clone(), email.clone(), storage.clone()),
    );
    let state = AppState {
        storage,
        workflow_runtime,
        questionnaire_runtime: runtime.clone(),
        signature_provider: Arc::new(provider),
        esignature_hmac_key: Some(HMAC_KEY.to_string()),
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    // 2. Walk the trust questionnaire — trustee_name, trust_property.
    //    The final POST drives the generalized send path.
    for value in ["Capricorn", "The family home and a 401k"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/portal/admin/notations/{nid}/step"))
                    .header("authorization", "Bearer dev")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("value={}", urlencoding(value))))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            resp.status() == StatusCode::OK || resp.status() == StatusCode::SEE_OTHER,
            "walk step status {value}: {}",
            resp.status()
        );
    }

    // The real provider returned ENVELOPE_ID and it was persisted; the
    // trust parked at the same wait state the retainer uses.
    let row = entity::notation::Entity::find_by_id(nid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "sent_for_signature__pending");
    assert_eq!(row.signature_request_id.as_deref(), Some(ENVELOPE_ID));

    // Anchor + identity proof: the trust template's signature blocks
    // travelled template -> Typst -> rendered PDF -> DocuSign envelope,
    // and the captive settlor was resolved from the Person row (the
    // questionnaire never asked for an email).
    let envelope_post = docusign
        .received_requests()
        .await
        .expect("mock recorded requests")
        .into_iter()
        .find(|r| r.url.path().ends_with("/envelopes"))
        .expect("an envelope-create POST was made");
    let posted = String::from_utf8_lossy(&envelope_post.body);
    assert!(
        posted.contains("nlsig-client-signature-1"),
        "settlor signature anchor must reach DocuSign: {posted}"
    );
    assert!(
        posted.contains("nlsig-firm-signature-1"),
        "attorney countersignature anchor must reach DocuSign"
    );
    assert!(
        posted.contains(SETTLOR_EMAIL),
        "the captive settlor's Person-row email must address the envelope: {posted}"
    );

    // 3. Provider posts a validly-signed completion callback → END.
    let body = completion_body();
    let signature = sign_hmac_sha256_b64(HMAC_KEY.as_bytes(), &body);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/esignature/any-token")
                .header("content-type", "application/json")
                .header("x-docusign-signature-1", signature)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "body: {}",
        body_string(resp).await
    );

    let row = entity::notation::Entity::find_by_id(nid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "END");

    // 4. The completion webhook archived the executed document set under
    //    the generic per-notation keys (no retainer-specific naming).
    let signed = storage_handle
        .get(&web::retainer_walk::signed_document_storage_key(nid))
        .await
        .expect("signed trust archived")
        .bytes;
    assert_eq!(signed, b"%PDF-signed-trust");
    let cert = storage_handle
        .get(&web::retainer_walk::certificate_of_completion_storage_key(
            nid,
        ))
        .await
        .expect("certificate of completion archived")
        .bytes;
    assert_eq!(cert, b"%PDF-trust-certificate");
}
