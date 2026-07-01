//! Template resolution and body access.
//!
//! Two changes this module owns (see `docs/notation.md`):
//!
//! - **Body in storage.** A Template's markdown body no longer lives in
//!   a `templates.body` TEXT column; it is a content-addressed
//!   [`crate::blobs`] blob referenced by `templates.blob_id`. [`body`]
//!   fetches it transparently.
//! - **Project scoping.** A Template is either workspace-shared
//!   (`project_id IS NULL`) or scoped to one Project. [`resolve`] looks
//!   a code up preferring the caller's Project, falling back to the
//!   shared row — so a Project can override a shared `code` or define
//!   its own without colliding with another Project's.

use std::sync::Arc;

use cloud::StorageService;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::entity::template;
use crate::Db;

/// Errors from [`body`].
#[derive(Debug, thiserror::Error)]
pub enum TemplateBodyError {
    #[error("template `{0}` has no stored body (blob_id is null)")]
    MissingBody(Uuid),
    #[error("blob: {0}")]
    Blob(#[from] crate::blobs::BlobError),
    #[error("template body is not valid UTF-8")]
    NotUtf8,
}

/// Resolve a template by `code` for a given Project context. Prefers a
/// Project-scoped row (`project_id = project_id`), then falls back to
/// the workspace-shared row (`project_id IS NULL`). Returns `None` when
/// neither exists.
///
/// Pass `project_id = None` to look up only the shared row (the public
/// catalog).
pub async fn resolve(
    db: &Db,
    project_id: Option<Uuid>,
    code: &str,
) -> Result<Option<template::Model>, sea_orm::DbErr> {
    if let Some(pid) = project_id {
        if let Some(scoped) = template::Entity::find()
            .filter(template::Column::Code.eq(code))
            .filter(template::Column::ProjectId.eq(pid))
            .filter(template::Column::IsCurrent.eq(true))
            .one(db)
            .await?
        {
            return Ok(Some(scoped));
        }
    }
    template::Entity::find()
        .filter(template::Column::Code.eq(code))
        .filter(template::Column::ProjectId.is_null())
        .filter(template::Column::IsCurrent.eq(true))
        .one(db)
        .await
}

/// The spec fields that make up one Template version. Identity for
/// "did this change?" is the tuple `(title, respondent_type, blob_id,
/// form_code)` — `code` and `project_id` locate the version line.
pub struct Version {
    pub title: String,
    pub respondent_type: String,
    pub blob_id: Option<Uuid>,
    pub form_code: Option<String>,
}

/// Outcome of [`save_version`].
pub enum Saved {
    /// The current row already matched the spec; nothing was written.
    Unchanged(template::Model),
    /// A new current row was written — the first version of this code, or
    /// a change that retired the prior version.
    Written(template::Model),
}

impl Saved {
    /// The now-current Template row, either way.
    #[must_use]
    pub fn into_model(self) -> template::Model {
        match self {
            Saved::Unchanged(m) | Saved::Written(m) => m,
        }
    }

