//! `questions` table — one prompt presented to a respondent during
//! template traversal.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// `questions.audience` — only staff see this question.
pub const AUDIENCE_STAFF: &str = "staff";
/// `questions.audience` — only the client sees this question (magic link).
pub const AUDIENCE_CLIENT: &str = "client";
/// `questions.audience` — both sides may answer this question.
pub const AUDIENCE_BOTH: &str = "both";

/// Whether a question with `audience` is shown to the client on the
/// self-serve magic-link surface (`client` or `both`).
#[must_use]
pub fn is_client_facing(audience: &str) -> bool {
    audience == AUDIENCE_CLIENT || audience == AUDIENCE_BOTH
}

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "questions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub code: String,
    pub prompt: String,
    /// `string`, `int`, `bool`, `choice`, …
    pub answer_type: String,
    /// `staff` | `client` | `both` — which side of the intake sees this
    /// question. Never null (defaults `both`).
    pub audience: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::answer::Entity")]
    Answer,
}

impl Related<super::answer::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Answer.def()
    }
}

crate::uuid_active_model_behavior!();

#[cfg(test)]
mod tests {
    use super::{is_client_facing, AUDIENCE_BOTH, AUDIENCE_CLIENT, AUDIENCE_STAFF};

    #[test]
    fn audience_filters_the_client_visible_set() {
        assert!(is_client_facing(AUDIENCE_CLIENT));
        assert!(is_client_facing(AUDIENCE_BOTH));
        assert!(!is_client_facing(AUDIENCE_STAFF));
        // An unknown/garbage audience is not shown to the client.
        assert!(!is_client_facing("nonsense"));
    }
}
