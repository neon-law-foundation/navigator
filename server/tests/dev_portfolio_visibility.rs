//! The development portfolio must exercise the same visibility helpers the
//! lawyer workbench and client portal use. These assertions deliberately seed
//! the real portfolio instead of creating ad-hoc matter fixtures.

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
async fn development_portfolio_is_scoped_for_clients_lawyer_and_admin() {
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
    let all_names: std::collections::BTreeSet<String> =
        all.iter().map(|project| project.name.clone()).collect();
    assert!(all_names.len() >= 6, "portfolio must have multiple matters");

    // The training cohort layers disposable starter matters (`dev-training-*`)
    // scoped to their own trainer, so the practice-portfolio lawyer sees the
    // practice portfolio, not the training walkthroughs. Since ENG-81 the admin
    // is scoped the same way — the assertion below is that the seed puts them on
    // their matters, not that a bypass hands them everything.
    let portfolio_names: std::collections::BTreeSet<String> = all
        .iter()
        .filter(|project| !project.code.starts_with("dev-training-"))
        .map(|project| project.name.clone())
        .collect();

    let lawyer = person_id(&surreal, store::seed::DEV_PORTFOLIO_LAWYER_EMAIL).await;
    let lawyer_projects = store::access::visible_projects_as_lawyer(
        &surreal,
        Some(lawyer),
        store::persons::Role::Lawyer,
    )
    .await
    .unwrap();
    let lawyer_names: std::collections::BTreeSet<String> = lawyer_projects
        .iter()
        .map(|project| project.name.clone())
        .collect();
    assert_eq!(
        lawyer_names, portfolio_names,
        "seeded lawyer sees assigned portfolio"
    );

    let virgo = person_id(&surreal, "client@neonlaw.com").await;
    let aries = person_id(&surreal, "aries@example.com").await;
    // `client@neonlaw.com` is the client of record on both the Henderson matter and
    // the shared five-role `simpsons` demo matter, so the client lens sees both.
    let virgo_names: std::collections::BTreeSet<String> =
        store::access::visible_projects_as_client(&surreal, Some(virgo))
            .await
            .unwrap()
            .into_iter()
            .map(|project| project.name)
            .collect();
    assert_eq!(
        virgo_names,
        std::collections::BTreeSet::from([
            store::seed::DEV_PORTFOLIO_HENDERSON_NAME.to_string(),
            "Simpson v. Flanders".to_string(),
        ]),
    );
    let aries_project = all
        .iter()
        .find(|project| project.name == "Sagebrush LLC Formation")
        .unwrap();
    assert!(
        !store::access::can_see_project_as_client(&surreal, Some(virgo), aries_project.id)
            .await
            .unwrap(),
        "a client must not see another seeded client’s matter"
    );
    assert!(
        store::access::can_see_project_as_client(&surreal, Some(aries), aries_project.id)
            .await
            .unwrap()
    );

    // The firm principal is an admin, and since ENG-81 that buys no silent
    // project-scoping reach: they see exactly the matters the seed put them on.
    // The seed is what has to keep the local workbench non-empty now, so this
    // asserts both halves — the admin sees their own matters, and they do not
    // see a matter nobody assigned them to.
    let admin = person_id(&surreal, "nick@neonlaw.com").await;
    let admin_projects = store::access::visible_projects_as_lawyer(
        &surreal,
        Some(admin),
        store::persons::Role::Admin,
    )
    .await
    .unwrap();
    let admin_names: std::collections::BTreeSet<String> = admin_projects
        .iter()
        .map(|project| project.name.clone())
        .collect();
    assert!(
        !admin_names.is_empty(),
        "the seed must leave the firm principal on their own matters, or the \
         local workbench opens empty"
    );
    for name in &admin_names {
        assert!(all_names.contains(name), "{name} is not a seeded matter");
    }
    assert!(
        admin_names.len() < all_names.len(),
        "an admin is participation-scoped like every other firm tier — seeing \
         every seeded matter would mean the bypass is still in place"
    );
}
