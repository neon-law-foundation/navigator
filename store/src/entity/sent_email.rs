//! `sent_emails` table — one row per outbound message that went
//! through `EmailService`. Append-only; never updated in place.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "sent_emails")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub recipient: String,
    pub subject: String,
    /// `support@neonlaw.com` for the default outbound path; carried
    /// so the audit trail reflects whatever the trait actually used.
    pub sender: String,
    /// Slug of the template that rendered the body (`welcome`, etc.).
    /// `None` for ad-hoc messages.
    pub template_slug: Option<String>,
    pub body: String,
    /// `sent` on success, `failed:<reason>` on failure.
    pub outcome: String,
    /// SendGrid's `X-Message-Id` response header, captured on a 202.
    /// The join key to the delivery-side Event Webhook stream
    /// (Iceberg/Parquet on GCS, queried from BigQuery). `None` for
    /// failed sends and for the `CapturingEmail` dev backend, neither
    /// of which yields an upstream id.
    pub sg_message_id: Option<String>,
    /// RFC 3339 / ISO 8601.
    pub sent_at: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

crate::uuid_active_model_behavior!();
