//! `signatures` table — one signature request/execution on a Notation's
//! document, correlated back from the provider by `(provider,
//! provider_id)`.
//!
//! A Notation's document is sent to an e-signature provider (DocuSign);
//! the provider issues an opaque request id (an envelope id). This row
//! records that request so the inbound completion webhook
//! (`web::esignature_webhook`) can resolve a callback back to its
//! Notation by matching `(provider, provider_id)` — the pair is unique.
//! `signer_person_id`/`field` name who signs where (null until known);
//! `signed_at` is stamped when the provider reports completion.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The e-signature provider that executed a signature. Stored as `TEXT`.
/// A closed set keeps call sites and the webhook from inventing provider
/// strings; today the firm signs exclusively through DocuSign.
#[derive(Clone, Copy, Debug, Eq, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum SignatureProvider {
    /// DocuSign — the production e-signature seam (`web::signature`).
    #[sea_orm(string_value = "docusign")]
    DocuSign,
}

impl SignatureProvider {
    /// String form stored in the `provider` column and matched by the
    /// completion webhook.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DocuSign => "docusign",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "signatures")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The Notation whose document this signature executes on.
    pub notation_id: Uuid,
    /// The Person signing, once resolved. Null while only the envelope
    /// (provider request) is known.
    pub signer_person_id: Option<Uuid>,
    /// The signature field/tab this row tracks (e.g. `client.signature`).
    /// Null when the request tracks the envelope as a whole.
    pub field: Option<String>,
    /// The e-signature provider (`docusign`).
    pub provider: SignatureProvider,
    /// Opaque provider-issued request id (DocuSign `envelopeId`). Unique
    /// with `provider`; the webhook's only correlation key.
    pub provider_id: String,
    /// RFC 3339 timestamp the provider reported the signature complete.
    /// Null until the completion webhook stamps it.
    pub signed_at: Option<String>,
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
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::SignerPersonId",
        to = "super::person::Column::Id"
    )]
    Signer,
}

impl Related<super::notation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Notation.def()
    }
}

crate::uuid_active_model_behavior!();
