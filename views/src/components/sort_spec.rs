//! Sortable-column state shared by every admin list page.
//!
//! Mirrors the JSON:API 1.1 `sort` query-parameter contract — fields
//! are comma-separated, a leading `-` flips a field to descending,
//! and the server MUST `400` when asked to sort by a key it does
//! not advertise. [`SortSpec::validated`] enforces that for us.
//!
//! Parsing + toggling + encoding round-trip; render-side tests live
//! alongside [`super::data_table`].

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    /// Unicode glyph rendered in sortable header cells to indicate
    /// the active direction. BMP characters so we ship without entity
    /// escaping.
    #[must_use]
    pub const fn arrow(self) -> &'static str {
        match self {
            Self::Ascending => "\u{2191}",
            Self::Descending => "\u{2193}",
        }
    }

    #[must_use]
    pub const fn flipped(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortField {
    pub key: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SortSpec {
    pub fields: Vec<SortField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortError {
    UnsupportedField(String),
}

impl std::fmt::Display for SortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedField(k) => write!(f, "unsupported sort field: {k}"),
        }
    }
}

impl std::error::Error for SortError {}

impl SortSpec {
    #[must_use]
    pub fn single(key: impl Into<String>, direction: SortDirection) -> Self {
        Self {
            fields: vec![SortField {
                key: key.into(),
                direction,
            }],
        }
    }

    /// Parse a raw `?sort=` value. Empty / whitespace fields drop;
    /// leading `-` flips to descending. Round-trips with [`Self::encoded`].
    #[must_use]
    pub fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw else {
            return Self::default();
        };
        let fields = raw
            .split(',')
            .filter_map(|part| {
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    return None;
                }
                if let Some(key) = trimmed.strip_prefix('-') {
                    if key.is_empty() {
                        None
                    } else {
                        Some(SortField {
                            key: key.to_string(),
                            direction: SortDirection::Descending,
                        })
                    }
                } else {
                    Some(SortField {
                        key: trimmed.to_string(),
                        direction: SortDirection::Ascending,
                    })
                }
            })
            .collect();
        Self { fields }
    }

    /// Re-encode the spec into a JSON:API `sort=` value. Empty spec
    /// returns an empty string.
    #[must_use]
    pub fn encoded(&self) -> String {
        self.fields
            .iter()
            .map(|f| match f.direction {
                SortDirection::Descending => format!("-{}", f.key),
                SortDirection::Ascending => f.key.clone(),
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Direction this spec sorts `key` in, if at all.
    #[must_use]
    pub fn direction_for(&self, key: &str) -> Option<SortDirection> {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.direction)
    }

    /// Spec produced by clicking the header for `key`. Toggling a
    /// non-primary key resets to a single-field ascending sort —
    /// multi-field shift-click sorting is intentionally not in scope.
    #[must_use]
    pub fn toggling(&self, key: &str) -> Self {
        let direction = match self.direction_for(key) {
            Some(d) => d.flipped(),
            None => SortDirection::Ascending,
        };
        Self::single(key, direction)
    }

    /// Reject any field whose key is not in `allowed_keys`. Route
    /// handlers should turn the error into a `400 Bad Request` per
    /// JSON:API 1.1.
    pub fn validated(self, allowed_keys: &HashSet<&str>) -> Result<Self, SortError> {
        for f in &self.fields {
            if !allowed_keys.contains(f.key.as_str()) {
                return Err(SortError::UnsupportedField(f.key.clone()));
            }
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{SortDirection, SortError, SortSpec};
    use std::collections::HashSet;

    #[test]
    fn parse_none_yields_empty_spec() {
        let s = SortSpec::parse(None);
        assert!(s.fields.is_empty());
    }

    #[test]
    fn parse_empty_string_yields_empty_spec() {
        let s = SortSpec::parse(Some(""));
        assert!(s.fields.is_empty());
    }

    #[test]
    fn parse_single_ascending_field() {
        let s = SortSpec::parse(Some("name"));
        assert_eq!(s.fields.len(), 1);
        assert_eq!(s.fields[0].key, "name");
        assert_eq!(s.fields[0].direction, SortDirection::Ascending);
    }

    #[test]
    fn parse_leading_dash_marks_descending() {
        let s = SortSpec::parse(Some("-created_at"));
        assert_eq!(s.fields[0].key, "created_at");
        assert_eq!(s.fields[0].direction, SortDirection::Descending);
    }

    #[test]
    fn parse_drops_whitespace_only_and_lone_dash_fields() {
        let s = SortSpec::parse(Some("name, ,-,-created_at"));
        let keys: Vec<&str> = s.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, vec!["name", "created_at"]);
    }

    #[test]
    fn parse_then_encoded_round_trips() {
        let raw = "name,-created_at,email";
        assert_eq!(SortSpec::parse(Some(raw)).encoded(), raw);
    }

    #[test]
    fn encoded_empty_spec_is_empty_string() {
        assert_eq!(SortSpec::default().encoded(), "");
    }

    #[test]
    fn direction_for_returns_none_for_missing_key() {
        let s = SortSpec::parse(Some("name"));
        assert!(s.direction_for("email").is_none());
    }

    #[test]
    fn toggling_unknown_key_yields_single_ascending() {
        let s = SortSpec::default().toggling("name");
        assert_eq!(s.encoded(), "name");
    }

    #[test]
    fn toggling_active_ascending_flips_to_descending() {
        let s = SortSpec::single("name", SortDirection::Ascending).toggling("name");
        assert_eq!(s.encoded(), "-name");
    }

    #[test]
    fn toggling_active_descending_flips_to_ascending() {
        let s = SortSpec::single("name", SortDirection::Descending).toggling("name");
        assert_eq!(s.encoded(), "name");
    }

    #[test]
    fn toggling_a_different_key_collapses_to_single_field_ascending() {
        let s = SortSpec::single("name", SortDirection::Descending).toggling("email");
        assert_eq!(s.encoded(), "email");
    }

    #[test]
    fn validated_accepts_known_keys() {
        let allowed: HashSet<&str> = ["name", "email"].into_iter().collect();
        let s = SortSpec::parse(Some("name,-email"))
            .validated(&allowed)
            .expect("known keys validate");
        assert_eq!(s.fields.len(), 2);
    }

    #[test]
    fn validated_rejects_first_unknown_key() {
        let allowed: HashSet<&str> = ["name"].into_iter().collect();
        let err = SortSpec::parse(Some("name,wat,email"))
            .validated(&allowed)
            .expect_err("unknown key rejected");
        assert_eq!(err, SortError::UnsupportedField("wat".into()));
    }

    #[test]
    fn arrow_glyphs_pick_bmp_unicode() {
        assert_eq!(SortDirection::Ascending.arrow(), "\u{2191}");
        assert_eq!(SortDirection::Descending.arrow(), "\u{2193}");
    }
}
