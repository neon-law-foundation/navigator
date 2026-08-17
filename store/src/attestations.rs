//! `attestations` — the durable local record of one on-chain attorney
//! attestation (the Neon Law Node product), and every query against the
//! table.
//!
//! # This table lives in SurrealDB
//!
//! `attestations` moved with wave five of #1093 (ENG-121), in the
//! satellite-ring slice.
//!
//! Written by the workflow worker inside the `onchain__record_attestation`
//! step's `ctx.run`. The row is the **system of record**: written
//! unconditionally, even when no chain backend is configured (`status`
//! stays `pending`, `chain` is `null`). A real Solana write later fills
//! `pda` / `tx_signature` and flips `status` to `recorded`.
//!
//! # One row per notation, by convention rather than by index
//!
//! The Surreal schema carries no unique index on `notation_id`
//! (`store/src/schema/navigator.surql`'s
//! comment on `attestation_notation` says so) — [`record`] reads the
//! existing row back before writing, the same shape
//! `crate::entities::create`'s firm-anchor guard uses. This is safe here
//! specifically because the only writer is a journaled `ctx.run` step keyed
//! on one notation at a time; it is not a general concurrency guarantee.

use chrono::{DateTime, Utc};
use serde::Serialize;
use surrealdb::types::SurrealValue;
use uuid::Uuid;

use crate::surreal::{record_id, record_uuid, SurrealDb};

/// The table these rows live in.
const TABLE: &str = "attestation";

/// Attestation statuses. The row is [`STATUS_PENDING`] until a real chain
/// tx lands ([`STATUS_RECORDED`]); [`STATUS_FAILED`] records a chain write
/// that errored.
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_RECORDED: &str = "recorded";
pub const STATUS_FAILED: &str = "failed";

/// One on-chain attestation record.
///
/// The application-facing shape: plain Rust types, no engine handles.
/// [`AttestationRow`] is the seam that turns it into (and back out of) what
/// the SDK reads and writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attestation {
    pub id: Uuid,
    pub notation_id: Uuid,
    /// On-chain backend: `solana`, or `"null"` when none is configured.
    pub chain: String,
    /// Lowercase hex SHA-256 of the attested document bytes.
    pub sha256: String,
    /// `pending` / `recorded` / `failed` — see the `STATUS_*` constants.
    pub status: String,
    pub pda: Option<String>,
    pub tx_signature: Option<String>,
    pub firm_wallet: Option<String>,
    pub client_wallet: Option<String>,
    /// RFC 3339 timestamp the on-chain tx confirmed; `None` while pending.
    pub recorded_at: Option<String>,
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The row as the engine reads and writes it.
#[derive(SurrealValue)]
struct AttestationRow {
    id: surrealdb::types::RecordId,
    notation_id: surrealdb::types::RecordId,
    chain: String,
    sha256: String,
    status: String,
    pda: Option<String>,
    tx_signature: Option<String>,
    firm_wallet: Option<String>,
    client_wallet: Option<String>,
    recorded_at: Option<String>,
    inserted_at: surrealdb::types::Datetime,
    updated_at: surrealdb::types::Datetime,
}

impl AttestationRow {
    /// `None` when a record id is not a native UUID key — a row written by
    /// something that bypassed [`crate::surreal::record_id`].
    fn into_attestation(self) -> Option<Attestation> {
        Some(Attestation {
            id: record_uuid(&self.id)?,
            notation_id: record_uuid(&self.notation_id)?,
            chain: self.chain,
            sha256: self.sha256,
            status: self.status,
            pda: self.pda,
            tx_signature: self.tx_signature,
            firm_wallet: self.firm_wallet,
            client_wallet: self.client_wallet,
            recorded_at: self.recorded_at,
            inserted_at: self.inserted_at.into(),
            updated_at: self.updated_at.into(),
        })
    }
}

/// The projection every read shares, so one field list describes the row
/// and a new column cannot reach [`AttestationRow`] from only one query.
const SELECT: &str = "id, notation_id, chain, sha256, status, pda, tx_signature, firm_wallet, \
     client_wallet, recorded_at, inserted_at, updated_at";

/// What to record for one attestation. The `sha256` (of the attested
/// document) and `chain` are always known; `pda` / `tx_signature` /
/// `recorded_at` are present only once a real chain backend records it.
#[derive(Debug, Clone)]
pub struct NewAttestation<'a> {
    pub notation_id: Uuid,
    pub chain: &'a str,
    pub sha256: &'a str,
    pub status: &'a str,
    pub pda: Option<&'a str>,
    pub tx_signature: Option<&'a str>,
    pub firm_wallet: Option<&'a str>,
    pub client_wallet: Option<&'a str>,
    pub recorded_at: Option<&'a str>,
}

