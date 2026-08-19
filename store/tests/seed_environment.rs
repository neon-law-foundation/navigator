//! Integration coverage for environment-aware seed orchestration.
//!
//! The Simpsons development fixture is a `SurrealDB` projects-cluster
//! concern. These tests assert the public project and participation read seams.

use std::sync::Arc;

use store::persons::{self, NewPerson, Role};
use store::projects::{self, DriSide, NewProject};
use store::test_support::mem_surreal;
use store::DeploymentEnvironment;

async fn storage() -> Arc<dyn cloud::StorageService> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "navigator-seed-env-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed),
    ));
    Arc::new(cloud::FsStorage::new(dir).await.unwrap())
}

#[tokio::test]
async fn production_seed_has_no_disposable_projects_or_people() {
    let surreal = mem_surreal().await;
    let storage = storage().await;

    let report = store::seed::seed_environment(
        &surreal,
        &storage,
        DeploymentEnvironment::Production,
        store::seed::BrandSeed::Neon,
    )
    .await
    .unwrap();

    assert!(projects::all(&surreal).await.unwrap().is_empty());
    for email in ["lawyer@neonlaw.com", "client@neonlaw.com"] {
        assert!(
            persons::find_by_email_ci(&surreal, email)
                .await
                .unwrap()
                .is_none(),
            "production must not contain {email}"
        );
    }
    assert_eq!(report.projects_inserted, 0);
}

#[tokio::test]
async fn development_seed_opens_only_simpsons_with_dris() {
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

    let simpsons = projects::find_by_code(&surreal, "simpsons")
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("Simpsons matter");
    assert_eq!(simpsons.status, "open");
    let client = persons::find_by_email_ci(&surreal, "client@neonlaw.com")
        .await
        .unwrap()
        .expect("litigation client");
    let lawyer = persons::find_by_email_ci(&surreal, "lawyer@neonlaw.com")
        .await
        .unwrap()
        .expect("lawyer fixture");
    let participations = projects::participations_for_project(&surreal, simpsons.id)
        .await
        .unwrap();
    assert!(participations.iter().any(|row| {
        row.person_id == client.id && row.participation == "client" && row.is_client_dri
    }));
    assert!(participations.iter().any(|row| {
        row.person_id == lawyer.id && row.participation == "attorney" && row.is_lawyer_dri
    }));

    assert_eq!(projects::all(&surreal).await.unwrap().len(), 1);
}

#[tokio::test]
async fn development_seed_is_idempotent_and_repairs_participation_drift() {
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
    let simpsons = projects::find_by_code(&surreal, "simpsons")
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("Simpsons matter");
    let client = persons::find_by_email_ci(&surreal, "client@neonlaw.com")
        .await
        .unwrap()
        .expect("litigation client");
    let role = projects::participations_for_project(&surreal, simpsons.id)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.person_id == client.id)
        .expect("client participation");
    projects::update_participation(&surreal, role.id, client.id, "paralegal")
        .await
        .unwrap();
    let before = projects::all(&surreal).await.unwrap().len();

    store::seed::seed_environment(
        &surreal,
        &storage,
        DeploymentEnvironment::Dev,
        store::seed::BrandSeed::Neon,
    )
    .await
    .unwrap();

    assert_eq!(projects::all(&surreal).await.unwrap().len(), before);
    let repaired = projects::participations_for_project(&surreal, simpsons.id)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.person_id == client.id)
        .expect("repaired client participation");
    assert_eq!(repaired.participation, "client");
    assert!(repaired.is_client_dri);
}

