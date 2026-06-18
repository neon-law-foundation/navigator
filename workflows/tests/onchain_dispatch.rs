//! Integration test for the `onchain__record_attestation` step (the Neon
//! Law Node on-chain attestation).
//!
//! Drives the dispatch through the shared `workflows::dispatch_step`
//! registry — the same arm the `workflows-service` worker runs inside
//! `ctx.run` — and asserts the durable `attestations` row lands. Needs
//! Postgres (testcontainers) because the side effect writes a real row.
//!
//! Two paths, both proving the council's "no false success" rule:
//! - the `NullAttestor` (no chain configured) writes a `pending` row with
//!   **no** transaction — the honest default the Node page promises;
//! - a recording attestor writes a `recorded` row carrying the real tx
//!   signature + PDA.

use std::sync::Arc;

use async_trait::async_trait;
use workflows::{
    dispatch_step, AttestError, AttestationRequest, Attestor, NullAttestor, OnChainPayload,
    RecordedTx, StateName, StepDeps,
};

async fn fs_storage(suite: &str) -> Arc<dyn cloud::StorageService> {
    Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("navigator-onchain-{suite}")))
            .await
            .expect("temp FsStorage"),
    )
}

/// A stand-in for the not-yet-shipped `SolanaAttestor`: records a
/// deterministic transaction so the `recorded` path is exercised without
/// a real chain.
struct RecordingAttestor;

#[async_trait]
impl Attestor for RecordingAttestor {
    fn chain(&self) -> &'static str {
        "solana"
    }

    async fn record(&self, req: &AttestationRequest) -> Result<Option<RecordedTx>, AttestError> {
        Ok(Some(RecordedTx {
            signature: format!("SIG-{}", req.sha256),
            pda: "PDA-test".into(),
            recorded_at: "2026-06-17T00:00:00Z".into(),
        }))
    }
}

const PDF_BYTES: &[u8] = b"%PDF-1.7 signed Node attestation";

fn payload(key: &str) -> String {
    serde_json::to_string(&OnChainPayload {
        storage_key: key.to_string(),
        firm_wallet: Some("FIRMwallet".into()),
        client_wallet: Some("CLIENTwallet".into()),
    })
    .unwrap()
}

#[tokio::test]
async fn null_attestor_writes_a_pending_row_with_no_transaction() {
    let db = store::test_support::pg().await;
    let notation_id = store::test_support::seed_notation(&db).await;
    let storage = fs_storage("pending").await;
    let key = format!("notations/{notation_id}/attestation.pdf");
    storage
        .put(&key, PDF_BYTES, "application/pdf")
        .await
        .unwrap();

    let deps = StepDeps::new(
        Arc::new(workflows::CapturingEmail::new()),
        storage,
        Some(db.clone()),
    )
    .with_attestor(Arc::new(NullAttestor));

    dispatch_step(
        &deps,
        notation_id,
        &StateName::from("onchain__record_attestation"),
        Some(&payload(&key)),
    )
    .await
    .expect("onchain dispatch writes the local row");

    let row = store::attestations::by_notation(&db, notation_id)
        .await
        .unwrap()
        .expect("an attestation row landed");
    assert_eq!(row.status, store::attestations::STATUS_PENDING);
    assert_eq!(row.chain, "null");
    assert!(row.tx_signature.is_none(), "no false on-chain tx");
    assert!(row.pda.is_none());
    assert_eq!(row.firm_wallet.as_deref(), Some("FIRMwallet"));
    assert_eq!(row.client_wallet.as_deref(), Some("CLIENTwallet"));
    // The hash on the row is the SHA-256 of the stored document bytes.
    assert_eq!(row.sha256, workflows::attest::sha256_hex(PDF_BYTES));
}

#[tokio::test]
async fn recording_attestor_writes_a_recorded_row_with_the_transaction() {
    let db = store::test_support::pg().await;
    let notation_id = store::test_support::seed_notation(&db).await;
    let storage = fs_storage("recorded").await;
    let key = format!("notations/{notation_id}/attestation.pdf");
    storage
        .put(&key, PDF_BYTES, "application/pdf")
        .await
        .unwrap();

    let deps = StepDeps::new(
        Arc::new(workflows::CapturingEmail::new()),
        storage,
        Some(db.clone()),
    )
    .with_attestor(Arc::new(RecordingAttestor));

    dispatch_step(
        &deps,
        notation_id,
        &StateName::from("onchain__record_attestation"),
        Some(&payload(&key)),
    )
    .await
    .expect("onchain dispatch records the attestation");

    let row = store::attestations::by_notation(&db, notation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.status, store::attestations::STATUS_RECORDED);
    assert_eq!(row.chain, "solana");
    let sha = workflows::attest::sha256_hex(PDF_BYTES);
    assert_eq!(
        row.tx_signature.as_deref(),
        Some(format!("SIG-{sha}").as_str())
    );
    assert_eq!(row.pda.as_deref(), Some("PDA-test"));
    assert_eq!(row.recorded_at.as_deref(), Some("2026-06-17T00:00:00Z"));
}

#[tokio::test]
async fn onchain_step_without_an_attestor_errors_clearly() {
    let db = store::test_support::pg().await;
    let notation_id = store::test_support::seed_notation(&db).await;
    let storage = fs_storage("noattestor").await;
    let key = format!("notations/{notation_id}/attestation.pdf");
    storage
        .put(&key, PDF_BYTES, "application/pdf")
        .await
        .unwrap();

    // No `.with_attestor(..)` — the onchain arm must fail loudly, not
    // silently skip recording.
    let deps = StepDeps::new(
        Arc::new(workflows::CapturingEmail::new()),
        storage,
        Some(db.clone()),
    );

    let err = dispatch_step(
        &deps,
        notation_id,
        &StateName::from("onchain__record_attestation"),
        Some(&payload(&key)),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            err,
            workflows::StepDispatchError::MissingAttestor("onchain_record")
        ),
        "expected MissingAttestor, got {err:?}"
    );

    assert!(
        store::attestations::by_notation(&db, notation_id)
            .await
            .unwrap()
            .is_none(),
        "no row written when the step can't record"
    );
}
