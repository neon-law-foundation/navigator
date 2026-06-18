//! `person_project_roles` — a person's participation on a project
//! (eg. attorney, paralegal, client, co_counsel).
//!
//! The `participation` column is the matter-side role. The presence
//! of the row, not its value, is what gates project visibility for
//! `client` and `staff` tiers. See
//! [`docs/access-model.md`](../../../../docs/access-model.md).

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Participation marking the **staff-side Directly Responsible
/// Individual** — the attorney/paralegal accountable for the matter
/// inside the firm.
pub const PARTICIPATION_STAFF_DRI: &str = "staff_dri";

/// Participation marking the **client-side Directly Responsible
/// Individual** — the one natural person on the client's side accountable
/// for the matter.
///
/// Both DRIs are designated at matter-open as participation roles (this
/// column), not bare nullable pointers, so they reuse the project-scoping
/// the access model already enforces. See `docs/glossary.md` (DRI).
pub const PARTICIPATION_CLIENT_DRI: &str = "client_dri";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "person_project_roles")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub person_id: Uuid,
    pub project_id: Uuid,
    pub participation: String,
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
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id"
    )]
    Project,
}

crate::uuid_active_model_behavior!();
