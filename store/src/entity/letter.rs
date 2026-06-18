//! `letters` table — one physical piece of mail (incoming or outgoing).

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "letters")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub mailroom_id: Uuid,
    /// `incoming` or `outgoing`.
    pub direction: String,
    pub sender: String,
    pub recipient: String,
    pub summary: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::mailroom::Entity",
        from = "Column::MailroomId",
        to = "super::mailroom::Column::Id"
    )]
    Mailroom,
}

impl Related<super::mailroom::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Mailroom.def()
    }
}

crate::uuid_active_model_behavior!();
