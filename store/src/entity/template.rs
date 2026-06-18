//! `templates` table — the questionnaire + markdown body that
//! drives a notation. A template is identified by a stable `code`.
//!
//! The markdown body lives in blob storage (`blob_id` → a
//! [`super::blob`]), not an inline column — read it via
//! [`crate::templates::body`]. A template is workspace-shared
//! (`project_id` is `None`) or scoped to one Project; resolve a code
//! with [`crate::templates::resolve`]. Uniqueness is enforced by two
//! partial indexes (shared codes globally unique; project codes unique
//! per Project) — see `m20260624_template_storage_and_scoping`.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "templates")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Stable code. Unique among shared templates; unique per Project
    /// among project-scoped ones (two partial unique indexes). Not a
    /// plain `#[sea_orm(unique)]` because the uniqueness is conditional.
    pub code: String,
    pub title: String,
    /// `entity`, `person`, or `person_and_entity`.
    pub respondent_type: String,
    /// FK → [`super::project`] when this template is scoped to a single
    /// Project; `None` for the workspace-shared public catalog.
    pub project_id: Option<Uuid>,
    /// FK → [`super::blob`] holding the markdown body (with
    /// `{{question_code}}` placeholders). `None` only transiently before
    /// the body is ingested. Read via [`crate::templates::body`].
    pub blob_id: Option<Uuid>,
    /// forms-registry code of the government form this template fills
    /// (e.g. `nv_sos__llc_formation`), from the `form:` frontmatter
    /// key; `None` for Typst-rendered templates.
    pub form_code: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::notation::Entity")]
    Notation,
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id"
    )]
    Project,
    #[sea_orm(
        belongs_to = "super::blob::Entity",
        from = "Column::BlobId",
        to = "super::blob::Column::Id"
    )]
    Blob,
}

impl Related<super::notation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Notation.def()
    }
}

crate::uuid_active_model_behavior!();
