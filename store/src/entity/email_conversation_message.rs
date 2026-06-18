//! `email_conversation_messages` — the append-only transcript of a
//! support thread. One row per hop; never updated in place. The parent
//! [`super::email_conversation`]'s `status` projects from the latest row.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Inbound from the external party (client / prospective client).
pub const DIRECTION_FROM_EXTERNAL: &str = "from_external";
/// The notification we send the attorney's cockpit inbox.
pub const DIRECTION_TO_STAFF: &str = "to_staff";
/// The attorney's reply back into the thread.
pub const DIRECTION_FROM_STAFF: &str = "from_staff";
/// The relay we send the external party as `support@`.
pub const DIRECTION_TO_EXTERNAL: &str = "to_external";
/// A system note (e.g. conflict-check result); not an email hop.
pub const DIRECTION_SYSTEM: &str = "system";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "email_conversation_messages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::email_conversation`] this hop belongs to.
    pub conversation_id: Uuid,
    /// One of the `DIRECTION_*` constants.
    pub direction: String,
    pub from_addr: String,
    pub to_addr: String,
    pub subject: String,
    /// Cleaned body — quoted history + signature stripped on staff
    /// replies, so a relayed message carries only the new prose.
    pub body_text: String,
    /// Object-storage key of the raw `.eml` for inbound hops; `None` for
    /// messages we generated.
    pub raw_storage_key: Option<String>,
    /// SendGrid `X-Message-Id` (outbound) or inbound `Message-ID` — the
    /// join key to the delivery stream and the dedup key on retries.
    pub provider_message_id: Option<String>,
    /// RFC 5322 `In-Reply-To` of this hop, when present.
    pub in_reply_to: Option<String>,
    /// Parsed staff-reply directives (`@approve`/`@deny`/…) as JSON;
    /// `None` when the reply carried no command.
    pub command_payload: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::email_conversation::Entity",
        from = "Column::ConversationId",
        to = "super::email_conversation::Column::Id"
    )]
    Conversation,
}

impl Related<super::email_conversation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Conversation.def()
    }
}

crate::uuid_active_model_behavior!();
