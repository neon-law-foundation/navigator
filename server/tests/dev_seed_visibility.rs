//! The Simpsons development fixture exercises participation-based visibility.

use std::sync::Arc;

use store::test_support::mem_surreal;
use store::DeploymentEnvironment;

async fn storage() -> Arc<dyn cloud::StorageService> {
    Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!(
            "navigator-dev-seed-visibility-{}",
            uuid::Uuid::now_v7()
        )))
        .await
        .unwrap(),
    )
}

fn project_names(rows: Vec<store::projects::Project>) -> Vec<String> {
    rows.into_iter().map(|row| row.name).collect()
}

#[tokio::test]
async fn simpsons_participation_drives_client_and_lawyer_visibility() {
    let surreal = mem_surreal().await;
    let storage = storage().await;
    store::seed::seed_environment(
        &surreal,
        &storage,
        DeploymentEnvironment::Dev,
        store::seed::BrandSeed::Neon,
    )
    .await
    .unwrap();

    let client = store::persons::find_by_email_ci(&surreal, "client@neonlaw.com")
        .await
        .unwrap()
        .expect("local client login");
    let lawyer = store::persons::find_by_email_ci(&surreal, "lawyer@neonlaw.com")
        .await
        .unwrap()
        .expect("local lawyer login");
    let unassigned_lawyer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role(
            "Unassigned Lawyer",
            "unassigned.lawyer@neonlaw.com",
            store::persons::Role::Lawyer,
        ),
    )
    .await
    .unwrap();

    assert_eq!(
        project_names(
            store::access::visible_projects_as_client(&surreal, Some(client.id))
                .await
                .unwrap(),
        ),
        vec!["Simpson v. Flanders"],
    );
    assert_eq!(
        project_names(
            store::access::visible_projects_as_lawyer(
                &surreal,
                Some(lawyer.id),
                store::persons::Role::Lawyer,
            )
            .await
            .unwrap(),
        ),
        vec!["Simpson v. Flanders"],
    );
    assert!(store::access::visible_projects_as_lawyer(
        &surreal,
        Some(unassigned_lawyer.id),
        store::persons::Role::Lawyer,
    )
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn simpsons_is_withheld_from_the_unassigned_admin() {
    let surreal = mem_surreal().await;
    let storage = storage().await;
    store::seed::seed_environment(
        &surreal,
        &storage,
        DeploymentEnvironment::Dev,
        store::seed::BrandSeed::Neon,
    )
    .await
    .unwrap();

    let admin = store::persons::find_by_email_ci(&surreal, "admin@neonlaw.com")
        .await
        .unwrap()
        .expect("fixture admin");
    assert!(
        store::access::visible_projects(&surreal, Some(admin.id), admin.role)
            .await
            .unwrap()
            .is_empty()
    );

    let owner = store::persons::find_by_email_ci(&surreal, "owner@neonlaw.com")
        .await
        .unwrap()
        .expect("fixture owner");
    assert_eq!(
        project_names(
            store::access::visible_projects(&surreal, Some(owner.id), owner.role)
                .await
                .unwrap(),
        ),
        vec!["Simpson v. Flanders"],
    );
}
