use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::entity::{person, project, testimonial};
use store::test_support::{dri_person, pg, seed_entity};
use tower::ServiceExt;
// Keyed render assertions: assert the page wires up a catalog slot, not
// what the slot currently says. Editing the copy in `en.yml` keeps these
// green; a typo'd/deleted key fails loudly via `t_strict`.
use views::assert_renders;

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn get(state: web::AppState, uri: &str) -> axum::http::Response<Body> {
    web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR))
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn seeded_testimonial_db() -> store::Db {
    let db = pg().await;
    let storage: std::sync::Arc<dyn cloud::StorageService> = std::sync::Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-testimonials-route-test"))
            .await
            .unwrap(),
    );
    store::seed::seed_canonical(&db, &storage).await.unwrap();

    let entity_id = seed_entity(&db).await;
    let dri = dri_person(&db).await;
    let sender = person::ActiveModel {
        name: ActiveValue::Set("A. Client".into()),
        email: ActiveValue::Set("testimonial-route@example.com".into()),
        title: ActiveValue::Set(Some("Founder".into())),
        profile_image_url: ActiveValue::Set(Some("/images/testimonial-route.webp".into())),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let project = project::ActiveModel {
        name: ActiveValue::Set("Nexus proof matter".into()),
        status: ActiveValue::Set("closed".into()),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(dri)),
        client_dri_person_id: ActiveValue::Set(Some(dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    testimonial::ActiveModel {
        project_id: ActiveValue::Set(project.id),
        person_id: ActiveValue::Set(sender.id),
        product_code: ActiveValue::Set(Some("nexus".into())),
        quote: ActiveValue::Set("Nexus made general counsel feel close at hand.".into()),
        attribution_label: ActiveValue::Set(Some("Approved Client".into())),
        consented_at: ActiveValue::Set(Some("2026-06-23T00:00:00Z".into())),
        published_at: ActiveValue::Set(Some("2026-06-24T00:00:00Z".into())),
        display_order: ActiveValue::Set(0),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    db
}

#[tokio::test]
async fn homepage_renders_published_testimonials() {
    let state = web::test_support::app_state(seeded_testimonial_db().await).await;
    let resp = get(state, "/").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // Section heading: catalog copy, keyed — survives copy edits.
    assert_renders!(&body, "testimonials.home_heading");
    // The quote, attribution, and image are DB-seeded data, not UI copy —
    // they stay literal (there is no catalog key for client content).
    assert!(body.contains("Nexus made general counsel feel close at hand."));
    assert!(body.contains("Approved Client"));
    assert!(body.contains("src=\"/images/testimonial-route.webp\""));
}

#[tokio::test]
async fn product_page_renders_only_matching_product_testimonials() {
    let state = web::test_support::app_state(seeded_testimonial_db().await).await;
    let nexus = get(state.clone(), "/services/nexus").await;
    assert_eq!(nexus.status(), StatusCode::OK);
    let nexus_body = body_string(nexus).await;
    assert_renders!(&nexus_body, "testimonials.service_heading");
    assert!(nexus_body.contains("Nexus made general counsel feel close at hand."));

    let litigation = get(state, "/services/litigation").await;
    assert_eq!(litigation.status(), StatusCode::OK);
    let litigation_body = body_string(litigation).await;
    assert!(!litigation_body.contains("Nexus made general counsel feel close at hand."));
}

#[tokio::test]
async fn referral_campaign_renders_modal_overlay_and_local_javascript() {
    let state = web::test_support::app_state(seeded_testimonial_db().await).await;
    let resp = get(state, "/services/litigation?ref=1337lawyers").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;

    assert!(
        body.contains("class=\"modal fade show d-block lawyers-terminal-modal\"")
            && body.contains("role=\"dialog\"")
            && body.contains("aria-modal=\"true\"")
            && body.contains("class=\"modal-backdrop fade show lawyers-terminal-backdrop\""),
        "campaign link must render an open modal overlay, got: {body}"
    );
    assert!(
        body.contains("&gt; wake up...")
            && body.contains("&gt; the matrix has you.")
            && body.contains("&gt; Need to fight for your rights? Follow the white rabbit.")
            && body.contains("Follow 🐰")
            && !body.contains("lawyers-terminal__prompt\"><span"),
        "campaign modal copy should match the referral prompt, got: {body}"
    );
    assert!(
        body.contains("script defer src=\"/public/js/bootstrap.bundle.min.js\"")
            && body.contains("script defer src=\"/public/js/htmx.min.js\"")
            && body.contains("script defer src=\"/public/js/alpine.min.js\""),
        "layout must load vendored JavaScript from /public/js, got: {body}"
    );
    for cdn in [
        "cdn.jsdelivr.net",
        "unpkg.com",
        "cdnjs.cloudflare.com",
        "code.jquery.com",
        "ajax.googleapis.com",
    ] {
        assert!(
            !body.contains(cdn),
            "campaign page must not load JavaScript from CDN host {cdn}: {body}"
        );
    }
}
