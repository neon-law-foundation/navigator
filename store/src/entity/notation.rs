//! `notations` table — one filled-in template, owned by a person
//! (and optionally an entity) inside exactly one Project, with a
//! workflow `state`. See [`docs/notation.md#notation`].
//!
//! [`docs/notation.md#notation`]: ../../../docs/notation.md#notation

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "notations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub template_id: Uuid,
    pub person_id: Uuid,
    pub entity_id: Option<Uuid>,
    /// FK → [`super::project`]. Every Notation belongs to exactly
    /// one Project; the glossary's load-bearing rule for the
    /// matter-audit-trail story.
    pub project_id: Uuid,
    /// `draft`, `staff_review`, `signed`, …
    pub state: String,
    /// Opaque e-signature provider request id (DocuSign `envelopeId`),
    /// set when the retainer reaches `sent_for_signature__pending`. The
    /// inbound completion webhook (`web::esignature_webhook`) resolves a
    /// provider callback back to this notation by matching on this
    /// column. `None` for notations not yet sent for signature. See
    /// `m20260621_add_signature_request_id_to_notations`.
    pub signature_request_id: Option<String>,
    /// How the client receives this notation when it is sent for
    /// signature: [`DELIVERY_EMBEDDED`] (captive — signs inside Neon Law Navigator,
    /// not emailed) or [`DELIVERY_EMAILED`] (DocuSign emails a signing
    /// link). Read once when the signature manifest is built; selects how
    /// the single send path addresses the client recipient. Defaults to
    /// `embedded`. See `m20260708_add_delivery_to_notations`.
    pub delivery: String,
    /// Admin-discretion discount: whole-number percent off list
    /// (`0..=100`). `None` when undiscounted. At most one of
    /// `discount_pct` / `discount_amount_cents` is set. The list price
    /// itself stays in the `products` catalog — this records only how far
    /// *below* list this engagement was billed. See
    /// `m20260710_add_discount_to_notations`.
    pub discount_pct: Option<i32>,
    /// Admin-discretion discount: a flat amount off list, in minor units
    /// (cents). `None` when undiscounted.
    pub discount_amount_cents: Option<i64>,
    /// Recorded basis for the discount (hardship / pro bono / PPP /
    /// mission). `None` when undiscounted.
    pub discount_reason: Option<String>,
    /// Approving staff/admin email. `None` when undiscounted.
    pub discount_approved_by: Option<String>,
    /// RFC 3339 timestamp of the discount approval. `None` when
    /// undiscounted.
    pub discount_approved_at: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

/// Captive client recipient: signs embedded inside Neon Law Navigator (no email).
/// The historical retainer-walk default.
pub const DELIVERY_EMBEDDED: &str = "embedded";

/// Non-captive client recipient: DocuSign emails a signing link the
/// client opens from their own inbox. The matter-open default — a
/// brand-new client opened from the admin page is not in the room.
pub const DELIVERY_EMAILED: &str = "emailed";

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::template::Entity",
        from = "Column::TemplateId",
        to = "super::template::Column::Id"
    )]
    Template,
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
    #[sea_orm(
        belongs_to = "super::entity::Entity",
        from = "Column::EntityId",
        to = "super::entity::Column::Id"
    )]
    Entity,
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id"
    )]
    Project,
}

impl Related<super::template::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Template.def()
    }
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

crate::uuid_active_model_behavior!();
