//! `email_conversations` — one threaded support exchange behind
//! `support@neonlaw.com` (the "headless Front").
//!
//! Keyed by an opaque `token` that rides in the `Reply-To`
//! (`c<token>@parse.neonlaw.com`) of every hop, so staff and client
//! replies thread back here without leaking an internal address. The
//! `status` projects from the latest message; `notation_id`, when set,
//! links the thread to a running matter so an attorney's `@approve` reply
//! can fire a Restate workflow signal. See [`super::email_conversation_message`]
//! for the append-only transcript.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Freshly opened; not yet acted on.
pub const STATUS_OPEN: &str = "open";
/// Staff have been notified; waiting on the attorney to reply.
pub const STATUS_AWAITING_STAFF: &str = "awaiting_staff";
/// A reply was relayed out; waiting on the external party.
pub const STATUS_AWAITING_CLIENT: &str = "awaiting_client";
/// Closed — no further relay.
pub const STATUS_CLOSED: &str = "closed";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "email_conversations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Opaque, unguessable thread token; the VERP key in `Reply-To`.
    pub token: String,
    /// The external party's address (client or prospective client).
    pub external_email: String,
    /// The external party's display name, if the inbound carried one.
    pub external_name: Option<String>,
    /// FK → [`super::person`] once matched; `None` until conflict-checked.
    pub person_id: Option<Uuid>,
    /// Subject of the originating message.
    pub subject: String,
    /// `open`, `awaiting_staff`, `awaiting_client`, or `closed` — see the
    /// `STATUS_*` constants.
    pub status: String,
    /// FK → [`super::notation`] — the matter this thread drives, if any.
    pub notation_id: Option<Uuid>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
    #[sea_orm(
        belongs_to = "super::notation::Entity",
        from = "Column::NotationId",
        to = "super::notation::Column::Id"
    )]
    Notation,
    #[sea_orm(has_many = "super::email_conversation_message::Entity")]
    Message,
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

impl Related<super::notation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Notation.def()
    }
}

impl Related<super::email_conversation_message::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Message.def()
    }
}

crate::uuid_active_model_behavior!();
