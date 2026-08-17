//! Database-backed apply tests — the real upsert path against an
//! embedded `SurrealDB`. Single-engine since wave four (ENG-120) moved
//! `entities` and `person_entity_roles`.

use import::{apply, parse, Outcome, Payload};
use store::persons;
use store::surreal::SurrealDb;

/// A fresh engine seeds no reference data; the importer resolves
/// entity-type and jurisdiction by name/code, so the rows it resolves
/// against must exist first.
async fn seed_reference_data(surreal: &SurrealDb) {
    store::entity_types::create(surreal, "501(c)(3) Non-Profit")
        .await
        .expect("seed entity_type");

    for (code, name) in [
        ("WA", "Washington"),
        ("MN", "Minnesota"),
        ("IL", "Illinois"),
        ("NY", "New York"),
    ] {
        store::jurisdictions::create(
            surreal,
            &store::jurisdictions::NewJurisdiction::new(name, code, "state"),
        )
        .await
        .expect("seed jurisdiction");
    }
}

fn sample() -> Payload {
    parse(SAMPLE).expect("parse sample payload")
}

#[tokio::test]
async fn first_import_creates_orgs_people_and_links() {
    let surreal = store::surreal::test_support::mem().await;
    seed_reference_data(&surreal).await;

    let report = apply(&surreal, &sample()).await.expect("apply");

    assert!(!report.has_errors(), "summary: {}", report.summary());
    assert_eq!(report.count(Outcome::Created), 10, "4 orgs + 6 people");
    assert!(report
        .organizations
        .iter()
        .all(|r| r.status == Outcome::Created));
    assert!(report.people.iter().all(|r| r.status == Outcome::Created));

    // Entities, persons, and the client_contact links all landed.
    assert_eq!(store::entities::all(&surreal).await.unwrap().len(), 4);
    assert_eq!(
        persons::list_directory(&surreal, "", "", &[])
            .await
            .unwrap()
            .len(),
        6
    );
    let links = store::entity_roles::all(&surreal).await.unwrap();
    assert_eq!(links.len(), 6);
    assert!(links.iter().all(|l| l.role == "client_contact"));

    // The canonical URL and org phone were stored on the entity.
    let ejp = store::entities::find_by_name(&surreal, "Example Justice Project")
        .await
        .unwrap()
        .expect("ejp exists");
    assert_eq!(ejp.url.as_deref(), Some("https://justice.example"));
    assert_eq!(ejp.phone.as_deref(), Some("206-555-0142"));

    // The person carries title + phone, and defaults to the client tier.
    let ada = persons::find_by_email_ci(&surreal, "acounsel@justice.example")
        .await
        .unwrap()
        .expect("ada exists");
    assert_eq!(ada.title.as_deref(), Some("Executive Director"));
    assert_eq!(ada.role, store::persons::Role::Client);
}

#[tokio::test]
async fn reimport_is_idempotent() {
    let surreal = store::surreal::test_support::mem().await;
    seed_reference_data(&surreal).await;

    apply(&surreal, &sample()).await.expect("first apply");
    let second = apply(&surreal, &sample()).await.expect("second apply");

    assert_eq!(second.count(Outcome::Created), 0);
    assert_eq!(second.count(Outcome::Unchanged), 10);
    // No duplicate rows from the re-run.
    assert_eq!(store::entities::all(&surreal).await.unwrap().len(), 4);
    assert_eq!(
        persons::list_directory(&surreal, "", "", &[])
            .await
            .unwrap()
            .len(),
        6
    );
    assert_eq!(store::entity_roles::all(&surreal).await.unwrap().len(), 6);
}

#[tokio::test]
async fn reimport_with_different_email_casing_resolves_to_the_same_person() {
    // Email is one mailbox regardless of casing, and the database enforces
    // that with `persons_email_lower_key`. A byte-exact lookup would miss
    // the stored row and then fail the insert against that index, so the
    // re-import must resolve to the existing person instead.
    let surreal = store::surreal::test_support::mem().await;
    seed_reference_data(&surreal).await;
    apply(&surreal, &sample()).await.expect("first apply");
    let before = persons::list_directory(&surreal, "", "", &[])
        .await
        .unwrap()
        .len();

    let recased = SAMPLE.replace("acounsel@justice.example", "ACounsel@Justice.example");
    let report = apply(&surreal, &parse(&recased).unwrap())
        .await
        .expect("a re-cased re-import must not collide with the unique index");

    assert_eq!(
        report.count(Outcome::Created),
        0,
        "a case variant is the same mailbox, not a new person"
    );
    assert_eq!(
        persons::list_directory(&surreal, "", "", &[])
            .await
            .unwrap()
            .len(),
        before
    );
}

