//! Upsert helper for the `attestations` table — the durable local record
//! of one on-chain attorney attestation (the Neon Law Node product).
//!
//! Called by the workflow worker inside the `onchain__record_attestation`
//! step's `ctx.run`. The row is the **system of record**: written
//! unconditionally, even when no chain backend is configured (`status`
//! stays `pending`, `chain` is `null`). A real Solana write later fills
//! `pda` / `tx_signature` and flips `status` to `recorded`.
//!
//! One row per notation — `notation_id` is unique, so [`record`] upserts
//! on it. A Restate replay of the journaled step therefore updates the
//! same row rather than writing a duplicate (the replay-idempotency the
//! `ctx.run` boundary promises). Kept here beside the other orchestration
//! helpers so `web` / the worker reach it without importing the entity.

use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::entity::attestation;
use crate::Db;

/// Attestation statuses. The row is `Pending` until a real chain tx lands
/// (`Recorded`); `Failed` records a chain write that errored.
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_RECORDED: &str = "recorded";
pub const STATUS_FAILED: &str = "failed";

/// What to record for one attestation. The `sha256` (of the attested
/// document) and `chain` are always known; `pda` / `tx_signature` /
/// `recorded_at` are present only once a real chain backend records it.
#[derive(Debug, Clone)]
pub struct NewAttestation<'a> {
    pub notation_id: Uuid,
    /// On-chain backend: `solana`, or `null` when none is configured.
    pub chain: &'a str,
    /// Lowercase hex SHA-256 of the attested document bytes.
    pub sha256: &'a str,
    /// `pending` / `recorded` / `failed` (see the `STATUS_*` constants).
    pub status: &'a str,
    pub pda: Option<&'a str>,
    pub tx_signature: Option<&'a str>,
    pub firm_wallet: Option<&'a str>,
    pub client_wallet: Option<&'a str>,
    /// RFC 3339 timestamp the chain tx confirmed; `None` while pending.
    pub recorded_at: Option<&'a str>,
}

/// Upsert one `attestations` row keyed on `notation_id`, returning it.
///
/// On a conflict (the notation already has an attestation) every
/// chain-outcome column is overwritten — so a `pending` row becomes
/// `recorded` when the chain write later lands, and a journaled replay is
/// idempotent. `inserted_at` is preserved across the upsert.
///
/// # Errors
///
/// Propagates any database error.
pub async fn record(
    db: &Db,
    new: &NewAttestation<'_>,
) -> Result<attestation::Model, sea_orm::DbErr> {
    // The `on_conflict` upsert goes through `Entity::insert(..).exec*`,
    // which bypasses the `ActiveModelBehavior` that stamps `id` and the
    // timestamps on the plain `.insert()` path — so set them here. `id` /
    // `inserted_at` are not in `update_columns`, so on a conflict the
    // existing row keeps its original identity and insert time.
    let now = chrono::Utc::now().to_rfc3339();
    let row = attestation::ActiveModel {
        id: Set(Uuid::now_v7()),
        notation_id: Set(new.notation_id),
        chain: Set(new.chain.to_string()),
        sha256: Set(new.sha256.to_string()),
        status: Set(new.status.to_string()),
        pda: Set(new.pda.map(str::to_string)),
        tx_signature: Set(new.tx_signature.map(str::to_string)),
        firm_wallet: Set(new.firm_wallet.map(str::to_string)),
        client_wallet: Set(new.client_wallet.map(str::to_string)),
        recorded_at: Set(new.recorded_at.map(str::to_string)),
        inserted_at: Set(now.clone()),
        updated_at: Set(now),
    };
    attestation::Entity::insert(row)
        .on_conflict(
            OnConflict::column(attestation::Column::NotationId)
                .update_columns([
                    attestation::Column::Chain,
                    attestation::Column::Sha256,
                    attestation::Column::Status,
                    attestation::Column::Pda,
                    attestation::Column::TxSignature,
                    attestation::Column::FirmWallet,
                    attestation::Column::ClientWallet,
                    attestation::Column::RecordedAt,
                    attestation::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec_with_returning(db)
        .await
}

/// The attestation recorded for a notation, if any.
///
/// # Errors
///
/// Propagates any database error.
pub async fn by_notation(
    db: &Db,
    notation_id: Uuid,
) -> Result<Option<attestation::Model>, sea_orm::DbErr> {
    attestation::Entity::find()
        .filter(attestation::Column::NotationId.eq(notation_id))
        .one(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::{by_notation, record, NewAttestation, STATUS_PENDING, STATUS_RECORDED};
    use crate::entity::{notation, person, project, template};
    use sea_orm::{ActiveModelTrait, ActiveValue};

    async fn seed_notation(db: &crate::Db) -> uuid::Uuid {
        let tmpl = template::ActiveModel {
            code: ActiveValue::Set("onboarding__retainer_node".into()),
            title: ActiveValue::Set("Node Engagement".into()),
            respondent_type: ActiveValue::Set("person_and_entity".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let person = person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let proj = project::ActiveModel {
            name: ActiveValue::Set("matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        notation::ActiveModel {
            template_id: ActiveValue::Set(tmpl.id),
            person_id: ActiveValue::Set(person.id),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(proj.id),
            state: ActiveValue::Set("BEGIN".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn record_writes_a_pending_row_then_upserts_to_recorded() {
        let db = crate::test_support::pg().await;
        let notation_id = seed_notation(&db).await;

        // First write: no chain configured → a pending row, the system
        // of record, with no tx.
        let pending = record(
            &db,
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
        assert!(!pending.inserted_at.is_empty());

        // Second write for the SAME notation upserts on the unique
        // notation_id — one row, now recorded with a real tx. This is the
        // replay-idempotent path: a journaled re-run does not duplicate.
        let recorded = record(
            &db,
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

        let found = by_notation(&db, notation_id).await.unwrap().unwrap();
        assert_eq!(found.id, pending.id);
        assert_eq!(found.status, STATUS_RECORDED);
    }
}
