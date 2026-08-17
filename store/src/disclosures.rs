//! `disclosures` — the formal disclosures the firm records against an
//! Entity or a matter (conflicts, related-party, and the rest).
//!
//! The conflict graph reads the `conflict` and `related_party` kinds:
//! a recorded disclosure on any entity the traversal reaches always
//! surfaces, regardless of how it was reached. See [`crate::conflicts`].
//!
//! # This table lives in SurrealDB
//!
//! `disclosures` moved with wave six of #1093 (ENG-160).

use chrono::{DateTime, Utc};
use serde::Serialize;
use surrealdb::types::SurrealValue;
use thiserror::Error;
use uuid::Uuid;

use crate::surreal::{record_id, record_uuid, SurrealDb};

/// The table these rows live in.
const TABLE: &str = "disclosure";
const ENTITY_TABLE: &str = "entity";

/// A conflict the firm has recorded against an Entity.
pub const KIND_CONFLICT: &str = "conflict";
/// A related-party relationship the firm has recorded.
pub const KIND_RELATED_PARTY: &str = "related_party";

/// The kinds the conflict graph surfaces. Other kinds are recorded and
/// read back, but do not by themselves raise a conflict finding.
pub const CONFLICT_KINDS: &[&str] = &[KIND_CONFLICT, KIND_RELATED_PARTY];

/// What can go wrong reading or writing a disclosure.
#[derive(Debug, Error)]
pub enum DisclosureError {
    /// A database operation failed.
    #[error("database: {0}")]
    Db(#[from] surrealdb::Error),
}

/// One formal disclosure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Disclosure {
    pub id: Uuid,
    /// The company the disclosure is about, when it is about one.
    pub entity_id: Option<Uuid>,
    /// The matter the disclosure is about, when it is about one.
    pub project_id: Option<Uuid>,
    pub kind: String,
    pub summary: String,
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The row as the engine reads and writes it.
#[derive(SurrealValue)]
struct DisclosureRow {
    id: surrealdb::types::RecordId,
    entity_id: Option<surrealdb::types::RecordId>,
    project_id: Option<surrealdb::types::RecordId>,
    kind: String,
    summary: String,
    inserted_at: surrealdb::types::Datetime,
    updated_at: surrealdb::types::Datetime,
}

impl DisclosureRow {
    fn into_disclosure(self) -> Option<Disclosure> {
        Some(Disclosure {
            id: record_uuid(&self.id)?,
            entity_id: self.entity_id.as_ref().and_then(record_uuid),
            project_id: self.project_id.as_ref().and_then(record_uuid),
            kind: self.kind,
            summary: self.summary,
            inserted_at: self.inserted_at.into(),
            updated_at: self.updated_at.into(),
        })
    }
}

/// The projection every read shares.
const SELECT: &str = "id, entity_id, project_id, kind, summary, inserted_at, updated_at";

/// What to record for one disclosure.
#[derive(Debug, Clone)]
pub struct NewDisclosure<'a> {
    pub entity_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub kind: &'a str,
    pub summary: &'a str,
}

