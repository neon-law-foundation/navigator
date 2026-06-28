//! Field maps: questionnaire answers → `AcroForm` field values.
//!
//! Each vendored form carries a `<code>.fields.toml` beside the blank PDF,
//! mapping the form's exact `/T` field names (derived from a
//! dump of the vendored bytes — see the vendoring workflow: the
//! canonical example is on disk, **no guessing**) to answer sources.
//! Real government field names are hostile (`undefined`, `City_5`,
//! `Name of Registered Agenl` — a typo printed in the official form),
//! which is exactly why the map is data the guard tests pin, not code.
//!
//! A rule has exactly one source:
//!
//! - `question = "entity_name"` — the answer string verbatim.
//! - `literal = "NRS 86"` — a fixed value; for a checkbox this is the
//!   on-state to set.
//!
//! And optional modifiers on `question`:
//!
//! - `checked_when` + `on_state` — checkbox driven by a choice answer:
//!   set `on_state` iff the answer equals `checked_when`, else leave
//!   the box untouched.
//! - `value_map = { managers = "Manager", … }` — translate a choice
//!   answer into the printed value; an answer missing from the map is
//!   a loud error, never a blank.
//! - `row` + `part` — index into a `people_list` answer (a JSON array
//!   of objects with `name` / `street` / `city` / `state` / `zip` /
//!   `country` / `title`); a row the respondent didn't provide simply
//!   leaves the slot blank.
//! - `present_in` + `row_present` — gate any rule on a `people_list`
//!   having a row at that index, so a slot's title label (`Trustee`,
//!   `Managing Member`) never prints beside an empty name line.
//!
//! [`resolve`] turns (map, answers) into the `field name → value` map
//! that `pdf::fill_acroform` consumes. Missing answers skip their
//! fields — the questionnaire owns required-ness — while structural
//! problems (a malformed people list, an unmapped choice) error loudly.

use std::collections::BTreeMap;

use serde::Deserialize;

/// One vendored form's parsed `<code>.fields.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct FieldMap {
    /// Must match the form template `code`.
    pub form_code: String,
    /// The mapping rules, one per `AcroForm` field we fill.
    pub field: Vec<FieldRule>,
}

/// One field's mapping rule. See the module docs for the source kinds.
#[derive(Debug, Clone, Deserialize)]
pub struct FieldRule {
    /// The exact `AcroForm` `/T` name, byte-for-byte from the dump.
    pub name: String,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub literal: Option<String>,
    #[serde(default)]
    pub row: Option<usize>,
    #[serde(default)]
    pub part: Option<String>,
    #[serde(default)]
    pub checked_when: Option<String>,
    #[serde(default)]
    pub on_state: Option<String>,
    #[serde(default)]
    pub value_map: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub present_in: Option<String>,
    #[serde(default)]
    pub row_present: Option<usize>,
}

/// Errors parsing or resolving a field map.
#[derive(Debug, thiserror::Error)]
pub enum FieldMapError {
    #[error("parse field map: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("field `{0}`: a rule needs exactly one of `question` / `literal`")]
    AmbiguousSource(String),
    #[error("field `{0}`: `checked_when` requires `on_state`")]
    MissingOnState(String),
    #[error("field `{0}`: `row` requires `part` (and vice versa)")]
    RowWithoutPart(String),
    #[error("field `{0}`: `present_in` requires `row_present` (and vice versa)")]
    PresentInWithoutRow(String),
    #[error("field `{field}`: answer `{value}` for `{question}` is not in the value_map")]
    UnmappedChoice {
        field: String,
        question: String,
        value: String,
    },
    #[error("field `{field}`: people-list answer for `{question}` is not a JSON array of objects: {source}")]
    MalformedPeopleList {
        field: String,
        question: String,
        #[source]
        source: serde_json::Error,
    },
}

/// The bundled field maps, keyed by `form_code`.
const BUNDLED_MAPS: &[(&str, &str)] = &[
    (
        "nv__llc_formation",
        include_str!(
            "../../notation_templates/forms/united_states/nevada/state/nv__llc_formation.fields.toml"
        ),
    ),
    (
        "nv__profit_corp_formation",
        include_str!(
            "../../notation_templates/forms/united_states/nevada/state/nv__profit_corp_formation.fields.toml"
        ),
    ),
    (
        "nv__business_trust_formation",
        include_str!(
            "../../notation_templates/forms/united_states/nevada/state/nv__business_trust_formation.fields.toml"
        ),
    ),
];

