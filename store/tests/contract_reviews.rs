//! Schema + helper guards for the inbound contract-review tables
//! (`m20260721_create_contract_review_tables`).
//!
//! Three invariants:
//! 1. A playbook round-trips, and its JSONB `positions` deserialize back to
//!    the typed `Vec<Position>`.
//! 2. `(entity_id, name)` is unique per Entity.
//! 3. A contract review walks its lifecycle (`pending` → `analyzed` →
//!    `approved`), the JSONB `findings` round-trip to `Vec<Finding>`, and
//!    `accepted` defaults to `false` until the attorney acts.

use store::contract_reviews::{self, Finding, NewContractReview};
use store::entity::contract_review::{STATUS_ANALYZED, STATUS_APPROVED, STATUS_PENDING};
use store::playbooks::{self, NewPlaybook, Position, SEVERITY_HIGH};
use store::test_support::{pg, seed_entity, seed_notation};

fn liability_position() -> Position {
    Position {
        topic: "Limitation of liability".into(),
        preferred: "Mutual cap at 12 months' fees".into(),
        fallback: "Cap at 24 months' fees".into(),
        walkaway: "Uncapped liability".into(),
        severity: SEVERITY_HIGH.into(),
    }
}

#[tokio::test]
async fn playbook_round_trips_with_typed_positions() {
    let db = pg().await;
    let entity_id = seed_entity(&db).await;

    let positions = vec![liability_position()];
    let id = playbooks::create(
        &db,
        &NewPlaybook {
            entity_id,
            name: "SaaS vendor MSA",
            positions: &positions,
        },
    )
    .await
    .expect("create playbook");

    let row = playbooks::by_id(&db, id)
        .await
        .expect("load playbook")
        .expect("playbook exists");
    assert!(row.active);
    assert_eq!(row.entity_id, entity_id);

    let stored = playbooks::positions_of(&row).expect("positions deserialize");
    assert_eq!(stored, positions);
    assert_eq!(stored[0].topic, "Limitation of liability");

    // for_entity lists it.
    let listed = playbooks::for_entity(&db, entity_id).await.expect("list");
    assert_eq!(listed.len(), 1);
}

#[tokio::test]
async fn playbook_name_is_unique_per_entity() {
    let db = pg().await;
    let entity_id = seed_entity(&db).await;
    let positions = vec![liability_position()];
    let new = NewPlaybook {
        entity_id,
        name: "SaaS vendor MSA",
        positions: &positions,
    };

    playbooks::create(&db, &new).await.expect("first insert");
    let err = playbooks::create(&db, &new)
        .await
        .expect_err("duplicate (entity_id, name) must violate the unique index");
    assert!(
        store::is_unique_violation(&err),
        "expected a unique violation, got {err:?}"
    );
}

#[tokio::test]
async fn contract_review_walks_pending_to_approved() {
    let db = pg().await;
    let notation_id = seed_notation(&db).await;
    let entity_id = seed_entity(&db).await;
    let positions = vec![liability_position()];
    let playbook_id = playbooks::create(
        &db,
        &NewPlaybook {
            entity_id,
            name: "SaaS vendor MSA",
            positions: &positions,
        },
    )
    .await
    .expect("create playbook");

    // Opens at `pending` with no findings, no document yet.
    let review_id = contract_reviews::create(
        &db,
        &NewContractReview {
            notation_id,
            playbook_id,
            document_id: None,
        },
    )
    .await
    .expect("create review");

    let row = contract_reviews::by_id(&db, review_id)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(row.status, STATUS_PENDING);
    assert_eq!(row.document_id, None);
    assert_eq!(row.risk_summary, None);
    assert!(contract_reviews::findings_of(&row)
        .expect("empty findings")
        .is_empty());

    // Analysis writes findings + risk summary, advances to `analyzed`.
    let findings = vec![Finding {
        clause_ref: "§7.2 Liability".into(),
        deviation: "Liability is uncapped; playbook walk-away line.".into(),
        severity: SEVERITY_HIGH.into(),
        suggested_redline: Some("Add a mutual cap at 12 months' fees.".into()),
        attorney_note: None,
        accepted: false,
    }];
    contract_reviews::record_analysis(&db, review_id, "One high-severity deviation.", &findings)
        .await
        .expect("record analysis");

    let analyzed = contract_reviews::by_id(&db, review_id)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(analyzed.status, STATUS_ANALYZED);
    assert_eq!(
        analyzed.risk_summary.as_deref(),
        Some("One high-severity deviation.")
    );
    let stored = contract_reviews::findings_of(&analyzed).expect("findings deserialize");
    assert_eq!(stored, findings);
    // Nothing is accepted until the attorney acts.
    assert!(!stored[0].accepted);

    // Attorney edits the finding (accepts + adds a note), then approves.
    let edited = vec![Finding {
        attorney_note: Some("Agreed — push the redline.".into()),
        accepted: true,
        ..findings[0].clone()
    }];
    contract_reviews::update_findings(&db, review_id, &edited)
        .await
        .expect("update findings");
    contract_reviews::set_status(&db, review_id, STATUS_APPROVED)
        .await
        .expect("approve");

    let approved = contract_reviews::latest_for_notation(&db, notation_id)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(approved.status, STATUS_APPROVED);
    let final_findings = contract_reviews::findings_of(&approved).expect("findings");
    assert!(final_findings[0].accepted);
    assert_eq!(
        final_findings[0].attorney_note.as_deref(),
        Some("Agreed — push the redline.")
    );
}

#[tokio::test]
async fn update_risk_summary_edits_summary_only() {
    let db = pg().await;
    let notation_id = seed_notation(&db).await;
    let entity_id = seed_entity(&db).await;
    let positions = vec![liability_position()];
    let playbook_id = playbooks::create(
        &db,
        &NewPlaybook {
            entity_id,
            name: "MSA",
            positions: &positions,
        },
    )
    .await
    .unwrap();
    let review_id = contract_reviews::create(
        &db,
        &NewContractReview {
            notation_id,
            playbook_id,
            document_id: None,
        },
    )
    .await
    .unwrap();
    let findings = vec![Finding {
        clause_ref: "§7.2".into(),
        deviation: "uncapped".into(),
        severity: SEVERITY_HIGH.into(),
        suggested_redline: None,
        attorney_note: None,
        accepted: true,
    }];
    contract_reviews::record_analysis(&db, review_id, "machine summary", &findings)
        .await
        .unwrap();

    contract_reviews::update_risk_summary(&db, review_id, "attorney-revised summary")
        .await
        .unwrap();

    let row = contract_reviews::by_id(&db, review_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.risk_summary.as_deref(),
        Some("attorney-revised summary")
    );
    // Findings and status are untouched.
    assert_eq!(row.status, STATUS_ANALYZED);
    let stored = contract_reviews::findings_of(&row).unwrap();
    assert!(stored[0].accepted);
}
