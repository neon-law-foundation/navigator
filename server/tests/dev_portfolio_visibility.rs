//! The local fixture is three sample matters shared by the lenses.

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
async fn the_sample_fixture_is_scoped_for_client_lawyer_and_admin() {
    let surreal = mem_surreal().await;
    let (storage, _storage_dir) = storage().await;
    store::seed::seed_environment_with(
        &surreal,
        &storage,
        DeploymentEnvironment::Dev,
        store::seed::BrandSeed::Neon,
        true,
    )
    .await
    .unwrap();

    let expected = store::seed::sample_matter_codes();
    let codes = |mut projects: Vec<store::projects::Project>| {
        projects.sort_by(|a, b| a.code.cmp(&b.code));
        projects.into_iter().map(|p| p.code).collect::<Vec<_>>()
    };
    let mut sorted_expected: Vec<String> = expected.iter().map(ToString::to_string).collect();
    sorted_expected.sort();

    assert_eq!(
        codes(store::projects::all(&surreal).await.unwrap()),
        sorted_expected
    );

    // Both firm-side and client-side lenses reach all three, because the
    // fixture seeds each of them onto every matter. The point of three is that
    // the participation-scoped list has something in it worth reading.
    let lawyer = person_id(&surreal, "lawyer@neonlaw.com").await;
    assert_eq!(
        codes(
            store::access::visible_projects_as_lawyer(
                &surreal,
                Some(lawyer),
                store::persons::Role::Lawyer,
            )
            .await
            .unwrap()
        ),
        sorted_expected
    );

    let client = person_id(&surreal, "client@neonlaw.com").await;
    assert_eq!(
        codes(
            store::access::visible_projects_as_client(&surreal, Some(client))
                .await
                .unwrap()
        ),
        sorted_expected
    );

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
