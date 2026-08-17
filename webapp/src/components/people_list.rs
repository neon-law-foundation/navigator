//! The `people_list` question widget, as a Dioxus component (issue #641,
//! Phase 2).
//!
//! The successor to the `views::components::people_list`. A bounded set of
//! person-row groups (name + contact + mailing address + optional title) for
//! questions like "who are the managing members?". Each vendored government form
//! prints a fixed number of slots, so the widget renders a fixed number of row
//! groups and the respondent leaves trailing rows blank. Inputs are named
//! `p{row}_{part}`; the POST handler assembles them into one JSON-array answer.
//! Rendered inside a form so the inputs post with the rest of it.

use dioxus::prelude::*;

/// The row parts, in render order: input-name suffix + visible label. The keys
/// are the canonical `people` aggregate shape (`store::question_registry`'s
/// `PERSON_ROW_PARTS`); the labels are this render layer's presentation. Must
/// stay in lock-step with `forms::fieldmap`'s `PersonRow`.
pub const PARTS: [(&str, &str); 9] = [
    ("name", "Full legal name"),
    ("email", "Email"),
    ("title", "Title (officers only)"),
    ("phone", "Phone"),
    ("street", "Street address"),
    ("city", "City"),
    ("state", "State"),
    ("zip", "ZIP / postal code"),
    ("country", "Country"),
];

/// Parse a prior people-list answer (a JSON array of objects) into per-row part
/// values. Deserializing with `serde_json` (already in the tree via Dioxus)
/// keeps escaped quotes, braces, and unicode escapes inside a value intact: a
/// hand-rolled scan truncated a value at its first `\"` and mis-split any object
/// whose value contained a `{`/`}`. Tolerant by design — a prior value that is
/// not a JSON array just pre-fills nothing, and a non-string field is skipped.
fn prior_rows(prior_json: &str) -> Vec<Vec<(String, String)>> {
    let Ok(serde_json::Value::Array(objects)) = serde_json::from_str(prior_json) else {
        return Vec::new();
    };
    objects
        .iter()
        .map(|object| {
            PARTS
                .iter()
                .filter_map(|(part, _)| {
                    object
                        .get(*part)
                        .and_then(serde_json::Value::as_str)
                        .map(|value| ((*part).to_string(), value.to_string()))
                })
                .collect()
        })
        .collect()
}

/// The value of `part` for `row` from the parsed prior answer, or empty.
fn value_of(prior: &[Vec<(String, String)>], row: usize, part: &str) -> String {
    prior
        .get(row)
        .and_then(|r| r.iter().find(|(p, _)| p == part))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// Render `rows` person-row groups, pre-filled from `prior_json`.
#[component]
pub fn PeopleListInputs(prior_json: String, rows: usize) -> Element {
    let prior = prior_rows(&prior_json);
    rsx! {
        for row in 0..rows {
            fieldset { class: "nav-fieldset",
                legend { class: "nav-fieldset__legend",
                    "Person {row + 1}"
                    if row > 0 {
                        span { class: "nav-text-muted", " — leave blank if not applicable" }
                    }
                }
                for (part , label) in PARTS.iter() {
                    div { class: "nav-field",
                        label { class: "nav-label", r#for: "p{row}_{part}", "{label}" }
                        input {
                            class: "nav-input",
                            r#type: "text",
                            id: "p{row}_{part}",
                            name: "p{row}_{part}",
                            value: value_of(&prior, row, part),
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssr(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn renders_one_fieldset_per_row_with_named_inputs() {
        fn app() -> Element {
            rsx! { PeopleListInputs { prior_json: String::new(), rows: 3 } }
        }
        let html = ssr(app);
        assert_eq!(html.matches("<fieldset").count(), 3, "{html}");
        for name in ["p0_name", "p1_street", "p2_zip", "p0_title"] {
            assert!(html.contains(&format!("name=\"{name}\"")), "{name}: {html}");
        }
    }

    #[test]
    fn prefills_from_a_prior_json_answer() {
        fn app() -> Element {
            let prior = r#"[{"name": "Aries Client", "street": "1 Main St", "city": "Las Vegas"},
                            {"name": "Libra Partner"}]"#;
            rsx! { PeopleListInputs { prior_json: prior.to_string(), rows: 3 } }
        }
        let html = ssr(app);
        assert!(html.contains(r#"value="Aries Client""#), "{html}");
        assert!(html.contains(r#"value="1 Main St""#), "{html}");
        assert!(html.contains(r#"value="Libra Partner""#), "{html}");
    }

    #[test]
    fn prefills_values_that_contain_escaped_quotes_and_braces() {
        fn app() -> Element {
            // A name with an escaped quote and a street with braces — both valid
            // JSON. The old hand-rolled scan truncated the name at the first
            // `\"` and mis-split the object on the brace.
            let prior = r#"[{"name": "Ada \"Boss\" Lovelace", "street": "Apt {3}, 1 Main St"}]"#;
            rsx! { PeopleListInputs { prior_json: prior.to_string(), rows: 1 } }
        }
        let html = ssr(app);
        // The full name survives past the escaped quote (a truncating parser
        // drops "Lovelace"), and the braced street is intact.
        assert!(
            html.contains("Lovelace"),
            "escaped quote must not truncate the value: {html}"
        );
        assert!(
            html.contains("Apt {3}, 1 Main St"),
            "braces must not break the object boundary: {html}"
        );
    }

    #[test]
    fn a_garbage_prior_value_prefills_nothing_and_does_not_panic() {
        fn app() -> Element {
            rsx! {
                PeopleListInputs { prior_json: "Jane, John, and the {weird} one".to_string(), rows: 2 }
            }
        }
        assert_eq!(ssr(app).matches("<fieldset").count(), 2);
    }
}
