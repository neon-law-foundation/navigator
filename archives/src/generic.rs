//! Schema-agnostic snapshot helpers.
//!
//! Every entity under `store::entity::*` derives `Serialize` on
//! its `Model`, so we can build an Arrow `RecordBatch` for any
//! table by:
//!
//! 1. Serializing every row to a JSON object.
//! 2. Taking the union of the first row's keys as the column set.
//!    (`serde_json`'s default `Map` is `BTreeMap`-ordered, so the
//!    key list is deterministic regardless of entity-struct field
//!    order.)
//! 3. For each key, building a nullable Utf8 `StringArray` whose
//!    cell is the JSON value rendered as a string — strings and
//!    nulls pass through verbatim; numbers and bools use their
//!    canonical decimal/`true`/`false` form; arrays and objects
//!    are re-serialized as JSON text.
//!
//! All columns are `Utf8` nullable. `BigLake` reads these as
//! `STRING`; downstream queries `CAST` to numeric / boolean
//! when they need a typed value. This is deliberate v1 scope —
//! a richer type mapping (Int64, Boolean, Date) lands when a
//! consumer actually needs it.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use serde::Serialize;
use serde_json::Value;

/// Convert a slice of `Serialize` rows into an Arrow `RecordBatch`.
/// Returns `Ok(None)` if `rows` is empty — the caller decides
/// whether to skip the snapshot or emit an empty-table sentinel.
pub fn batch_from_rows<M: Serialize>(rows: &[M]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let json_rows: Vec<Value> = rows
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .context("serialize entity rows to JSON")?;
    let keys = first_object_keys(&json_rows)?;
    let columns: Vec<ArrayRef> = keys
        .iter()
        .map(|key| string_column(key, &json_rows))
        .collect();
    let schema = Schema::new(
        keys.iter()
            .map(|k| Field::new(k, DataType::Utf8, true))
            .collect::<Vec<_>>(),
    );
    let batch = RecordBatch::try_new(Arc::new(schema), columns)
        .context("assemble RecordBatch from generic rows")?;
    Ok(Some(batch))
}

fn first_object_keys(rows: &[Value]) -> Result<Vec<String>> {
    let first = rows
        .first()
        .ok_or_else(|| anyhow!("rows must be non-empty"))?;
    let Value::Object(map) = first else {
        return Err(anyhow!(
            "expected entity rows to serialize as JSON objects, got {first}"
        ));
    };
    Ok(map.keys().cloned().collect())
}

fn string_column(key: &str, rows: &[Value]) -> ArrayRef {
    let strings: StringArray = rows
        .iter()
        .map(|row| match row.get(key) {
            Some(Value::Null) | None => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Bool(b)) => Some(b.to_string()),
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(other @ (Value::Array(_) | Value::Object(_))) => Some(other.to_string()),
        })
        .collect();
    Arc::new(strings)
}

/// The column set carried by a snapshot batch — used for drift
/// detection against the previous run's stored fingerprint.
#[must_use]
pub fn fingerprint(batch: &RecordBatch) -> Vec<String> {
    batch
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{batch_from_rows, fingerprint};
    use serde::Serialize;

    #[derive(Serialize)]
    struct Row {
        id: i64,
        name: String,
        active: bool,
        roles: Vec<String>,
        nickname: Option<String>,
    }

    #[test]
    fn empty_rows_returns_none() {
        let rows: Vec<Row> = Vec::new();
        assert!(batch_from_rows(&rows).unwrap().is_none());
    }

    #[test]
    fn batch_has_one_column_per_struct_field() {
        let rows = vec![Row {
            id: 1,
            name: "Libra".into(),
            active: true,
            roles: vec!["staff".into()],
            nickname: None,
        }];
        let batch = batch_from_rows(&rows).unwrap().unwrap();
        let mut got: Vec<String> = fingerprint(&batch);
        got.sort();
        let mut want = ["id", "name", "active", "roles", "nickname"]
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>();
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn null_optional_column_survives() {
        let rows = vec![Row {
            id: 1,
            name: "Taurus".into(),
            active: false,
            roles: vec![],
            nickname: None,
        }];
        let batch = batch_from_rows(&rows).unwrap().unwrap();
        let idx = batch.schema().index_of("nickname").unwrap();
        assert_eq!(batch.column(idx).null_count(), 1);
    }

    #[test]
    fn array_columns_are_json_serialized_strings() {
        let rows = vec![Row {
            id: 1,
            name: "Cancer".into(),
            active: true,
            roles: vec!["staff".into(), "admin".into()],
            nickname: Some("C".into()),
        }];
        let batch = batch_from_rows(&rows).unwrap().unwrap();
        let idx = batch.schema().index_of("roles").unwrap();
        let column = batch
            .column(idx)
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        assert_eq!(column.value(0), r#"["staff","admin"]"#);
    }
}
