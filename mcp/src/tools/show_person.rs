//! `aida_show_person` MCP tool.
//!
//! Fuzzy-find people by name and/or email. Both fields are matched
//! case-insensitively as substrings (`LOWER(col) LIKE '%needle%'`),
//! so the model can call this with whatever fragment the user
//! mentions ("libra", "@neonlaw.com", "ari") and get back every row
//! that matches. When both fields are supplied they are combined
//! with a logical AND. Returns the full list of matches so the
//! model can disambiguate in a follow-up turn. Zero matches is a
//! successful empty result (`count: 0`), not an error — fuzzy
//! searches succeed even when they find nothing.

use sea_orm::sea_query::{Expr, Func};
use sea_orm::{Condition, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Deserialize;
use serde_json::{json, Value};
use store::entity::person;
use store::Db;

use super::ToolError;

/// Cap on the number of rows returned in a single response. Keeps the
/// MCP payload bounded when the model passes an overly broad needle
/// (e.g. a single letter). Picked to be generous for human-scale
/// directories without flooding the model's context.
const MAX_RESULTS: u64 = 50;

/// Tool descriptor advertised by `tools/list`.
#[must_use]
pub fn descriptor() -> Value {
    json!({
        "name": "aida_show_person",
        "description": "Fuzzy-find people in Navigator by name and/or email. \
                        Both fields are matched case-insensitively as substrings, \
                        so partial fragments (\"libra\", \"@neonlaw.com\") work. \
                        At least one of `name` or `email` is required; when both \
                        are given they are ANDed. Returns up to 50 matches with \
                        id, name, email, role, and oidc_subject.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Substring of the person's name. Case-insensitive."
                },
                "email": {
                    "type": "string",
                    "description": "Substring of the person's email. Case-insensitive."
                }
            },
            "additionalProperties": false
        }
    })
}

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

/// Run the fuzzy search and return the MCP `result` payload.
pub async fn call(db: &Db, arguments: &Value) -> Result<Value, ToolError> {
    let args: Args = super::decode_args(arguments)?;

    let name = args
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let email = args
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if name.is_none() && email.is_none() {
        return Err(ToolError::InvalidArguments(
            "provide at least one of `name` or `email`".into(),
        ));
    }

    let mut conditions = Condition::all();
    if let Some(needle) = name {
        conditions = conditions.add(case_insensitive_contains(person::Column::Name, needle));
    }
    if let Some(needle) = email {
        conditions = conditions.add(case_insensitive_contains(person::Column::Email, needle));
    }

    let rows = person::Entity::find()
        .filter(conditions)
        .order_by_asc(person::Column::Name)
        .limit(MAX_RESULTS)
        .all(db)
        .await?;

    let persons: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "id": row.id,
                "name": row.name,
                "email": row.email,
                "role": row.role.as_str(),
                "oidc_subject": row.oidc_subject,
            })
        })
        .collect();

    let summary = match rows.len() {
        0 => format!("No people matched {}.", describe_needle(name, email)),
        1 => format!(
            "Found 1 person: {} <{}> (id={}).",
            rows[0].name, rows[0].email, rows[0].id
        ),
        n => {
            let listed = rows
                .iter()
                .map(|r| format!("{} <{}>", r.name, r.email))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Found {n} people: {listed}.")
        }
    };

    Ok(json!({
        "content": [{ "type": "text", "text": summary }],
        "structuredContent": {
            "count": persons.len(),
            "persons": persons,
        }
    }))
}

/// Build `LOWER(col) LIKE '%lower(needle)%'` as a `SeaORM` expression.
/// Lowercasing both sides keeps the predicate identical on Postgres
/// (which is case-sensitive for `LIKE`) and `SQLite` (which folds
/// ASCII but not non-ASCII).
fn case_insensitive_contains(
    column: person::Column,
    needle: &str,
) -> sea_orm::sea_query::SimpleExpr {
    Expr::expr(Func::lower(Expr::col(column))).like(format!("%{}%", needle.to_lowercase()))
}

