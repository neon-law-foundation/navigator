//! Database-backed apply tests — the real upsert path against a
//! testcontainers Postgres (one schema per test via `store::test_support`).

use import::{apply, parse, Outcome, Payload};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use store::entity::{entity, entity_type, jurisdiction, person, person_entity_role};
use store::test_support::pg;
use store::Db;

/// `pg()` migrates a fresh schema but seeds no reference data; the
/// importer resolves entity-type and jurisdiction by name/code, so the
/// rows it resolves against must exist first.
async fn seed_reference_data(db: &Db) {
    entity_type::ActiveModel {
        name: Set("501(c)(3) Non-Profit".to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("seed entity_type");

    for (code, name) in [
        ("WA", "Washington"),
        ("MN", "Minnesota"),
        ("IL", "Illinois"),
        ("NY", "New York"),
    ] {
        jurisdiction::ActiveModel {
            name: Set(name.to_string()),
            code: Set(code.to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed jurisdiction");
    }
}

fn sample() -> Payload {
    parse(SAMPLE).expect("parse sample payload")
}

#[tokio::test]
async fn first_import_creates_orgs_people_and_links() {
    let db = pg().await;
    seed_reference_data(&db).await;

    let report = apply(&db, &sample()).await.expect("apply");

    assert!(!report.has_errors(), "summary: {}", report.summary());
    assert_eq!(report.count(Outcome::Created), 10, "4 orgs + 6 people");
    assert!(report
        .organizations
        .iter()
        .all(|r| r.status == Outcome::Created));
    assert!(report.people.iter().all(|r| r.status == Outcome::Created));

    // Entities, persons, and the client_contact links all landed.
    assert_eq!(entity::Entity::find().all(&db).await.unwrap().len(), 4);
    assert_eq!(person::Entity::find().all(&db).await.unwrap().len(), 6);
    let links = person_entity_role::Entity::find().all(&db).await.unwrap();
    assert_eq!(links.len(), 6);
    assert!(links.iter().all(|l| l.role == "client_contact"));

    // The canonical URL and org phone were stored on the entity.
    let njp = entity::Entity::find()
        .filter(entity::Column::Name.eq("Northwest Justice Project"))
        .one(&db)
        .await
        .unwrap()
        .expect("njp exists");
    assert_eq!(njp.url.as_deref(), Some("https://nwjustice.org"));
    assert_eq!(njp.phone.as_deref(), Some("206-464-1519"));

    // The person carries title + phone, and defaults to the client tier.
    let abigail = person::Entity::find()
        .filter(person::Column::Email.eq("adaquiz@nwjustice.org"))
        .one(&db)
        .await
        .unwrap()
        .expect("abigail exists");
    assert_eq!(abigail.title.as_deref(), Some("Executive Director"));
    assert_eq!(abigail.role, person::Role::Client);
}

#[tokio::test]
async fn reimport_is_idempotent() {
    let db = pg().await;
    seed_reference_data(&db).await;

    apply(&db, &sample()).await.expect("first apply");
    let second = apply(&db, &sample()).await.expect("second apply");

    assert_eq!(second.count(Outcome::Created), 0);
    assert_eq!(second.count(Outcome::Unchanged), 10);
    // No duplicate rows from the re-run.
    assert_eq!(entity::Entity::find().all(&db).await.unwrap().len(), 4);
    assert_eq!(person::Entity::find().all(&db).await.unwrap().len(), 6);
    assert_eq!(
        person_entity_role::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .len(),
        6
    );
}

#[tokio::test]
async fn reimport_with_changed_title_updates_only_that_person() {
    let db = pg().await;
    seed_reference_data(&db).await;
    apply(&db, &sample()).await.expect("first apply");

    let changed = SAMPLE.replace(
        "\"title\": \"IT Director\"",
        "\"title\": \"Chief Technology Officer\"",
    );
    let report = apply(&db, &parse(&changed).unwrap()).await.expect("apply");

    assert_eq!(report.count(Outcome::Updated), 1);
    assert_eq!(report.count(Outcome::Unchanged), 9);
    let marv = person::Entity::find()
        .filter(person::Column::Email.eq("mgordon@mylegalaid.org"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(marv.title.as_deref(), Some("Chief Technology Officer"));
}

#[tokio::test]
async fn unknown_jurisdiction_fails_only_its_row() {
    let db = pg().await;
    // Deliberately omit Washington so njp can't resolve its jurisdiction.
    entity_type::ActiveModel {
        name: Set("501(c)(3) Non-Profit".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    for (code, name) in [("MN", "Minnesota"), ("IL", "Illinois"), ("NY", "New York")] {
        jurisdiction::ActiveModel {
            name: Set(name.to_string()),
            code: Set(code.to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let report = apply(&db, &sample()).await.expect("apply");

    // njp org failed; the three resolvable orgs still created.
    let njp = report
        .organizations
        .iter()
        .find(|r| r.key == "njp")
        .unwrap();
    assert_eq!(njp.status, Outcome::Failed);
    assert!(njp.detail.as_deref().unwrap().contains("jurisdiction"));
    assert_eq!(
        report
            .organizations
            .iter()
            .filter(|r| r.status == Outcome::Created)
            .count(),
        3
    );
    // Abigail (njp) was still created as a person, but her link was skipped.
    let abigail = report
        .people
        .iter()
        .find(|r| r.key == "abigail-daquiz")
        .unwrap();
    assert_eq!(abigail.status, Outcome::Created);
    assert!(abigail.detail.as_deref().unwrap().contains("link skipped"));
    assert_eq!(
        person_entity_role::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .len(),
        5
    );

    // Both the row failure and the skipped-link note must survive into
    // the text block the MCP/A2A surface renders — otherwise Gemini
    // Enterprise shows the tally with no reason for the failure.
    let problems = report
        .problem_lines()
        .expect("a failed row produces problems");
    assert!(problems.contains("organization `njp` failed"), "{problems}");
    assert!(problems.contains("link skipped"), "{problems}");
}

const SAMPLE: &str = r#"{
  "version": 1,
  "source": "legal-aid-outreach-2026-06",
  "organizations": [
    { "key": "njp", "name": "Northwest Justice Project", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "WA", "phone": "206-464-1519", "url": "https://nwjustice.org" },
    { "key": "mmla", "name": "Mid-Minnesota Legal Aid", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "MN", "phone": "612-332-1441", "url": "https://mylegalaid.org" },
    { "key": "lac", "name": "Legal Aid Chicago", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "IL", "phone": "312-341-1070", "url": "https://legalaidchicago.org" },
    { "key": "lsnyc", "name": "Legal Services NYC", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "NY", "phone": "646-442-3600", "url": "https://lsnyc.org" }
  ],
  "people": [
    { "key": "abigail-daquiz", "name": "Abigail Daquiz", "email": "adaquiz@nwjustice.org", "title": "Executive Director", "phone": "206-464-1519", "organization": "njp" },
    { "key": "milo-mumgaard", "name": "Milo Mumgaard", "email": "mmumgaard@mylegalaid.org", "title": "Executive Director", "phone": "612-332-1441", "organization": "mmla" },
    { "key": "marv-gordon", "name": "Marv Gordon", "email": "mgordon@mylegalaid.org", "title": "IT Director", "phone": "612-332-1441", "organization": "mmla" },
    { "key": "katherine-shank", "name": "Katherine W. Shank", "email": "kshank@legalaidchicago.org", "title": "CEO and Executive Director", "phone": "312-341-1070", "organization": "lac" },
    { "key": "shervon-small", "name": "Shervon M. Small", "email": "ssmall@lsnyc.org", "title": "Executive Director", "phone": "646-442-3600", "organization": "lsnyc" },
    { "key": "dilip-kulkarni", "name": "Dilip Kulkarni", "email": "dkulkarni@lsnyc.org", "title": "Chief Information Officer", "phone": "646-442-3600", "organization": "lsnyc" }
  ]
}"#;
