//! The local development fixture is one Simpsons matter shared by the lenses.

use std::sync::Arc;

use cloud::StorageService;
use store::test_support::mem_surreal;
use store::DeploymentEnvironment;

async fn storage() -> (Arc<dyn StorageService>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        cloud::FsStorage::new(dir.path().to_path_buf())
            .await
            .unwrap(),
    );
    (storage, dir)
}

async fn person_id(surreal: &store::surreal::SurrealDb, email: &str) -> uuid::Uuid {
    store::persons::find_by_email_ci(surreal, email)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("seeded person {email} missing"))
        .id
}

#[tokio::test]
async fn the_simpsons_fixture_is_scoped_for_client_lawyer_and_admin() {
    let surreal = mem_surreal().await;
    let (storage, _storage_dir) = storage().await;
    store::seed::seed_environment(
        &surreal,
        &storage,
        DeploymentEnvironment::Dev,
        store::seed::BrandSeed::Neon,
    )
    .await
    .unwrap();

    let all = store::projects::all(&surreal).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].code, "simpsons");

    let lawyer = person_id(&surreal, "lawyer@neonlaw.com").await;
    let lawyer_projects = store::access::visible_projects_as_lawyer(
        &surreal,
        Some(lawyer),
        store::persons::Role::Lawyer,
    )
    .await
    .unwrap();
    assert_eq!(lawyer_projects[0].code, "simpsons");

    let client = person_id(&surreal, "client@neonlaw.com").await;
    let client_projects = store::access::visible_projects_as_client(&surreal, Some(client))
        .await
        .unwrap();
    assert_eq!(client_projects[0].code, "simpsons");

    let admin = person_id(&surreal, "admin@neonlaw.com").await;
    assert!(store::access::visible_projects_as_lawyer(
        &surreal,
        Some(admin),
        store::persons::Role::Admin,
    )
    .await
    .unwrap()
    .is_empty());
}
