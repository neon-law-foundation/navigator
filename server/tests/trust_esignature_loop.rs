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
use portal::signature::DocuSignSignatureProvider;
use portal::webhook_auth::sign_hmac_sha256_b64;
use portal::AppState;
use store::seed;
use store::test_support::mem_surreal;
use tower::ServiceExt;
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

    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-trust-esignature-loop-storage"))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&surreal, &storage).await.unwrap();
    let storage_handle = storage.clone();
    let tmpl = store::templates::resolve(&surreal, None, TEMPLATE_CODE)
        .await
        .unwrap()
        .expect("seed inserts trusts__nevada");
    // The settlor's identity lives on the Person row; the trust
    // questionnaire never captures an email, so the captive recipient
    // must fall back to this row.
    let settlor = store::persons::create(
        &surreal,
        &store::persons::NewPerson::new("Capricorn", SETTLOR_EMAIL),
    )
    .await
    .unwrap();
    let proj = store::projects::create(
        &surreal,
        &store::projects::NewProject {
            code: format!("capricorn-trust-{}", uuid::Uuid::now_v7()),
            name: "Capricorn trust".into(),
            status: "open".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let nid = store::notations::create(
        &surreal,
        &store::notations::NewNotation::new(tmpl.id, settlor.id, proj.id, "BEGIN"),
    )
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
    let email: Arc<dyn portal::email::EmailService> =
        Arc::new(portal::email::CapturingEmail::new());
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
        ..portal::test_support::app_state(surreal.clone()).await
    };
    let app = server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR));

    // 2. Walk the trust questionnaire — trustee_name, trust_property.
    //    The final POST drives the generalized send path.
    for value in ["Capricorn", "The family home and a 401k"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/lawyer/notations/{nid}/step"))
                    .header(
                        "authorization",
                        portal::test_support::lawyer_bearer_header(),
                    )
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

    // The client's final answer parks at `lawyer_review` — no render on the
    // completion request. Lawyer then approves (renders + persists the PDF at
    // `generate_pdf__*`) and sends (creates the real provider's envelope).
    for action in ["approve-send", "send"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/lawyer/notations/{nid}/{action}"))
                    .header(
                        "authorization",
                        portal::test_support::lawyer_bearer_header(),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Both actions are post/redirect/get onto the review screen since it
        // moved to Dioxus: a refresh after dispatching an envelope re-reads the
        // screen instead of re-posting the send.
        assert_eq!(resp.status(), StatusCode::SEE_OTHER, "{action} status");
        assert_eq!(
            resp.headers().get("location").and_then(|v| v.to_str().ok()),
            Some(format!("/lawyer/notations/{nid}/review").as_str()),
            "{action} redirect target",
        );
    }

    // The real provider returned ENVELOPE_ID and it was persisted; the
    // trust parked at the same wait state the retainer uses.
    let row = store::notations::find_by_id(&surreal, nid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "sent_for_signature__pending");
    assert_eq!(
        store::signatures::request_id_for_notation(&surreal, nid)
            .await
            .unwrap()
            .as_deref(),
        Some(ENVELOPE_ID)
    );

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

    let row = store::notations::find_by_id(&surreal, nid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "END");

    // 4. The completion webhook archived the executed document set under
    //    the generic per-notation keys (no retainer-specific naming).
    let signed = storage_handle
        .get(&store::notations::signed_document_storage_key(nid))
        .await
        .expect("signed trust archived")
        .bytes;
    assert_eq!(signed, b"%PDF-signed-trust");
    let cert = storage_handle
        .get(&store::notations::certificate_of_completion_storage_key(
            nid,
        ))
        .await
        .expect("certificate of completion archived")
        .bytes;
    assert_eq!(cert, b"%PDF-trust-certificate");
}