    /// Whether this call wrote a new row.
    #[must_use]
    pub fn was_written(&self) -> bool {
        matches!(self, Saved::Written(_))
    }
}

/// Make `version` the current Template for `(project_id, code)`, appending
/// it as a new row and retiring any existing current row.
///
/// Immutable by policy: a spec change never rewrites a row a Notation
/// pinned via `notation.template_id`. When `version` matches the existing
/// current row, this is a no-op and returns [`Saved::Unchanged`] — so a
/// re-seed of an unchanged template does not churn versions.
pub async fn save_version(
    db: &Db,
    project_id: Option<Uuid>,
    code: &str,
    version: Version,
) -> Result<Saved, sea_orm::DbErr> {
    use sea_orm::{ActiveModelTrait, ActiveValue};

    let current = resolve_exact(db, project_id, code).await?;
    if let Some(existing) = &current {
        if existing.title == version.title
            && existing.respondent_type == version.respondent_type
            && existing.blob_id == version.blob_id
            && existing.form_code == version.form_code
        {
            return Ok(Saved::Unchanged(existing.clone()));
        }
    }
    if let Some(existing) = current {
        let mut retire: template::ActiveModel = existing.into();
        retire.is_current = ActiveValue::Set(false);
        retire.update(db).await?;
    }
    let written = template::ActiveModel {
        code: ActiveValue::Set(code.to_string()),
        title: ActiveValue::Set(version.title),
        respondent_type: ActiveValue::Set(version.respondent_type),
        project_id: ActiveValue::Set(project_id),
        blob_id: ActiveValue::Set(version.blob_id),
        form_code: ActiveValue::Set(version.form_code),
        is_current: ActiveValue::Set(true),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(Saved::Written(written))
}

/// The current row for an exact `(project_id, code)` — no shared
/// fallback, unlike [`resolve`]. `save_version`'s pointer flip must act on
/// the same scope it writes to.
async fn resolve_exact(
    db: &Db,
    project_id: Option<Uuid>,
    code: &str,
) -> Result<Option<template::Model>, sea_orm::DbErr> {
    let mut q = template::Entity::find()
        .filter(template::Column::Code.eq(code))
        .filter(template::Column::IsCurrent.eq(true));
    q = match project_id {
        Some(pid) => q.filter(template::Column::ProjectId.eq(pid)),
        None => q.filter(template::Column::ProjectId.is_null()),
    };
    q.one(db).await
}

/// Fetch a Template's markdown body from blob storage.
pub async fn body(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    template: &template::Model,
) -> Result<String, TemplateBodyError> {
    let blob_id = template
        .blob_id
        .ok_or(TemplateBodyError::MissingBody(template.id))?;
    let bytes = crate::blobs::fetch(db, storage, blob_id).await?;
    String::from_utf8(bytes).map_err(|_| TemplateBodyError::NotUtf8)
}

#[cfg(test)]
mod tests {
    use super::{body, resolve};
    use crate::entity::{project, template};
    use crate::is_unique_violation;
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    use uuid::Uuid;

    async fn fs_storage() -> std::sync::Arc<dyn cloud::StorageService> {
        std::sync::Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join("navigator-templates-test"))
                .await
                .unwrap(),
        )
    }

