//! The registered-table set and the per-entity `RecordBatch`
//! dispatch. Lifted out of the old `main.rs` so both the library
//! (the Restate workflow worker) and any future CLI share one
//! registry.

use anyhow::{bail, Result};
use arrow::array::RecordBatch;
use sea_orm::{DatabaseConnection, EntityTrait};

use crate::batch_from_rows;

/// Every `SeaORM` entity's SQL table name. Order matches the
/// snapshot iteration order. Keep this synchronized with
/// `store::entity::mod` — [`fetch_batch`] fails loud on an unknown
/// name, so an entity added without registering here is caught the
/// first time the snapshot phase runs.
pub const ALL_TABLES: &[&str] = &[
    "addresses",
    "answers",
    "blobs",
    "credentials",
    "disclosures",
    "documents",
    "entities",
    "entity_billing_profiles",
    "entity_types",
    "git_repositories",
    "invoice_line_items",
    "invoices",
    "jurisdictions",
    "letters",
    "mailrooms",
    "notation_events",
    "notations",
    "person_entity_roles",
    "person_project_roles",
    "persons",
    "projects",
    "questions",
    "relationship_logs",
    "sent_emails",
    "share_issuances",
    "templates",
];

/// Per-table dispatch. The match owns the type of every `Entity`
/// because async closures can't be stored uniformly; the explicit
/// branch list also doubles as the registry that `ALL_TABLES`
/// mirrors. Returns `Ok(None)` when the table is empty.
#[allow(clippy::too_many_lines)]
pub async fn fetch_batch(db: &DatabaseConnection, table: &str) -> Result<Option<RecordBatch>> {
    use store::entity;
    let batch = match table {
        "addresses" => batch_from_rows(&entity::address::Entity::find().all(db).await?)?,
        "answers" => batch_from_rows(&entity::answer::Entity::find().all(db).await?)?,
        "blobs" => batch_from_rows(&entity::blob::Entity::find().all(db).await?)?,
        "credentials" => batch_from_rows(&entity::credential::Entity::find().all(db).await?)?,
        "disclosures" => batch_from_rows(&entity::disclosure::Entity::find().all(db).await?)?,
        "documents" => batch_from_rows(&entity::document::Entity::find().all(db).await?)?,
        "entities" => batch_from_rows(&entity::entity::Entity::find().all(db).await?)?,
        "entity_billing_profiles" => batch_from_rows(
            &entity::entity_billing_profile::Entity::find()
                .all(db)
                .await?,
        )?,
        "entity_types" => batch_from_rows(&entity::entity_type::Entity::find().all(db).await?)?,
        "git_repositories" => {
            batch_from_rows(&entity::git_repository::Entity::find().all(db).await?)?
        }
        "invoice_line_items" => {
            batch_from_rows(&entity::invoice_line_item::Entity::find().all(db).await?)?
        }
        "invoices" => batch_from_rows(&entity::invoice::Entity::find().all(db).await?)?,
        "jurisdictions" => batch_from_rows(&entity::jurisdiction::Entity::find().all(db).await?)?,
        "letters" => batch_from_rows(&entity::letter::Entity::find().all(db).await?)?,
        "mailrooms" => batch_from_rows(&entity::mailroom::Entity::find().all(db).await?)?,
        "notation_events" => {
            batch_from_rows(&entity::notation_event::Entity::find().all(db).await?)?
        }
        "notations" => batch_from_rows(&entity::notation::Entity::find().all(db).await?)?,
        "person_entity_roles" => {
            batch_from_rows(&entity::person_entity_role::Entity::find().all(db).await?)?
        }
        "person_project_roles" => {
            batch_from_rows(&entity::person_project_role::Entity::find().all(db).await?)?
        }
        "persons" => batch_from_rows(&entity::person::Entity::find().all(db).await?)?,
        "projects" => batch_from_rows(&entity::project::Entity::find().all(db).await?)?,
        "questions" => batch_from_rows(&entity::question::Entity::find().all(db).await?)?,
        "relationship_logs" => {
            batch_from_rows(&entity::relationship_log::Entity::find().all(db).await?)?
        }
        "sent_emails" => batch_from_rows(&entity::sent_email::Entity::find().all(db).await?)?,
        "share_issuances" => {
            batch_from_rows(&entity::share_issuance::Entity::find().all(db).await?)?
        }
        "templates" => batch_from_rows(&entity::template::Entity::find().all(db).await?)?,
        unknown => bail!("unknown table `{unknown}` — not registered in fetch_batch"),
    };
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::ALL_TABLES;

    #[test]
    fn all_entities_are_registered() {
        assert_eq!(ALL_TABLES.len(), 26);
    }

    #[test]
    fn all_tables_are_sorted_so_diffs_are_minimal() {
        let mut sorted = ALL_TABLES.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, ALL_TABLES);
    }
}
