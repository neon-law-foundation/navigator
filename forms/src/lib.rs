//! Vendored government forms — the bundled registry behind
//! `templates/forms/`.
//!
//! Each canonical blank PDF lives under the same path it uses in the
//! public assets bucket: `templates/<object_path>`. The sibling
//! markdown template is the catalog card, and this crate embeds the PDF
//! bytes so runtime form filling never depends on a network read.

pub mod fieldmap;

pub use fieldmap::{field_map, resolve, FieldMap, FieldMapError, FieldRule};

/// Metadata for one vendored government form.
#[derive(Debug, Clone)]
pub struct FormMeta {
    /// Stable form/template code. For forms, this is jurisdiction-first:
    /// `nv__llc_formation`, `us__form_990`, etc.
    pub code: &'static str,
    /// Jurisdiction code from `store/seeds/Jurisdiction.yaml`.
    pub jurisdiction: &'static str,
    /// Human title from the sibling markdown template.
    pub title: &'static str,
    /// Canonical government page where the blank can be obtained.
    pub origin_url: &'static str,
    /// Path in the public assets bucket and, with `templates/`
    /// prepended, in the repo.
    pub object_path: &'static str,
}

impl FormMeta {
    /// Compatibility accessor while callers migrate from the old
    /// `form_code` vocabulary to plain `code`.
    #[must_use]
    pub fn form_code(&self) -> &'static str {
        self.code
    }
}

/// One vendored form: metadata plus bundled PDF bytes.
#[derive(Debug, Clone)]
pub struct Form {
    pub meta: FormMeta,
    pub bytes: &'static [u8],
}

/// Errors loading the bundled registry.
#[derive(Debug, thiserror::Error)]
pub enum FormsError {
    #[error("forms registry unavailable")]
    Unavailable,
}

const NV_SOS_FORMS_URL: &str =
    "https://www.nvsos.gov/businesses/commercial-recordings/forms-fees/all-business-forms";

const BUNDLED: &[Form] = &[
    Form {
        meta: FormMeta {
            code: "nv__llc_formation",
            jurisdiction: "NV",
            title: "Nevada LLC Formation",
            origin_url: NV_SOS_FORMS_URL,
            object_path: "forms/united_states/nevada/state/nv__llc_formation.pdf",
        },
        bytes: include_bytes!(
            "../../templates/forms/united_states/nevada/state/nv__llc_formation.pdf"
        ),
    },
    Form {
        meta: FormMeta {
            code: "nv__profit_corp_formation",
            jurisdiction: "NV",
            title: "Nevada Profit Corporation Formation",
            origin_url: NV_SOS_FORMS_URL,
            object_path: "forms/united_states/nevada/state/nv__profit_corp_formation.pdf",
        },
        bytes: include_bytes!(
            "../../templates/forms/united_states/nevada/state/nv__profit_corp_formation.pdf"
        ),
    },
    Form {
        meta: FormMeta {
            code: "nv__business_trust_formation",
            jurisdiction: "NV",
            title: "Nevada Business Trust Formation",
            origin_url: NV_SOS_FORMS_URL,
            object_path: "forms/united_states/nevada/state/nv__business_trust_formation.pdf",
        },
        bytes: include_bytes!(
            "../../templates/forms/united_states/nevada/state/nv__business_trust_formation.pdf"
        ),
    },
];

/// Return the bundled form registry.
///
/// # Errors
///
/// This currently cannot fail; the `Result` keeps the public seam stable
/// for callers that already propagate registry errors.
pub fn registry() -> Result<Vec<Form>, FormsError> {
    Ok(BUNDLED.to_vec())
}

/// Look up one vendored form by its stable `code`.
///
/// # Errors
///
/// Propagates [`registry`] errors; `Ok(None)` when the code is unknown.
pub fn get(code: &str) -> Result<Option<Form>, FormsError> {
    Ok(registry()?.into_iter().find(|f| f.meta.code == code))
}

#[cfg(test)]
mod tests {
    use super::{get, registry};

    #[test]
    fn registry_embeds_every_pdf() {
        let forms = registry().expect("registry loads");
        assert_eq!(forms.len(), 3);
        for form in &forms {
            assert!(
                form.bytes.starts_with(b"%PDF"),
                "{} bundled bytes are not a PDF",
                form.meta.code
            );
            assert!(std::path::Path::new(form.meta.object_path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")));
            assert!(form.meta.object_path.starts_with("forms/united_states/"));
        }
    }

    #[test]
    fn get_finds_known_and_misses_unknown() {
        assert!(get("nv__llc_formation").expect("registry loads").is_some());
        assert!(get("nv__annual_list").expect("registry loads").is_none());
    }
}