fn describe_needle(name: Option<&str>, email: Option<&str>) -> String {
    match (name, email) {
        (Some(n), Some(e)) => format!("name~={n} AND email~={e}"),
        (Some(n), None) => format!("name~={n}"),
        (None, Some(e)) => format!("email~={e}"),
        (None, None) => "<unknown>".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{call, descriptor, MAX_RESULTS};
    use crate::tools::ToolError;
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use serde_json::json;
    use store::entity::person;

    async fn db() -> store::Db {
        let db = store::test_support::pg().await;
        db
    }

    async fn seed(db: &store::Db, name: &str, email: &str) -> person::Model {
        person::ActiveModel {
            name: ActiveValue::Set(name.into()),
            email: ActiveValue::Set(email.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[test]
    fn descriptor_names_the_tool_under_aida_namespace() {
        let d = descriptor();
        assert_eq!(d["name"], "aida_show_person");
        // Neither key is hard-required at the schema level — the
        // handler enforces "at least one" so the model gets a clearer
        // error than a JSON-Schema mismatch.
        assert_eq!(d["inputSchema"]["additionalProperties"], false);
        assert!(d["inputSchema"]["properties"]["name"].is_object());
        assert!(d["inputSchema"]["properties"]["email"].is_object());
    }

    #[tokio::test]
    async fn fuzzy_name_match_is_case_insensitive_and_substring() {
        let db = db().await;
        seed(&db, "Libra", "libra@example.com").await;
        // Lowercased fragment of the *middle* of the name.
        let result = call(&db, &json!({ "name": "ibr" })).await.unwrap();
        assert_eq!(result["structuredContent"]["count"], 1);
        assert_eq!(result["structuredContent"]["persons"][0]["name"], "Libra");
    }

    #[tokio::test]
    async fn fuzzy_email_match_works_on_domain_fragment() {
        let db = db().await;
        seed(&db, "Libra", "libra@neonlaw.com").await;
        seed(&db, "Taurus", "taurus@example.com").await;
        let result = call(&db, &json!({ "email": "neonlaw" })).await.unwrap();
        assert_eq!(result["structuredContent"]["count"], 1);
        assert_eq!(
            result["structuredContent"]["persons"][0]["email"],
            "libra@neonlaw.com"
        );
    }

    #[tokio::test]
    async fn name_match_returns_multiple_rows_sorted_by_name() {
        let db = db().await;
        // Insert out of alphabetical order so we can prove the sort.
        // All three signs share the substring "ari".
        seed(&db, "Sagittarius", "sagittarius@example.com").await;
        seed(&db, "Aquarius", "aquarius@example.com").await;
        seed(&db, "Aries", "aries@example.com").await;
        let result = call(&db, &json!({ "name": "ari" })).await.unwrap();
        assert_eq!(result["structuredContent"]["count"], 3);
        let names: Vec<&str> = result["structuredContent"]["persons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["Aquarius", "Aries", "Sagittarius"]);
    }

    #[tokio::test]
    async fn both_name_and_email_are_anded() {
        let db = db().await;
        seed(&db, "Aquarius", "aquarius@neonlaw.com").await;
        seed(&db, "Aries", "aries@example.com").await;
        seed(&db, "Sagittarius", "sagittarius@neonlaw.com").await;
        let result = call(&db, &json!({ "name": "ari", "email": "neonlaw" }))
            .await
            .unwrap();
        assert_eq!(result["structuredContent"]["count"], 2);
        let names: Vec<&str> = result["structuredContent"]["persons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["Aquarius", "Sagittarius"]);
    }

    #[tokio::test]
    async fn missing_both_keys_is_invalid_arguments() {
        let db = db().await;
        let err = call(&db, &json!({})).await.unwrap_err();
        match err {
            ToolError::InvalidArguments(m) => {
                assert!(m.contains("name") && m.contains("email"));
            }
            other => panic!("expected InvalidArguments, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn whitespace_only_keys_are_treated_as_missing() {
        let db = db().await;
        let err = call(&db, &json!({ "name": "   ", "email": "" }))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    #[tokio::test]
    async fn whitespace_around_needle_is_trimmed_before_matching() {
        let db = db().await;
        seed(&db, "Libra", "libra@example.com").await;
        let result = call(&db, &json!({ "name": "  libra\n" })).await.unwrap();
        assert_eq!(result["structuredContent"]["count"], 1);
    }

    #[tokio::test]
    async fn no_match_returns_empty_result_not_an_error() {
        let db = db().await;
        seed(&db, "Libra", "libra@example.com").await;
        let result = call(&db, &json!({ "name": "ghost" })).await.unwrap();
        assert_eq!(result["structuredContent"]["count"], 0);
        assert_eq!(
            result["structuredContent"]["persons"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("No people matched"), "got: {text}");
        assert!(text.contains("name~=ghost"), "got: {text}");
    }

    #[tokio::test]
    async fn no_match_with_both_keys_describes_both_in_summary() {
        let db = db().await;
        let result = call(&db, &json!({ "name": "ghost", "email": "void" }))
            .await
            .unwrap();
        assert_eq!(result["structuredContent"]["count"], 0);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("name~=ghost"), "got: {text}");
        assert!(text.contains("email~=void"), "got: {text}");
    }

    #[tokio::test]
    async fn results_are_capped_at_max_results() {
        let db = db().await;
        let total = MAX_RESULTS + 5;
        for i in 0..total {
            seed(&db, "Aries", &format!("user{i:03}@example.com")).await;
        }
        let result = call(&db, &json!({ "name": "aries" })).await.unwrap();
        assert_eq!(
            result["structuredContent"]["count"].as_u64().unwrap(),
            MAX_RESULTS
        );
    }

    #[tokio::test]
    async fn summary_text_uses_singular_for_one_match() {
        let db = db().await;
        seed(&db, "Libra", "libra@example.com").await;
        let result = call(&db, &json!({ "name": "libra" })).await.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("Found 1 person:"), "got: {text}");
        assert!(text.contains("Libra"));
    }

    #[tokio::test]
    async fn summary_text_uses_plural_for_multiple_matches() {
        let db = db().await;
        seed(&db, "Aquarius", "aquarius@example.com").await;
        seed(&db, "Aries", "aries@example.com").await;
        let result = call(&db, &json!({ "name": "ari" })).await.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("Found 2 people:"), "got: {text}");
    }
}