    async fn project(db: &crate::Db) -> Uuid {
        let __dri = crate::test_support::dri_person(db).await;
        project::ActiveModel {
            name: ActiveValue::Set("matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn insert_template(db: &crate::Db, code: &str, project_id: Option<Uuid>) -> Uuid {
        template::ActiveModel {
            code: ActiveValue::Set(code.into()),
            title: ActiveValue::Set(code.into()),
            respondent_type: ActiveValue::Set("entity".into()),
            project_id: ActiveValue::Set(project_id),
            blob_id: ActiveValue::Set(None),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn resolve_prefers_project_scoped_then_falls_back_to_shared() {
        let db = crate::test_support::pg().await;
        let p = project(&db).await;
        let shared = insert_template(&db, "amendment", None).await;
        let scoped = insert_template(&db, "amendment", Some(p)).await;

        // From the project: the scoped row wins.
        assert_eq!(
            resolve(&db, Some(p), "amendment")
                .await
                .unwrap()
                .unwrap()
                .id,
            scoped
        );
        // No project context: the shared row.
        assert_eq!(
            resolve(&db, None, "amendment").await.unwrap().unwrap().id,
            shared
        );
        // A different project with no scoped row falls back to shared.
        let other = project(&db).await;
        assert_eq!(
            resolve(&db, Some(other), "amendment")
                .await
                .unwrap()
                .unwrap()
                .id,
            shared
        );
    }

    #[tokio::test]
    async fn shared_and_project_scoped_codes_coexist() {
        let db = crate::test_support::pg().await;
        let p = project(&db).await;
        // Same code, one shared + one scoped — both insert (partial
        // unique indexes don't collide across the NULL / non-NULL split).
        insert_template(&db, "consent", None).await;
        insert_template(&db, "consent", Some(p)).await;
        let all = template::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.iter().filter(|t| t.code == "consent").count(), 2);
    }

    #[tokio::test]
    async fn two_shared_templates_with_the_same_code_collide() {
        let db = crate::test_support::pg().await;
        insert_template(&db, "ca__llc_operating_agreement", None).await;
        let err = template::ActiveModel {
            code: ActiveValue::Set("ca__llc_operating_agreement".into()),
            title: ActiveValue::Set("dup".into()),
            respondent_type: ActiveValue::Set("entity".into()),
            project_id: ActiveValue::Set(None),
            blob_id: ActiveValue::Set(None),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap_err();
        assert!(
            is_unique_violation(&err),
            "expected a unique violation: {err}"
        );
    }

    #[tokio::test]
    async fn two_templates_with_the_same_code_in_one_project_collide() {
        let db = crate::test_support::pg().await;
        let p = project(&db).await;
        insert_template(&db, "amendment", Some(p)).await;
        let err = template::ActiveModel {
            code: ActiveValue::Set("amendment".into()),
            title: ActiveValue::Set("dup".into()),
            respondent_type: ActiveValue::Set("entity".into()),
            project_id: ActiveValue::Set(Some(p)),
            blob_id: ActiveValue::Set(None),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap_err();
        assert!(
            is_unique_violation(&err),
            "expected a unique violation: {err}"
        );
    }

    #[tokio::test]
    async fn body_reads_the_markdown_back_from_blob_storage() {
        let db = crate::test_support::pg().await;
        let storage = fs_storage().await;
        let blob_id = crate::blobs::ingest(&db, &storage, b"# Deed\n\n{{buyer}}", "text/markdown")
            .await
            .unwrap();
        let tmpl = template::ActiveModel {
            code: ActiveValue::Set("deed".into()),
            title: ActiveValue::Set("Deed".into()),
            respondent_type: ActiveValue::Set("person".into()),
            project_id: ActiveValue::Set(None),
            blob_id: ActiveValue::Set(Some(blob_id)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let text = body(&db, &storage, &tmpl).await.unwrap();
        assert_eq!(text, "# Deed\n\n{{buyer}}");
    }

    fn version(title: &str, blob: Option<Uuid>) -> super::Version {
        super::Version {
            title: title.into(),
            respondent_type: "entity".into(),
            blob_id: blob,
            form_code: None,
        }
    }

    #[tokio::test]
    async fn save_version_writes_first_version_and_resolve_reads_it() {
        let db = crate::test_support::pg().await;
        let saved = super::save_version(&db, None, "amendment", version("Amendment", None))
            .await
            .unwrap();
        assert!(saved.was_written());
        let current = resolve(&db, None, "amendment").await.unwrap().unwrap();
        assert_eq!(current.id, saved.into_model().id);
        assert!(current.is_current);
    }

    #[tokio::test]
    async fn unchanged_re_save_is_a_no_op() {
        let db = crate::test_support::pg().await;
        let first = super::save_version(&db, None, "amendment", version("Amendment", None))
            .await
            .unwrap()
            .into_model();
        let again = super::save_version(&db, None, "amendment", version("Amendment", None))
            .await
            .unwrap();
        assert!(
            !again.was_written(),
            "an identical spec must not churn versions"
        );
        assert_eq!(again.into_model().id, first.id);
        assert_eq!(template::Entity::find().all(&db).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn changing_a_template_appends_a_version_and_pins_the_old_one() {
        let db = crate::test_support::pg().await;
        let v1 = super::save_version(&db, None, "amendment", version("Amendment", None))
            .await
            .unwrap()
            .into_model();
        let v2 = super::save_version(&db, None, "amendment", version("Amendment v2", None))
            .await
            .unwrap();
        assert!(v2.was_written());
        let v2 = v2.into_model();
        assert_ne!(v1.id, v2.id, "a change appends a new row, never rewrites");

        // resolve returns the new current version.
        assert_eq!(
            resolve(&db, None, "amendment").await.unwrap().unwrap().id,
            v2.id
        );
        // The old row survives, retired — a Notation that pinned
        // `template_id = v1` still finds its exact bytes.
        let pinned = template::Entity::find_by_id(v1.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(!pinned.is_current);
        assert_eq!(pinned.title, "Amendment");
    }
}
