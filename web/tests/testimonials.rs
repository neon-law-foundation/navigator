use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::entity::{person, project, testimonial};
use store::test_support::{dri_person, pg, seed_entity};
use tower::ServiceExt;

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
    assert!(body.contains("What clients say"));
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
    assert!(nexus_body.contains("Client proof"));
    assert!(nexus_body.contains("Nexus made general counsel feel close at hand."));

    let litigation = get(state, "/services/litigation").await;
    assert_eq!(litigation.status(), StatusCode::OK);
    let litigation_body = body_string(litigation).await;
    assert!(!litigation_body.contains("Nexus made general counsel feel close at hand."));
}
