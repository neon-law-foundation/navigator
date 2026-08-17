//! Field maps: the recorded judgment `navigator forms re-author` consumes.
//!
//! A hostile government blank is re-authored once: a `<code>.fields.toml`
//! map records how each exact `/T` field name (from a dump of the pinned
//! blank — `navigator forms fields <code>` prints them, **no guessing**)
//! becomes a canonical questionnaire state path, and [`reauthor::plan`]
//! turns that map plus the blank's own field names into the field-layer
//! transformation. Real government field names are hostile (`undefined`,
//! `City_5`, `Name of Registered Agenl` — a typo printed in the official
//! form), which is exactly why the map is data the guard tests pin, not
//! code. Once a form is re-authored its `/T` names *are* the data paths
//! (the tracked `.fields` manifest), the map is deleted, and fills need
//! no map at all — see [`reauthor::resolve_reauthored`].
//!
//! A rule has exactly one source:
//!
//! - `question = "entity_name"` — the answer string verbatim.
//! - `literal = "NRS 86"` — a fixed value; for a checkbox this is the
//!   on-state to set.
//!
//! And optional modifiers on `question`:
//!
//! - `checked_when` + `on_state` — a checkbox driven by a choice answer;
//!   the pair merges into one radio per canonical state.
//! - `value_map = { managers = "Manager", … }` — translate a choice
//!   answer into the printed value.
//! - `row` + `part` — index into a `people_list` answer (a JSON array
//!   of objects with `name` / `street` / `city` / `state` / `zip` /
//!   `country` / `title`).
//! - `present_in` + `row_present` — gate a slot's title label on a
//!   `people_list` having a row at that index.
//!
//! [`parse_field_map`] parses and shape-validates the map; the re-author
//! transform refuses the rule shapes (`value_map`, `present_in`) that
//! cannot become a bare `/T` name.

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

/// Errors parsing a field map or resolving a re-authored manifest.
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
    #[error("field `{field}`: people-list answer for `{question}` is not a JSON array of objects: {source}")]
    MalformedPeopleList {
        field: String,
        question: String,
        #[source]
        source: serde_json::Error,
    },
}

/// Parse a form's `<code>.fields.toml` and validate every rule's shape —
/// the input `navigator forms re-author` transforms into the blank's
/// `.fields` manifest.
///
/// # Errors
///
/// [`FieldMapError`] on a TOML or rule-shape problem.
pub fn parse_field_map(raw: &str) -> Result<FieldMap, FieldMapError> {
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
    Ok(map)
}

/// One row of a `people_list` answer.
#[derive(Debug, Deserialize)]
pub(crate) struct PersonRow {
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
    pub(crate) fn part(&self, part: &str) -> Option<&str> {
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

#[cfg(test)]
mod tests {
    use super::{parse_field_map, FieldMapError};

    #[test]
    fn every_form_fills_through_a_reauthored_manifest() {
        for form in crate::registry().expect("registry") {
            let manifest = crate::reauthor::manifest(form.code)
                .unwrap_or_else(|| panic!("{}: no .fields manifest", form.code));
            assert!(!manifest.is_empty());
        }
    }

    #[test]
    fn a_well_formed_map_parses_and_validates() {
        let map = parse_field_map(
            r#"
            form_code = "t"
            [[field]]
            name = "Entity"
            question = "entity__company.name"
            [[field]]
            name = "NRS86"
            literal = "NRS 86"
            [[field]]
            name = "managers_a"
            question = "management_structure"
            checked_when = "managers"
            on_state = "managers"
            [[field]]
            name = "Name"
            question = "managing_members"
            row = 0
            part = "name"
            "#,
        )
        .expect("map validates");
        assert_eq!(map.form_code, "t");
        assert_eq!(map.field.len(), 4);
    }

    #[test]
    fn a_rule_with_both_sources_fails_validation() {
        let err = parse_field_map(
            r#"
            form_code = "t"
            [[field]]
            name = "X"
            question = "q"
            literal = "v"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, FieldMapError::AmbiguousSource(name) if name == "X"));
    }

    #[test]
    fn checked_when_without_on_state_fails_validation() {
        let err = parse_field_map(
            r#"
            form_code = "t"
            [[field]]
            name = "X"
            question = "q"
            checked_when = "managers"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, FieldMapError::MissingOnState(name) if name == "X"));
    }

    #[test]
    fn row_without_part_fails_validation() {
        let err = parse_field_map(
            r#"
            form_code = "t"
            [[field]]
            name = "X"
            question = "q"
            row = 0
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, FieldMapError::RowWithoutPart(name) if name == "X"));
    }

    #[test]
    fn present_in_without_row_present_fails_validation() {
        let err = parse_field_map(
            r#"
            form_code = "t"
            [[field]]
            name = "X"
            literal = "Trustee"
            present_in = "people__trustees"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, FieldMapError::PresentInWithoutRow(name) if name == "X"));
    }
}
