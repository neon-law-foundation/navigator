//! Turning a `UUID` into the SurrealDB record id that stands for the
//! same row.
//!
//! # The spelling matters, and nothing enforces it
//!
//! Every table is keyed by a `UUID`, and a reference carries that UUID
//! rather than a remapped one. SurrealDB has two ways to write such a
//! key, and they are **different records**:
//!
//! ```text
//! person:u'0198f3a2-…-89ab'   type::is_uuid(meta::id(id)) = true    a UUID key
//! person:⟨0198f3a2-…-89ab⟩    type::is_uuid(meta::id(id)) = false   a string key that looks like one
//! ```
//!
//! Both parse. Both round-trip. Neither errors. And a record link is
//! not validated against an existing row, so a node written with one
//! spelling and a link written with the other resolve to nothing —
//! silently, with no constraint violation to catch it.
//!
//! The Rust API makes that mistake easy rather than hard.
//! `RecordIdKey` has a `From<Uuid>` **and** a `From<String>`, so
//! `RecordId::new("person", id)` is a UUID key while
//! `RecordId::new("person", id.to_string())` is a string key. The two
//! call sites differ by a `.to_string()` and compile identically.
//!
//! [`record_id`] is therefore the one way this workspace mints a record
//! id from a `Uuid`, and [`record_uuid`] is the one way it reads one
//! back. Neither can express the string spelling, which is the point.

use surrealdb::types::{RecordId, RecordIdKey};
use uuid::Uuid;

/// The record id standing for `id` in `table`, keyed as a native UUID.
///
/// ```
/// # use uuid::Uuid;
/// # use store::surreal::record_id;
/// let id = Uuid::now_v7();
/// let record = record_id("person", id);
/// assert_eq!(record.table.as_str(), "person");
/// assert_eq!(store::surreal::record_uuid(&record), Some(id));
/// ```
#[must_use]
pub fn record_id(table: &str, id: Uuid) -> RecordId {
    RecordId::new(table, surrealdb::types::Uuid::from(id))
}

/// The `Uuid` behind a record id, or `None` when the key is not a
/// native UUID.
///
/// `None` is the honest answer for a string key that happens to spell a
/// UUID: it is a different record from the one [`record_id`] would
/// mint, so reporting it as the same `Uuid` would hide exactly the
/// mismatch this module exists to prevent.
#[must_use]
pub fn record_uuid(record: &RecordId) -> Option<Uuid> {
    match &record.key {
        RecordIdKey::Uuid(uuid) => Some(uuid::Uuid::from(*uuid)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{record_id, record_uuid};
    use crate::surreal::test_support::mem;
    use surrealdb::types::{RecordId, RecordIdKey};
    use uuid::Uuid;

    #[test]
    fn a_minted_id_round_trips_through_its_uuid() {
        let id = Uuid::now_v7();
        assert_eq!(record_uuid(&record_id("person", id)), Some(id));
    }

    #[test]
    fn a_string_key_spelling_the_same_uuid_reads_back_as_none() {
        let id = Uuid::now_v7();
        let string_keyed = RecordId::new("person", id.to_string());

        assert!(matches!(string_keyed.key, RecordIdKey::String(_)));
        assert_eq!(
            record_uuid(&string_keyed),
            None,
            "a string key must not report itself as the UUID it spells"
        );
    }

    #[tokio::test]
    async fn the_engine_agrees_the_minted_key_is_a_uuid() {
        let db = mem().await;
        let id = Uuid::now_v7();

        db.query("CREATE $who SET name = 'Libra', email = 'libra@example.com'")
            .bind(("who", record_id("person", id)))
            .await
            .unwrap()
            .check()
            .unwrap();

        let native: Option<bool> = db
            .query("SELECT VALUE type::is_uuid(meta::id(id)) FROM person")
            .await
            .unwrap()
            .take(0)
            .unwrap();
        assert_eq!(
            native,
            Some(true),
            "record_id must mint the native UUID spelling, not a string that looks like one"
        );
    }

    #[tokio::test]
    async fn a_link_minted_the_same_way_dereferences() {
        let db = mem().await;
        let person = Uuid::now_v7();
        let entity = Uuid::now_v7();

        db.query("CREATE $person SET name = 'Libra', email = 'libra@example.com'")
            .bind(("person", record_id("person", person)))
            .await
            .unwrap()
            .check()
            .unwrap();
        db.query(
            "CREATE $entity SET name = 'Acme LLC', \
             entity_type_id = entity_type:llc, jurisdiction_id = jurisdiction:nv",
        )
        .bind(("entity", record_id("entity", entity)))
        .await
        .unwrap()
        .check()
        .unwrap();
        db.query("RELATE $person->entity_role->$entity SET role = 'owner'")
            .bind(("person", record_id("person", person)))
            .bind(("entity", record_id("entity", entity)))
            .await
            .unwrap()
            .check()
            .unwrap();

        // The traversal reads the far node's *name*, so it can only
        // answer if the link resolved to a real row rather than to a
        // record id nothing was written under.
        let reached: Option<String> = db
            .query("SELECT VALUE ->entity_role->entity.name FROM ONLY $person LIMIT 1")
            .bind(("person", record_id("person", person)))
            .await
            .unwrap()
            .take(0)
            .unwrap();
        assert_eq!(reached.as_deref(), Some("Acme LLC"));
    }

    /// A bare UUID held by a referencing row addresses the record it
    /// names — no remapping table, no rewrite of the referencing field.
    ///
    /// `entities` is the one to prove it on: `notations`, `disclosures`,
    /// and `playbooks` all reach it through an `entity_id` they carry as
    /// a plain UUID.
    #[tokio::test]
    async fn a_row_is_addressed_by_the_bare_uuid_that_references_it() {
        let db = mem().await;
        let entity_id = crate::test_support::seed_entity(&db).await;

        // Minting the record id from that bare UUID is the whole
        // reference-preserving step.
        let addressed = crate::entities::find_by_id(&db, entity_id)
            .await
            .unwrap()
            .expect("a bare UUID must address the Surreal row it names");
        assert_eq!(addressed.id, entity_id);

        // And the spelling is the native one, not a string that looks
        // like a UUID — the two are different records.
        let native: Option<bool> = db
            .query("SELECT VALUE type::is_uuid(meta::id(id)) FROM ONLY $what LIMIT 1")
            .bind(("what", record_id("entity", entity_id)))
            .await
            .unwrap()
            .take(0)
            .unwrap();
        assert_eq!(native, Some(true));
    }

    #[tokio::test]
    async fn a_link_written_in_the_other_spelling_dangles() {
        let db = mem().await;
        let entity = Uuid::now_v7();

        // The node is written the way the port writes it…
        db.query(
            "CREATE $entity SET name = 'Acme LLC', \
             entity_type_id = entity_type:llc, jurisdiction_id = jurisdiction:nv",
        )
        .bind(("entity", record_id("entity", entity)))
        .await
        .unwrap()
        .check()
        .unwrap();

        // …and the link is written the way a stray `.to_string()`
        // would write it. Nothing rejects this.
        let dangling: Option<String> = db
            .query("SELECT VALUE name FROM ONLY $entity LIMIT 1")
            .bind(("entity", RecordId::new("entity", entity.to_string())))
            .await
            .unwrap()
            .take(0)
            .unwrap();
        assert_eq!(
            dangling, None,
            "the string spelling must not reach the row the UUID spelling wrote — \
             this is the silent break record_id exists to prevent"
        );
    }
}
