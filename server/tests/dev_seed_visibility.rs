//! The disposable KIND portfolio must exercise the real participation-based
//! access predicates, not merely create rows that look plausible in SQL.

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
async fn litigation_demo_participation_drives_client_and_lawyer_visibility() {
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

    let leo = store::persons::find_by_email_ci(&surreal, "leo.litigation@example.com")
        .await
        .unwrap()
        .expect("litigation demo client");
    let lawyer = store::persons::find_by_email_ci(&surreal, "lawyer@neonlaw.com")
        .await
        .unwrap()
        .expect("local lawyer login");
    let virgo = store::persons::find_by_email_ci(&surreal, "client@neonlaw.com")
        .await
        .unwrap()
        .expect("existing workshop client");
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
            store::access::visible_projects_as_client(&surreal, Some(leo.id))
                .await
                .unwrap()
        ),
        vec!["Example Signal Labs v. Example Data Systems"],
        "the client participation shows only the litigation matter"
    );
    let mut virgo_names = project_names(
        store::access::visible_projects_as_client(&surreal, Some(virgo.id))
            .await
            .unwrap(),
    );
    virgo_names.sort();
    assert_eq!(
        virgo_names,
        vec![
            "Henderson Bungalow Purchase".to_string(),
            "Simpson v. Flanders".to_string(),
        ],
        "the demo client sees Henderson and the shared simpsons matter, not the litigation one"
    );
    // The shared local `lawyer` login is also a paralegal on the workshop
    // portfolio, so its lawyer lens spans several matters; what this fixture
    // guarantees is that its attorney participation includes the litigation one.
    assert!(
        project_names(
            store::access::visible_projects_as_lawyer(
                &surreal,
                Some(lawyer.id),
                store::persons::Role::Lawyer
            )
            .await
            .unwrap(),
        )
        .contains(&"Example Signal Labs v. Example Data Systems".to_string()),
        "the designated lawyer attorney sees the litigation matter"
    );
    assert!(
        store::access::visible_projects_as_lawyer(
            &surreal,
            Some(unassigned_lawyer.id),
            store::persons::Role::Lawyer,
        )
        .await
        .unwrap()
        .is_empty(),
        "an unassigned lawyer fails closed"
    );

    // Since ENG-81 an admin is scoped by the participation ledger like anyone
    // else. The litigation demo seeds exactly two participants — the client and
    // the lawyer attorney — so the firm principal, who is on neither side of it,
    // does not see it. That is the decision working: privileged reach is a
    // place you navigate to, not a silent widening of this list.
    let admin = store::persons::find_by_email_ci(&surreal, "nick@neonlaw.com")
        .await
        .unwrap()
        .expect("the seeded firm principal");
    let admin_names = project_names(
        store::access::visible_projects(&surreal, Some(admin.id), admin.role)
            .await
            .unwrap(),
    );
    assert!(
        !admin_names.contains(&"Example Signal Labs v. Example Data Systems".to_string()),
        "an admin nobody put on the litigation matter does not see it: {admin_names:?}"
    );
    assert!(
        store::access::visible_projects(&surreal, None, store::persons::Role::Admin)
            .await
            .unwrap()
            .is_empty(),
        "an admin session with no linked person fails closed"
    );
}

/// The shared demo matter holds the ENG-81 line for the fixture Admin.
///
/// `admin@neonlaw.com` is given no participation on `simpsons`, so it is absent
/// from that Admin's list — and because one row gates both the list and
/// `/app/projects/{id}`, the same absence withholds the matter's detail view. An
/// Admin who needs reach navigates to `/app/admin` and grants themselves a row,
/// which is auditable rather than silent.
#[tokio::test]
async fn the_shared_demo_matter_is_withheld_from_an_unassigned_admin() {
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

    let fixture_admin = store::persons::find_by_email_ci(&surreal, "admin@neonlaw.com")
        .await
        .unwrap()
        .expect("the KIND fixture admin");
    let fixture_admin_names = project_names(
        store::access::visible_projects(&surreal, Some(fixture_admin.id), fixture_admin.role)
            .await
            .unwrap(),
    );
    assert!(
        !fixture_admin_names.contains(&"Simpson v. Flanders".to_string()),
        "the fixture admin holds no participation on the demo matter, so it must not \
         appear in their list: {fixture_admin_names:?}"
    );

    // The contrast that proves the seed still wires the matter up at all: the
    // Owner *is* given a row and does see it. Without this, dropping every
    // firm-side row would satisfy the assertion above.
    let owner = store::persons::find_by_email_ci(&surreal, "owner@neonlaw.com")
        .await
        .unwrap()
        .expect("the KIND fixture owner");
    let owner_names = project_names(
        store::access::visible_projects(&surreal, Some(owner.id), owner.role)
            .await
            .unwrap(),
    );
    assert!(
        owner_names.contains(&"Simpson v. Flanders".to_string()),
        "the fixture owner is seeded onto the demo matter and must see it: {owner_names:?}"
    );
}
