//! `attestations` table — one durable local record per on-chain
//! attorney attestation (the Neon Law Node product).
//!
//! Written by the workflow worker inside the
//! `onchain__record_attestation` step's `ctx.run`, so the row is the
//! replay-idempotent system of record for the attestation. The Solana
//! transaction (`pda` + `tx_signature`) is a *mirror* recorded only when
//! a real chain backend is configured; the row exists regardless, with
//! `status` distinguishing `pending` from `recorded`. See
//! [`crate::attestations`] for the upsert helper.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "attestations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::notation`] — the matter that was attested.
    pub notation_id: Uuid,
    /// On-chain backend: `solana`, or `null` when none is configured.
    pub chain: String,
    /// Lowercase hex SHA-256 of the attested document bytes.
    pub sha256: String,
    /// `pending` (no on-chain tx yet), `recorded` (a real tx landed), or
    /// `failed` (the chain write errored).
    pub status: String,
    /// Solana Program Derived Address; `None` until a real tx lands.
    pub pda: Option<String>,
    /// Solana transaction signature; `None` until a real tx lands.
    pub tx_signature: Option<String>,
    /// Firm wallet public key bound in the attestation.
    pub firm_wallet: Option<String>,
    /// Client wallet public key bound in the attestation.
    pub client_wallet: Option<String>,
    /// RFC 3339 timestamp the on-chain tx confirmed; `None` while pending.
    pub recorded_at: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::notation::Entity",
        from = "Column::NotationId",
        to = "super::notation::Column::Id"
    )]
    Notation,
}

impl Related<super::notation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Notation.def()
    }
}

crate::uuid_active_model_behavior!();