/// Record one disclosure, returning its id.
///
/// # Errors
/// Propagates any database error.
pub async fn record(db: &SurrealDb, new: &NewDisclosure<'_>) -> Result<Uuid, DisclosureError> {
    let id = Uuid::now_v7();
    let mut response = db
        .query(format!(
            "CREATE $id SET \
             entity_id = $entity_id, project_id = $project_id, kind = $kind, summary = $summary \
             RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, id)))
        .bind((
            "entity_id",
            new.entity_id.map(|e| record_id(ENTITY_TABLE, e)),
        ))
        .bind((
            "project_id",
            new.project_id
                .map(|p| record_id(crate::projects::PROJECT_TABLE, p)),
        ))
        .bind(("kind", new.kind.to_string()))
        .bind(("summary", new.summary.to_string()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<DisclosureRow> = response.take(0)?;
    Ok(row
        .and_then(DisclosureRow::into_disclosure)
        .map_or(id, |d| d.id))
}

/// Every disclosure, oldest first — the lawyer directory listing.
///
/// # Errors
/// Propagates any database error.
pub async fn all(db: &SurrealDb) -> Result<Vec<Disclosure>, DisclosureError> {
    let mut response = db
        .query(format!("SELECT {SELECT} FROM {TABLE} ORDER BY id ASC"))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let rows: Vec<DisclosureRow> = response.take(0)?;
    Ok(rows
        .into_iter()
        .filter_map(DisclosureRow::into_disclosure)
        .collect())
}

/// Every conflict / related-party disclosure that names an Entity,
/// grouped by that Entity — the lookup the conflict graph builds once per
/// check.
///
/// Bulk rather than per-entity: an under-inclusive result here is a
/// **missed conflict**, so the graph loads the whole set up front rather
/// than querying per node reached.
///
/// # Errors
/// Propagates any database error.
pub async fn conflict_summaries_by_entity(
    db: &SurrealDb,
) -> Result<std::collections::HashMap<Uuid, Vec<String>>, DisclosureError> {
    let mut response = db
        .query(format!(
            "SELECT {SELECT} FROM {TABLE} \
             WHERE kind IN $kinds AND entity_id != NONE ORDER BY id ASC"
        ))
        .bind((
            "kinds",
            CONFLICT_KINDS
                .iter()
                .map(|k| (*k).to_string())
                .collect::<Vec<_>>(),
        ))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let rows: Vec<DisclosureRow> = response.take(0)?;
    let mut by_entity: std::collections::HashMap<Uuid, Vec<String>> =
        std::collections::HashMap::new();
    for row in rows.into_iter().filter_map(DisclosureRow::into_disclosure) {
        if let Some(entity_id) = row.entity_id {
            by_entity.entry(entity_id).or_default().push(row.summary);
        }
    }
    Ok(by_entity)
}

#[cfg(test)]
mod tests {
    use super::{
        all, conflict_summaries_by_entity, record, NewDisclosure, KIND_CONFLICT, KIND_RELATED_PARTY,
    };
    use crate::surreal::test_support::mem;
    use crate::test_support::{seed_entity, seed_project_surreal};

    #[tokio::test]
    async fn a_disclosure_records_against_an_entity_a_matter_or_neither() {
        let surreal = mem().await;
        let entity_id = seed_entity(&surreal).await;
        let project_id = seed_project_surreal(&surreal, "matter").await;

        record(
            &surreal,
            &NewDisclosure {
                entity_id: Some(entity_id),
                project_id: None,
                kind: KIND_CONFLICT,
                summary: "Adverse to an existing client",
            },
        )
        .await
        .unwrap();
        record(
            &surreal,
            &NewDisclosure {
                entity_id: None,
                project_id: Some(project_id),
                kind: "related_party",
                summary: "Matter-scoped disclosure",
            },
        )
        .await
        .unwrap();
        // Neither link set: a disclosure the firm made that is about
        // nothing in the database yet.
        record(
            &surreal,
            &NewDisclosure {
                entity_id: None,
                project_id: None,
                kind: KIND_CONFLICT,
                summary: "Unattached",
            },
        )
        .await
        .unwrap();

        let rows = all(&surreal).await.unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].entity_id, Some(entity_id));
        assert_eq!(rows[1].project_id, Some(project_id));
        assert!(rows[2].entity_id.is_none() && rows[2].project_id.is_none());
    }

    #[tokio::test]
    async fn the_conflict_lookup_takes_both_kinds_and_skips_unattached_rows() {
        // Under-inclusive here is a missed conflict, so both kinds must
        // appear — and a row naming no entity has no entity to raise a
        // finding against.
        let surreal = mem().await;
        let entity_id = seed_entity(&surreal).await;

        for (kind, summary) in [
            (KIND_CONFLICT, "Adverse to Acme"),
            (KIND_RELATED_PARTY, "Shares a principal with Acme"),
        ] {
            record(
                &surreal,
                &NewDisclosure {
                    entity_id: Some(entity_id),
                    project_id: None,
                    kind,
                    summary,
                },
            )
            .await
            .unwrap();
        }
        // A kind the graph does not surface, and an unattached conflict.
        record(
            &surreal,
            &NewDisclosure {
                entity_id: Some(entity_id),
                project_id: None,
                kind: "engagement_terms",
                summary: "Not a conflict kind",
            },
        )
        .await
        .unwrap();
        record(
            &surreal,
            &NewDisclosure {
                entity_id: None,
                project_id: None,
                kind: KIND_CONFLICT,
                summary: "Names no entity",
            },
        )
        .await
        .unwrap();

        let by_entity = conflict_summaries_by_entity(&surreal).await.unwrap();
        assert_eq!(by_entity.len(), 1);
        assert_eq!(
            by_entity[&entity_id],
            vec!["Adverse to Acme", "Shares a principal with Acme"],
        );
    }

    #[tokio::test]
    async fn the_conflict_lookup_is_empty_with_nothing_recorded() {
        let surreal = mem().await;
        assert!(conflict_summaries_by_entity(&surreal)
            .await
            .unwrap()
            .is_empty());
    }
}
