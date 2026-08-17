//! `SurrealDB` table discovery and row-to-`RecordBatch` reads.
//!
//! [`ALL_TABLES`] is derived from every `DEFINE TABLE` in the shipped
//! `navigator.surql`, rather than a hand-maintained registry. A newly defined
//! table therefore reaches the analytical archive automatically; a read error
//! remains a per-table failure in the nightly summary instead of becoming a
//! silent omission.

use std::sync::LazyLock;

use anyhow::{Context, Result};
use arrow::array::RecordBatch;
use store::surreal::SurrealDb;

use crate::batch_from_rows;

/// Every table declared in Navigator's Surreal schema, in stable order.
pub static ALL_TABLES: LazyLock<Vec<String>> = LazyLock::new(store::schema::table_names);

/// Select one table's rows and convert their JSON representation to a batch.
/// Returns `Ok(None)` when the table is empty.
pub async fn fetch_batch(db: &SurrealDb, table: &str) -> Result<Option<RecordBatch>> {
    let mut response = db
        .query("SELECT * FROM type::table($table)")
        .bind(("table", table.to_string()))
        .await
        .with_context(|| format!("select rows from Surreal table `{table}`"))?;
    let rows: Vec<surrealdb::types::Value> = response
        .take(0)
        .with_context(|| format!("read rows from Surreal table `{table}`"))?;
    let rows: Vec<serde_json::Value> = rows
        .into_iter()
        .map(surrealdb::types::Value::into_json_value)
        .collect();
    batch_from_rows(&rows)
}

#[cfg(test)]
mod tests {
    use super::ALL_TABLES;

    #[test]
    fn all_entities_are_registered() {
        assert_eq!(
            ALL_TABLES.as_slice(),
            store::schema::table_names().as_slice()
        );
        assert!(!ALL_TABLES.is_empty());
    }

    #[test]
    fn all_tables_are_sorted_so_diffs_are_minimal() {
        let mut sorted = ALL_TABLES.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, *ALL_TABLES);
    }
}
