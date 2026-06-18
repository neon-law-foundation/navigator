//! Assemble a `people_list` answer from its form post.
//!
//! The `people_list` widget (`views::components::people_list`) renders
//! row groups of inputs named `p{row}_{part}`; this module folds those
//! back into the single JSON-array string stored as the answer's
//! `value` — so a people-list answer flows through `answers`,
//! `notation_session`, and `forms::resolve` exactly like every other
//! answer string.

use std::collections::BTreeMap;

/// The row parts, in lock-step with `views::components::people_list::PARTS`
/// and `forms::fieldmap`'s `PersonRow`.
const PARTS: [&str; 7] = ["name", "title", "street", "city", "state", "zip", "country"];

/// Fold `p{row}_{part}` form keys into a JSON array of row objects.
/// Rows whose every part is blank are dropped (the widget renders more
/// slots than most respondents need), and empty parts are omitted so
/// the stored answer carries only what was entered.
#[must_use]
pub fn assemble(form: &BTreeMap<String, String>) -> String {
    let mut rows: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    for row in 0..bound(form) {
        let mut object = serde_json::Map::new();
        for part in PARTS {
            if let Some(value) = form.get(&format!("p{row}_{part}")) {
                let value = value.trim();
                if !value.is_empty() {
                    object.insert(part.to_string(), serde_json::Value::String(value.into()));
                }
            }
        }
        if !object.is_empty() {
            rows.push(object);
        }
    }
    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
}

/// One past the highest row index present in the form keys.
fn bound(form: &BTreeMap<String, String>) -> usize {
    form.keys()
        .filter_map(|k| {
            let rest = k.strip_prefix('p')?;
            let (row, part) = rest.split_once('_')?;
            PARTS.contains(&part).then(|| row.parse::<usize>().ok())?
        })
        .map(|row| row + 1)
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::assemble;
    use std::collections::BTreeMap;

    fn form(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn folds_rows_and_drops_blank_ones() {
        let assembled = assemble(&form(&[
            ("p0_name", "Aries Client"),
            ("p0_street", "1 Main St"),
            ("p0_city", "Las Vegas"),
            ("p1_name", ""),
            ("p1_street", "  "),
            ("p2_name", "Libra Partner"),
            ("_csrf", "tok"),
            ("value", "ignored"),
        ]));
        let rows: Vec<serde_json::Value> = serde_json::from_str(&assembled).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "Aries Client");
        assert_eq!(rows[0]["street"], "1 Main St");
        assert!(rows[0].get("zip").is_none(), "empty parts omitted");
        assert_eq!(rows[1]["name"], "Libra Partner");
    }

    #[test]
    fn an_empty_form_assembles_an_empty_list() {
        assert_eq!(assemble(&form(&[("_csrf", "tok")])), "[]");
    }
}