/// Errors reading or writing an attestation.
#[derive(Debug, thiserror::Error)]
pub enum AttestationError {
    #[error("database: {0}")]
    Db(#[from] surrealdb::Error),
    /// A write reported success but returned no row, or returned one this
    /// module could not read back.
    #[error("writing an attestation returned no usable row")]
    WriteReturnedNothing,
}

fn one(mut response: surrealdb::IndexedResults) -> Result<Option<Attestation>, surrealdb::Error> {
    let row: Option<AttestationRow> = response.take(0)?;
    Ok(row.and_then(AttestationRow::into_attestation))
}

/// The attestation recorded for a notation, if any.
///
/// # Errors
///
/// Propagates any database error.
pub async fn by_notation(
    db: &SurrealDb,
    notation_id: Uuid,
) -> Result<Option<Attestation>, surrealdb::Error> {
    let response = db
        .query(format!(
            "SELECT {SELECT} FROM {TABLE} WHERE notation_id = $notation LIMIT 1"
        ))
        .bind(("notation", record_id(crate::notations::TABLE, notation_id)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    one(response)
}

/// Upsert one `attestations` row keyed on `notation_id`, returning it.
///
/// Reads the existing row back first (see the module header for why there
/// is no unique index to conflict on): on a match, every chain-outcome
/// column is overwritten — so a `pending` row becomes `recorded` when the
/// chain write later lands, and a journaled replay is idempotent.
/// `inserted_at` is preserved across the upsert.
///
/// # Errors
///
/// [`AttestationError::Db`] if the read or write fails.
pub async fn record(
    db: &SurrealDb,
    new: &NewAttestation<'_>,
) -> Result<Attestation, AttestationError> {
    let existing = by_notation(db, new.notation_id).await?;
    let id = existing.as_ref().map_or_else(Uuid::now_v7, |a| a.id);
    let mut response = db
        .query(format!(
            "UPSERT $id SET \
             notation_id = $notation_id, chain = $chain, sha256 = $sha256, status = $status, \
             pda = $pda, tx_signature = $tx_signature, firm_wallet = $firm_wallet, \
             client_wallet = $client_wallet, recorded_at = $recorded_at, \
             updated_at = time::now() \
             RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, id)))
        .bind((
            "notation_id",
            record_id(crate::notations::TABLE, new.notation_id),
        ))
        .bind(("chain", new.chain.to_string()))
        .bind(("sha256", new.sha256.to_string()))
        .bind(("status", new.status.to_string()))
        .bind(("pda", new.pda.map(str::to_string)))
        .bind(("tx_signature", new.tx_signature.map(str::to_string)))
        .bind(("firm_wallet", new.firm_wallet.map(str::to_string)))
        .bind(("client_wallet", new.client_wallet.map(str::to_string)))
        .bind(("recorded_at", new.recorded_at.map(str::to_string)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<AttestationRow> = response.take(0)?;
    row.and_then(AttestationRow::into_attestation)
        .ok_or(AttestationError::WriteReturnedNothing)
}

#[cfg(test)]
mod tests {
    use super::{by_notation, record, NewAttestation, STATUS_PENDING, STATUS_RECORDED};
    use crate::surreal::test_support::mem;
    use crate::test_support::seed_notation;

    #[tokio::test]
    async fn record_writes_a_pending_row_then_upserts_to_recorded() {
        let surreal = mem().await;
        let notation_id = seed_notation(&surreal).await;

        // First write: no chain configured → a pending row, the system of
        // record, with no tx.
        let pending = record(
            &surreal,
            &NewAttestation {
                notation_id,
                chain: "null",
                sha256: "abc123",
                status: STATUS_PENDING,
                pda: None,
                tx_signature: None,
                firm_wallet: None,
                client_wallet: None,
                recorded_at: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(pending.status, STATUS_PENDING);
        assert_eq!(pending.chain, "null");
        assert!(pending.tx_signature.is_none());

        // Second write for the SAME notation upserts on notation_id — one
        // row, now recorded with a real tx. This is the replay-idempotent
        // path: a journaled re-run does not duplicate.
        let recorded = record(
            &surreal,
            &NewAttestation {
                notation_id,
                chain: "solana",
                sha256: "abc123",
                status: STATUS_RECORDED,
                pda: Some("PDA111"),
                tx_signature: Some("SIG222"),
                firm_wallet: Some("FIRMwallet"),
                client_wallet: Some("CLIENTwallet"),
                recorded_at: Some("2026-06-17T00:00:00Z"),
            },
        )
        .await
        .unwrap();
        assert_eq!(recorded.id, pending.id, "upsert keeps the same row");
        assert_eq!(recorded.status, STATUS_RECORDED);
        assert_eq!(recorded.chain, "solana");
        assert_eq!(recorded.tx_signature.as_deref(), Some("SIG222"));
        assert_eq!(recorded.pda.as_deref(), Some("PDA111"));

        let found = by_notation(&surreal, notation_id).await.unwrap().unwrap();
        assert_eq!(found.id, pending.id);
        assert_eq!(found.status, STATUS_RECORDED);
    }
}