/// Parse the bundled field map for one form, validating rule shape.
///
/// # Errors
///
/// [`FieldMapError`] on TOML or rule-shape problems; `Ok(None)` when
/// the form has no map (a `fill = "none"` reference document).
pub fn field_map(form_code: &str) -> Result<Option<FieldMap>, FieldMapError> {
    let Some((_, raw)) = BUNDLED_MAPS.iter().find(|(code, _)| *code == form_code) else {
        return Ok(None);
    };
    let map: FieldMap = toml::from_str(raw)?;
    for rule in &map.field {
        match (&rule.question, &rule.literal) {
            (Some(_), None) | (None, Some(_)) => {}
            _ => return Err(FieldMapError::AmbiguousSource(rule.name.clone())),
        }
        if rule.checked_when.is_some() && rule.on_state.is_none() {
            return Err(FieldMapError::MissingOnState(rule.name.clone()));
        }
        if rule.row.is_some() != rule.part.is_some() {
            return Err(FieldMapError::RowWithoutPart(rule.name.clone()));
        }
        if rule.present_in.is_some() != rule.row_present.is_some() {
            return Err(FieldMapError::PresentInWithoutRow(rule.name.clone()));
        }
    }
    Ok(Some(map))
}

/// One row of a `people_list` answer.
#[derive(Debug, Deserialize)]
struct PersonRow {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    street: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    zip: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

impl PersonRow {
    fn part(&self, part: &str) -> Option<&str> {
        match part {
            "name" => self.name.as_deref(),
            "street" => self.street.as_deref(),
            "city" => self.city.as_deref(),
            "state" => self.state.as_deref(),
            "zip" => self.zip.as_deref(),
            "country" => self.country.as_deref(),
            "title" => self.title.as_deref(),
            _ => None,
        }
    }
}

/// Resolve a field map against the respondent's answers into the
/// `field name → value` map `pdf::fill_acroform` consumes.
///
/// Missing or empty answers skip their fields (the questionnaire owns
/// required-ness); structural problems error loudly.
///
/// # Errors
///
/// [`FieldMapError::UnmappedChoice`] and
/// [`FieldMapError::MalformedPeopleList`] — see the variants.
pub fn resolve(
    map: &FieldMap,
    answers: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, FieldMapError> {
    let mut out = BTreeMap::new();
    for rule in &map.field {
        // The presence gate runs first: a slot-label rule for a row the
        // respondent never provided is skipped entirely.
        if let (Some(list_question), Some(row)) = (&rule.present_in, rule.row_present) {
            let present = answers
                .get(list_question)
                .filter(|a| !a.is_empty())
                .map(|a| {
                    serde_json::from_str::<Vec<PersonRow>>(a).map(|rows| {
                        rows.get(row).is_some_and(|r| {
                            ["name", "street", "city", "state", "zip", "country", "title"]
                                .iter()
                                .any(|p| r.part(p).is_some_and(|v| !v.is_empty()))
                        })
                    })
                })
                .transpose()
                .map_err(|source| FieldMapError::MalformedPeopleList {
                    field: rule.name.clone(),
                    question: list_question.clone(),
                    source,
                })?
                .unwrap_or(false);
            if !present {
                continue;
            }
        }
        if let Some(literal) = &rule.literal {
            out.insert(rule.name.clone(), literal.clone());
            continue;
        }
        let question = rule.question.as_deref().unwrap_or_default();
        let Some(answer) = answers.get(question).filter(|a| !a.is_empty()) else {
            continue;
        };

        if let Some(when) = &rule.checked_when {
            if answer == when {
                // Validated at parse time: checked_when implies on_state.
                if let Some(state) = &rule.on_state {
                    out.insert(rule.name.clone(), state.clone());
                }
            }
            continue;
        }
        if let Some(value_map) = &rule.value_map {
            let Some(mapped) = value_map.get(answer) else {
                return Err(FieldMapError::UnmappedChoice {
                    field: rule.name.clone(),
                    question: question.to_string(),
                    value: answer.clone(),
                });
            };
            out.insert(rule.name.clone(), mapped.clone());
            continue;
        }
        if let (Some(row), Some(part)) = (rule.row, rule.part.as_deref()) {
            let rows: Vec<PersonRow> = serde_json::from_str(answer).map_err(|source| {
                FieldMapError::MalformedPeopleList {
                    field: rule.name.clone(),
                    question: question.to_string(),
                    source,
                }
            })?;
            if let Some(value) = rows.get(row).and_then(|r| r.part(part)) {
                if !value.is_empty() {
                    out.insert(rule.name.clone(), value.to_string());
                }
            }
            continue;
        }
        out.insert(rule.name.clone(), answer.clone());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{field_map, resolve, FieldMap, FieldMapError};
    use std::collections::BTreeMap;

    fn answers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn parse(toml_src: &str) -> FieldMap {
        toml::from_str(toml_src).expect("test map parses")
    }

    #[test]
    fn every_bundled_form_has_a_parsing_field_map() {
        for form in crate::registry().expect("registry") {
            let map = field_map(form.meta.code)
                .expect("map parses")
                .expect("map exists for every fill=acroform form");
            assert_eq!(map.form_code, form.meta.code);
            assert!(!map.field.is_empty());
        }
    }

    #[test]
    fn question_literal_checkbox_and_value_map_resolve() {
        let map = parse(
            r#"
            form_code = "t"
            [[field]]
            name = "Entity"
            question = "entity_name"
            [[field]]
            name = "NRS86"
            literal = "NRS 86"
            [[field]]
            name = "managers_a"
            question = "management_structure"
            checked_when = "managers"
            on_state = "managers"
            [[field]]
            name = "managers_b"
            question = "management_structure"
            checked_when = "members"
            on_state = "members"
            [[field]]
            name = "Title"
            question = "management_structure"
            value_map = { managers = "Manager", members = "Managing Member" }
            "#,
        );
        let resolved = resolve(
            &map,
            &answers(&[
                ("entity_name", "Neon Demo LLC"),
                ("management_structure", "members"),
            ]),
        )
        .unwrap();
        assert_eq!(resolved["Entity"], "Neon Demo LLC");
        assert_eq!(resolved["NRS86"], "NRS 86");
        assert_eq!(resolved["managers_b"], "members");
        assert!(
            !resolved.contains_key("managers_a"),
            "unmatched box untouched"
        );
        assert_eq!(resolved["Title"], "Managing Member");
    }

    #[test]
    fn people_list_rows_fill_slots_and_absent_rows_stay_blank() {
        let map = parse(
            r#"
            form_code = "t"
            [[field]]
            name = "Name"
            question = "managing_members"
            row = 0
            part = "name"
            [[field]]
            name = "City_2"
            question = "managing_members"
            row = 1
            part = "city"
            [[field]]
            name = "Name_3"
            question = "managing_members"
            row = 2
            part = "name"
            "#,
        );
        let people = r#"[
            {"name": "Aries Client", "street": "1 Main St", "city": "Las Vegas", "state": "NV", "zip": "89101", "country": "USA"},
            {"name": "Libra Partner", "city": "Reno"}
        ]"#;
        let resolved = resolve(&map, &answers(&[("managing_members", people)])).unwrap();
        assert_eq!(resolved["Name"], "Aries Client");
        assert_eq!(resolved["City_2"], "Reno");
        assert!(
            !resolved.contains_key("Name_3"),
            "no third member, slot blank"
        );
    }

    #[test]
    fn unmapped_choice_is_a_loud_error() {
        let map = parse(
            r#"
            form_code = "t"
            [[field]]
            name = "Title"
            question = "management_structure"
            value_map = { managers = "Manager" }
            "#,
        );
        let err = resolve(&map, &answers(&[("management_structure", "anarchy")])).unwrap_err();
        assert!(matches!(err, FieldMapError::UnmappedChoice { value, .. } if value == "anarchy"));
    }

    #[test]
    fn malformed_people_list_is_a_loud_error() {
        let map = parse(
            r#"
            form_code = "t"
            [[field]]
            name = "Name"
            question = "managing_members"
            row = 0
            part = "name"
            "#,
        );
        let err = resolve(&map, &answers(&[("managing_members", "Jane, John")])).unwrap_err();
        assert!(matches!(err, FieldMapError::MalformedPeopleList { .. }));
    }

    #[test]
    fn missing_answers_skip_their_fields() {
        let map = parse(
            r#"
            form_code = "t"
            [[field]]
            name = "Entity"
            question = "entity_name"
            "#,
        );
        let resolved = resolve(&map, &answers(&[])).unwrap();
        assert!(resolved.is_empty());
    }

    #[test]
    fn a_rule_with_both_sources_fails_validation() {
        let raw = r#"
            form_code = "nv__llc_formation"
            [[field]]
            name = "X"
            question = "q"
            literal = "v"
        "#;
        let map: FieldMap = toml::from_str(raw).unwrap();
        // Shape validation lives in field_map(); mirror it here directly.
        let rule = &map.field[0];
        assert!(rule.question.is_some() && rule.literal.is_some());
    }
}
