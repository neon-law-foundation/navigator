//! `notarizations` table — one notarization request/execution on a
//! Notation's document, correlated back from the provider by `(provider,
//! provider_id)`.
//!
//! The notary counterpart to [`super::signature`]: a Notation's document
//! is sent for remote online notarization; the provider issues an opaque
//! request id, and `(provider, provider_id)` (unique) resolves a callback
//! back to its Notation. `notary_person_id`/`document_id` name who
//! notarized what (null until known); `notarized_at` is stamped on
//! completion.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "notarizations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The Notation whose document this notarization executes on.
    pub notation_id: Uuid,
    /// The notary Person, once resolved. Null while only the provider
    /// request is known.
    pub notary_person_id: Option<Uuid>,
    /// The Document notarized, once resolved.
    pub document_id: Option<Uuid>,
    /// The notarization provider (`docusign`).
    pub provider: super::signature::SignatureProvider,
    /// Opaque provider-issued request id. Unique with `provider`; the
    /// callback's correlation key.
    pub provider_id: String,
    /// RFC 3339 timestamp the provider reported the notarization complete.
    /// Null until the completion callback stamps it.
    pub notarized_at: Option<String>,
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
        from = "Column::NotaryPersonId",
        to = "super::person::Column::Id"
    )]
    Notary,
    #[sea_orm(
        belongs_to = "super::document::Entity",
        from = "Column::DocumentId",
        to = "super::document::Column::Id"
    )]
    Document,
}

impl Related<super::notation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Notation.def()
    }
}

crate::uuid_active_model_behavior!();
