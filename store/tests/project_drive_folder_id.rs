//! Project Drive-folder addresses are owned by the `SurrealDB` projects cluster.

use store::projects::{create, set_drive_folder_id, NewProject, ProjectStoreError};
use store::test_support::{mem_surreal, seed_entity};

async fn project(surreal: &store::surreal::SurrealDb, code: &str) -> store::projects::Project {
    create(
        surreal,
        &NewProject {
            code: code.to_string(),
            name: code.to_string(),
            status: "open".to_string(),
            entity_id: seed_entity(surreal).await,
            ..Default::default()
        },
    )
    .await
    .expect("create matter")
}

#[tokio::test]
async fn projects_schema_rejects_a_duplicate_matter_code() {
    let surreal = mem_surreal().await;
    project(&surreal, "drive-unique").await;

    assert!(matches!(
        create(
            &surreal,
            &NewProject {
                code: "drive-unique".to_string(),
                name: "Duplicate code".to_string(),
                status: "open".to_string(),
                entity_id: seed_entity(&surreal).await,
                ..Default::default()
            },
        )
        .await,
        Err(ProjectStoreError::CodeTaken)
    ));
}

#[tokio::test]
async fn a_folder_address_is_written_on_the_authoritative_project_record() {
    let surreal = mem_surreal().await;
    let matter = project(&surreal, "drive-address").await;

    let updated = set_drive_folder_id(&surreal, matter.id, Some("1QaBcD_2-efG"))
        .await
        .expect("set address")
        .expect("matter exists");
    assert_eq!(updated.drive_folder_id.as_deref(), Some("1QaBcD_2-efG"));
}
