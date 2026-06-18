//! The `people_list` question widget — a bounded set of person rows
//! (name + mailing address + optional title) for questions like "who
//! are the managing members?".
//!
//! Each vendored government form prints a fixed number of officer /
//! manager / trustee slots, so the widget renders a fixed number of row
//! groups; the respondent leaves trailing rows blank. The inputs are
//! named `p{row}_{part}` and the POST handler assembles them into one
//! JSON-array answer (see `web::people_list_answer`), so the answer
//! stays a single `answers.value` string like every other question.

use maud::{html, Markup};

/// The row parts, in render order: input-name suffix + visible label.
/// Must stay in lock-step with `forms::fieldmap`'s `PersonRow` parts.
pub const PARTS: [(&str, &str); 7] = [
    ("name", "Full legal name"),
    ("title", "Title (officers only)"),
    ("street", "Street address"),
    ("city", "City"),
    ("state", "State"),
    ("zip", "ZIP / postal code"),
    ("country", "Country"),
];

/// Parse a prior people-list answer (a JSON array of objects) into
/// per-row part values without a JSON dependency: scans for the known
/// part keys inside each `{...}` object. Tolerant by design — a prior
/// value that doesn't parse just pre-fills nothing.
fn prior_rows(prior_json: &str) -> Vec<Vec<(String, String)>> {
    let mut rows = Vec::new();
    for object in prior_json.split('{').skip(1) {
        let Some(body) = object.split('}').next() else {
            continue;
        };
        let mut row = Vec::new();
        for (part, _) in PARTS {
            let needle = format!("\"{part}\"");
            let Some(at) = body.find(&needle) else {
                continue;
            };
            let rest = &body[at + needle.len()..];
            let Some(open) = rest.find('"') else { continue };
            let Some(close) = rest[open + 1..].find('"') else {
                continue;
            };
            row.push((
                part.to_string(),
                rest[open + 1..open + 1 + close].to_string(),
            ));
        }
        rows.push(row);
    }
    rows
}

/// Render `rows` person-row groups, pre-filled from `prior_json`.
/// Rendered inside a `FormCard` via [`super::form::FormCard`]'s
/// `extra_fields`, so the inputs post with the rest of the form.
#[must_use]
pub fn people_list_inputs(prior_json: &str, rows: usize) -> Markup {
    let prior = prior_rows(prior_json);
    let value_of = |row: usize, part: &str| -> String {
        prior
            .get(row)
            .and_then(|r| r.iter().find(|(p, _)| p == part))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    html! {
        @for row in 0..rows {
            fieldset."border"."rounded"."p-3"."mb-3" {
                legend."h6"."w-auto"."px-2" {
                    "Person " (row + 1)
                    @if row > 0 { span."text-body-secondary" { " — leave blank if not applicable" } }
                }
                @for (part, label) in PARTS {
                    @let name = format!("p{row}_{part}");
                    div."mb-3" {
                        label."form-label" for=(name) { (label) }
                        input."form-control" type="text" id=(name) name=(name)
                            value=(value_of(row, part));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::people_list_inputs;

    #[test]
    fn renders_one_fieldset_per_row_with_named_inputs() {
        let html = people_list_inputs("", 3).into_string();
        assert_eq!(html.matches("<fieldset").count(), 3, "{html}");
        for name in ["p0_name", "p1_street", "p2_zip", "p0_title"] {
            assert!(html.contains(&format!("name=\"{name}\"")), "{name}: {html}");
        }
    }

    #[test]
    fn prefills_from_a_prior_json_answer() {
        let prior = r#"[{"name": "Aries Client", "street": "1 Main St", "city": "Las Vegas"},
                        {"name": "Libra Partner"}]"#;
        let html = people_list_inputs(prior, 3).into_string();
        assert!(html.contains("value=\"Aries Client\""), "{html}");
        assert!(html.contains("value=\"1 Main St\""), "{html}");
        assert!(html.contains("value=\"Libra Partner\""), "{html}");
    }

    #[test]
    fn a_garbage_prior_value_prefills_nothing_and_does_not_panic() {
        let html = people_list_inputs("Jane, John, and the {weird} one", 2).into_string();
        assert_eq!(html.matches("<fieldset").count(), 2);
    }
}