#[tokio::test]
async fn reimport_with_changed_title_updates_only_that_person() {
    let surreal = store::surreal::test_support::mem().await;
    seed_reference_data(&surreal).await;
    apply(&surreal, &sample()).await.expect("first apply");

    let changed = SAMPLE.replace(
        "\"title\": \"IT Director\"",
        "\"title\": \"Chief Technology Officer\"",
    );
    let report = apply(&surreal, &parse(&changed).unwrap())
        .await
        .expect("apply");

    assert_eq!(report.count(Outcome::Updated), 1);
    assert_eq!(report.count(Outcome::Unchanged), 9);
    let marv = persons::find_by_email_ci(&surreal, "mgordon@mylegalaid.org")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(marv.title.as_deref(), Some("Chief Technology Officer"));
}

#[tokio::test]
async fn unknown_jurisdiction_fails_only_its_row() {
    let surreal = store::surreal::test_support::mem().await;
    // Deliberately omit Washington so ejp can't resolve its jurisdiction.
    store::entity_types::create(&surreal, "501(c)(3) Non-Profit")
        .await
        .unwrap();
    for (code, name) in [("MN", "Minnesota"), ("IL", "Illinois"), ("NY", "New York")] {
        store::jurisdictions::create(
            &surreal,
            &store::jurisdictions::NewJurisdiction::new(name, code, "state"),
        )
        .await
        .unwrap();
    }

    let report = apply(&surreal, &sample()).await.expect("apply");

    // ejp org failed; the three resolvable orgs still created.
    let ejp = report
        .organizations
        .iter()
        .find(|r| r.key == "ejp")
        .unwrap();
    assert_eq!(ejp.status, Outcome::Failed);
    assert!(ejp.detail.as_deref().unwrap().contains("jurisdiction"));
    assert_eq!(
        report
            .organizations
            .iter()
            .filter(|r| r.status == Outcome::Created)
            .count(),
        3
    );
    // Ada (ejp) was still created as a person, but her link was skipped.
    let ada = report
        .people
        .iter()
        .find(|r| r.key == "ada-counsel")
        .unwrap();
    assert_eq!(ada.status, Outcome::Created);
    assert!(ada.detail.as_deref().unwrap().contains("link skipped"));
    assert_eq!(store::entity_roles::all(&surreal).await.unwrap().len(), 5);

    // Both the row failure and the skipped-link note must survive into
    // the text block the MCP/A2A surface renders — otherwise Gemini
    // Enterprise shows the tally with no reason for the failure.
    let problems = report
        .problem_lines()
        .expect("a failed row produces problems");
    assert!(problems.contains("organization `ejp` failed"), "{problems}");
    assert!(problems.contains("link skipped"), "{problems}");
}

const SAMPLE: &str = r#"{
  "version": 1,
  "source": "partner-outreach-2026-06",
  "organizations": [
    { "key": "ejp", "name": "Example Justice Project", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "WA", "phone": "206-555-0142", "url": "https://justice.example" },
    { "key": "mmla", "name": "Mid-Minnesota Legal Aid", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "MN", "phone": "612-332-1441", "url": "https://mylegalaid.org" },
    { "key": "lac", "name": "Legal Aid Chicago", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "IL", "phone": "312-341-1070", "url": "https://legalaidchicago.org" },
    { "key": "lsnyc", "name": "Legal Services NYC", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "NY", "phone": "646-442-3600", "url": "https://lsnyc.org" }
  ],
  "people": [
    { "key": "ada-counsel", "name": "Ada Counsel", "email": "acounsel@justice.example", "title": "Executive Director", "phone": "206-555-0142", "organization": "ejp" },
    { "key": "milo-mumgaard", "name": "Milo Mumgaard", "email": "mmumgaard@mylegalaid.org", "title": "Executive Director", "phone": "612-332-1441", "organization": "mmla" },
    { "key": "marv-gordon", "name": "Marv Gordon", "email": "mgordon@mylegalaid.org", "title": "IT Director", "phone": "612-332-1441", "organization": "mmla" },
    { "key": "katherine-shank", "name": "Katherine W. Shank", "email": "kshank@legalaidchicago.org", "title": "CEO and Executive Director", "phone": "312-341-1070", "organization": "lac" },
    { "key": "shervon-small", "name": "Shervon M. Small", "email": "ssmall@lsnyc.org", "title": "Executive Director", "phone": "646-442-3600", "organization": "lsnyc" },
    { "key": "dilip-kulkarni", "name": "Dilip Kulkarni", "email": "dkulkarni@lsnyc.org", "title": "Chief Information Officer", "phone": "646-442-3600", "organization": "lsnyc" }
  ]
}"#;