#[tokio::test]
async fn development_seed_does_not_claim_a_same_named_project() {
    let surreal = mem_surreal().await;
    let storage = storage().await;

    store::seed::seed_canonical(&surreal, &storage)
        .await
        .unwrap();
    let owner = persons::create(
        &surreal,
        &NewPerson::with_role(
            "Unrelated Litigant",
            "unrelated-litigation@example.com",
            Role::Client,
        ),
    )
    .await
    .unwrap();
    let squatter = projects::create(
        &surreal,
        &NewProject {
            code: "unrelated-litigation".into(),
            name: "Example Signal Labs v. Example Data Systems".into(),
            status: "closed".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    projects::designate_dri_in_surreal(&surreal, squatter.id, owner.id, DriSide::Client)
        .await
        .unwrap();

    store::seed::seed_environment(
        &surreal,
        &storage,
        DeploymentEnvironment::Dev,
        store::seed::BrandSeed::Neon,
    )
    .await
    .unwrap();

    assert_eq!(
        projects::find_by_id(&surreal, squatter.id)
            .await
            .unwrap()
            .expect("unrelated project survives")
            .status,
        "closed"
    );
    assert_ne!(
        projects::find_by_code(&surreal, "simpsons")
            .await
            .unwrap()
            .expect("seeded Simpsons matter")
            .id,
        squatter.id
    );
}

/// The brand layer is the one seed besides the canonical set that reaches
/// production, so this asserts it there rather than in `dev`: a production
/// boot must carry every box we actually answer mail at.
///
/// Their being real is the point. `Address.yaml` used to sit in the disposable
/// Simpsons development fixture, which supplies the local matter rows
/// existed only on a developer's laptop.
///
/// One boot carries them all now. The firm and the Foundation seeded from
/// separate brand directories while they were separate deployments; they are
/// one binary, so there is one layer. They stay separate *entities* — asserted
/// by `each_entity_holds_only_its_own_box` below — which is the boundary that
/// actually matters for client mail.
#[tokio::test]
async fn a_production_boot_carries_every_box_we_answer_mail_at() {
    let surreal = mem_surreal().await;
    let storage = storage().await;

    store::seed::seed_environment(
        &surreal,
        &storage,
        DeploymentEnvironment::Production,
        store::seed::BrandSeed::Neon,
    )
    .await
    .unwrap();

    // The mail centre itself is a row too, and `seed_mailrooms` synthesizes a
    // placeholder address for it because `mailrooms.address_id` is NOT NULL.
    // That placeholder is not a box anyone receives mail at, so it is filtered
    // out here rather than folded into the expected list.
    assert!(
        store::mailrooms::find_by_name(&surreal, "Ridgeview Mail Center")
            .await
            .unwrap()
            .is_some(),
        "a production boot carries the mail centre its boxes live in"
    );

    let mut boxes: Vec<String> = store::addresses::list_all(&surreal)
        .await
        .unwrap()
        .into_iter()
        .map(|a| a.line1)
        .filter(|line| !line.starts_with("(via mailroom:"))
        .collect();
    boxes.sort();
    assert_eq!(
        boxes,
        vec![
            "5150 Mae Anne Ave Ste 405-9002".to_string(),
            "5150 Mae Anne Ave Ste 405-9005".to_string(),
            "5150 Mae Anne Ave Ste 405-9011".to_string(),
            "5150 Mae Anne Ave Ste 405-9999".to_string(),
        ],
        "a boot carries every box we hold and nothing else"
    );

    assert!(
        store::entities::find_by_name(&surreal, "Yakcobieus Industries PC")
            .await
            .unwrap()
            .is_some(),
        "and the Firm's own California law corporation"
    );
}

/// The retired partnership's boxes do not seed.
///
/// `Neon Law` held four of them, one per state it answered mail in, and
/// every one has left both the registry and the address layer. This is not
/// tidiness: within the Ridgeview Mail Center the box number is the whole
/// address, so a `405-9777` row surviving here would route mail to a box the
/// firm no longer rents — and the three out-of-state addresses would assert a
/// presence in states no current entity holds a box in.
#[tokio::test]
async fn the_retired_partnerships_boxes_do_not_seed() {
    let surreal = mem_surreal().await;
    let storage = storage().await;

    store::seed::seed_environment(
        &surreal,
        &storage,
        DeploymentEnvironment::Production,
        store::seed::BrandSeed::Neon,
    )
    .await
    .unwrap();

    assert!(
        store::entities::find_by_name(&surreal, "Neon Law")
            .await
            .unwrap()
            .is_none(),
        "the retired partnership is out of the canonical registry"
    );

    let lines: Vec<String> = store::addresses::list_all(&surreal)
        .await
        .unwrap()
        .into_iter()
        .map(|a| a.line1)
        .collect();
    for retired in [
        "5150 Mae Anne Ave Ste 405-9777",
        "1990 N California Blvd Ste 800",
        "12 E 49th St 18th Floor",
        "720 Seneca St Ste 107-715",
    ] {
        assert!(
            !lines.iter().any(|line| line == retired),
            "{retired} belonged to the retired partnership and must not seed"
        );
    }
}

/// One layer, still two entities: each box belongs to exactly one of them.
///
/// This is what the brand split was actually protecting, and it is the half
/// that survives consolidation. The firm's box and the Foundation's share a
/// street, a suite, and a ZIP at the same mail center, so within that facility
/// the box number is the whole address — a row keyed to the wrong entity
/// misdelivers one organization's mail to the other rather than bouncing, and
/// "they all seeded" would look correct in any test that only counts rows.
#[tokio::test]
async fn each_entity_holds_only_its_own_box() {
    let surreal = mem_surreal().await;
    let storage = storage().await;

    store::seed::seed_environment(
        &surreal,
        &storage,
        DeploymentEnvironment::Production,
        store::seed::BrandSeed::Neon,
    )
    .await
    .unwrap();

    for (name, expected) in [
        (
            store::seed::FIRM_ENTITY_NAME,
            "5150 Mae Anne Ave Ste 405-9002",
        ),
        ("Neon Law Foundation", "5150 Mae Anne Ave Ste 405-9999"),
    ] {
        let entity = store::entities::find_by_name(&surreal, name)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{name} seeds"));
        let boxes: Vec<String> = store::addresses::for_entity(&surreal, entity.id)
            .await
            .unwrap()
            .into_iter()
            .map(|a| a.line1)
            .collect();
        assert_eq!(
            boxes,
            vec![expected.to_string()],
            "{name} holds exactly its own box"
        );
    }
}

/// A white-label tenant is a deliberate "seed nothing": it runs our
/// application but is not us, so none of our entities' postal identities
/// belong in its database.
#[tokio::test]
async fn a_tenant_boot_carries_none_of_our_addresses() {
    let surreal = mem_surreal().await;
    let storage = storage().await;

    store::seed::seed_environment(
        &surreal,
        &storage,
        DeploymentEnvironment::Production,
        store::seed::BrandSeed::Tenant,
    )
    .await
    .unwrap();

    assert!(store::addresses::list_all(&surreal)
        .await
        .unwrap()
        .is_empty());
}

/// Re-running a boot is the ordinary case — every deployment seeds on every
/// start — so the brand layer has to be idempotent like the two around it.
#[tokio::test]
async fn the_brand_layer_is_idempotent_across_boots() {
    let surreal = mem_surreal().await;
    let storage = storage().await;

    for _ in 0..2 {
        store::seed::seed_environment(
            &surreal,
            &storage,
            DeploymentEnvironment::Production,
            store::seed::BrandSeed::Neon,
        )
        .await
        .unwrap();
    }

    let second = store::seed::seed_brand(&surreal, store::seed::BrandSeed::Neon)
        .await
        .unwrap();
    assert_eq!(second.addresses_inserted, 0);
    assert_eq!(
        second.entities_inserted, 0,
        "the brand layer seeds entities too, and re-seeding must not duplicate them"
    );
    // Four boxes plus the mail centre's own placeholder address. The mailroom
    // seed is find-or-create on a UNIQUE name, so a second boot must not mint
    // a second facility — nor a second placeholder address behind it.
    assert_eq!(second.mailrooms_inserted, 0);
    assert_eq!(store::addresses::list_all(&surreal).await.unwrap().len(), 5);
}

/// The `dev` portfolio's simulated mail still arrives after the mail centre
/// moved layers.
///
/// `seed_letters` resolves its mailroom by name and *skips* a record it cannot
/// find, so moving `seed_mailrooms` from the Simpsons development fixture into the
/// brand layer put a cross-layer ordering dependency between them. It holds
/// only because `seed_environment` applies the brand layer before the
/// portfolio; reverse those two calls and this suite still passes everywhere
/// except here, with the mail silently absent rather than an error.
#[tokio::test]
async fn the_dev_portfolios_mail_survives_the_mailroom_moving_layers() {
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

    let letters = store::letters::list_all(&surreal).await.unwrap();
    assert!(
        !letters.is_empty(),
        "a dev boot seeds the simulated mail; an empty set means the mailroom \
         lookup silently skipped every record"
    );
    assert!(letters
        .iter()
        .any(|l| l.summary == "Notice of Intent to Lien"));
}
