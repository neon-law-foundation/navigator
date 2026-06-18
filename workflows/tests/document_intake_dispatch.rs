//! Integration test for the reusable `document_intake__*` step.
//!
//! Drives the dispatch through the shared `workflows::dispatch_step`
//! registry — the same arm the `workflows-service` worker runs inside
//! `ctx.run` — and asserts the provided artifact lands as a
//! content-addressed blob + `documents` row on the notation's project,
//! via `store::documents::ingest_bytes`. Needs Postgres (testcontainers)
//! because the side effect writes real rows.

use std::sync::Arc;

use sea_orm::EntityTrait;
use workflows::{dispatch_step, IntakeArtifact, IntakePayload, StateName, StepDeps};

async fn fs_storage() -> Arc<dyn cloud::StorageService> {
    Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-intake-dispatch-test"))
            .await
            .expect("temp FsStorage"),
    )
}

fn deps(db: store::Db, storage: Arc<dyn cloud::StorageService>) -> StepDeps {
    // Email is unused by the intake arm; any EmailService satisfies the
    // struct.
    StepDeps::new(
        Arc::new(workflows::CapturingEmail::new()),
        storage,
        Some(db),
    )
}

#[tokio::test]
async fn document_intake_files_a_text_transcript_into_the_matter() {
    let db = store::test_support::pg().await;
    let notation_id = store::test_support::seed_notation(&db).await;
    let project_id = store::entity::notation::Entity::find_by_id(notation_id)
        .one(&db)
        .await
        .unwrap()
        .expect("seeded notation")
        .project_id;

    let storage = fs_storage().await;
    let deps = deps(db.clone(), storage.clone());

    let payload = serde_json::to_string(&IntakePayload {
        kind: "transcript".into(),
        filename: "sitting-transcript.txt".into(),
        artifact: IntakeArtifact::Text {
            text: "Consent given. Executor: Aries. Trustee: Capricorn.".into(),
        },
    })
    .unwrap();

    dispatch_step(
        &deps,
        notation_id,
        &StateName::from("document_intake__transcript"),
        Some(&payload),
    )
    .await
    .expect("document_intake dispatch files the transcript");

    // A `documents` row landed on the notation's project, carrying the
    // intake's kind/filename and the `upload` provenance.
    let docs = store::entity::document::Entity::find()
        .all(&db)
        .await
        .unwrap();
    let doc = docs
        .iter()
        .find(|d| d.project_id == project_id)
        .expect("a document filed on the project");
    assert_eq!(doc.kind, "transcript");
    assert_eq!(doc.filename, "sitting-transcript.txt");
    assert_eq!(doc.source, "upload");

    // And the bytes are retrievable from storage through the blob.
    let blob = store::entity::blob::Entity::find_by_id(doc.blob_id)
        .one(&db)
        .await
        .unwrap()
        .expect("blob row");
    assert_eq!(blob.content_type, "text/plain");
    let stored = storage.get(&blob.storage_key).await.unwrap();
    assert_eq!(
        stored.bytes,
        b"Consent given. Executor: Aries. Trustee: Capricorn."
    );
}

#[tokio::test]
async fn document_intake_link_artifact_files_a_uri_list_pointer() {
    let db = store::test_support::pg().await;
    let notation_id = store::test_support::seed_notation(&db).await;
    let storage = fs_storage().await;
    let deps = deps(db.clone(), storage.clone());

    let payload = serde_json::to_string(&IntakePayload {
        kind: "transcript".into(),
        filename: "zoom-recording.url".into(),
        artifact: IntakeArtifact::Link {
            url: "https://zoom.example/rec/abc123".into(),
        },
    })
    .unwrap();

    dispatch_step(
        &deps,
        notation_id,
        &StateName::from("document_intake__transcript"),
        Some(&payload),
    )
    .await
    .expect("link intake dispatch succeeds");

    let doc = store::entity::document::Entity::find()
        .all(&db)
        .await
        .unwrap()
        .into_iter()
        .find(|d| d.filename == "zoom-recording.url")
        .expect("link pointer document filed");
    let blob = store::entity::blob::Entity::find_by_id(doc.blob_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blob.content_type, "text/uri-list");
    let stored = storage.get(&blob.storage_key).await.unwrap();
    assert_eq!(stored.bytes, b"https://zoom.example/rec/abc123");
}
