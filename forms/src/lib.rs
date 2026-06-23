//! Vendored government forms — the bundled registry behind
//! `notation_templates/forms/`.
//!
//! Every official form we fill and file is vendored from its canonical
//! source (the issuing authority's own domain) and pinned in
//! `notation_templates/forms/FORMS.toml` by printed revision and SHA-256 — see the
//! `vendor-gov-forms` skill for the acquisition discipline. This crate
//! bundles the ledger and the PDF bytes into the binary (`include_str!` /
//! `include_bytes!`) so every consumer — the walker building an
//! [`Acroform` document payload], the web download routes, the `cli forms
//! sync` uploader, the guard tests — reads the same bytes the repo
//! committed, with no network or bucket dependency.
//!
//! [`Acroform` document payload]: https://docs.rs/workflows
//!
//! The guard test (`forms/tests/vendored_forms.rs`) recomputes each
//! `sha256` from the bundled bytes and cross-checks the on-disk file, so
//! ledger, bundle, and working tree cannot drift apart silently.

pub mod fieldmap;

pub use fieldmap::{field_map, resolve, FieldMap, FieldMapError, FieldRule};

use serde::Deserialize;

/// The parsed `FORMS.toml` ledger entry for one vendored form revision.
#[derive(Debug, Clone, Deserialize)]
pub struct FormMeta {
    /// The issuing government office, e.g. `Nevada Secretary of State`.
    pub authority: String,
    /// Human label, as the authority titles the form.
    pub name: String,
    /// Stable id; templates and field maps reference this.
    pub form_code: String,
    /// The revision date printed on the form itself (newest page).
    pub revision: String,
    /// The canonical page the bytes were downloaded from.
    pub source_url: String,
    /// The day we pulled the bytes (ISO date).
    pub retrieved: String,
    /// SHA-256 of the PDF bytes, enforced by the guard test.
    pub sha256: String,
    /// How the form is filled: `acroform` | `overlay` | `none`.
    pub fill: String,
    /// Path in the assets bucket; the repo path is `notation_templates/` + this.
    pub object_path: String,
    /// Acquisition caveats, if any.
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Ledger {
    form: Vec<FormMeta>,
}

/// One vendored form: its ledger entry plus the bundled PDF bytes.
#[derive(Debug, Clone)]
pub struct Form {
    pub meta: FormMeta,
    pub bytes: &'static [u8],
}

/// Errors loading the bundled registry.
#[derive(Debug, thiserror::Error)]
pub enum FormsError {
    #[error("parse FORMS.toml: {0}")]
    Ledger(#[from] toml::de::Error),
    #[error("ledger entry `{0}` has no bundled bytes — add it to BUNDLED")]
    MissingBytes(String),
    #[error("bundled bytes for `{0}` have no ledger entry — add it to FORMS.toml")]
    MissingLedgerEntry(String),
}

const LEDGER_TOML: &str = include_str!("../../notation_templates/forms/FORMS.toml");

/// The bundled PDF bytes, keyed by `form_code`. One row per ledger entry;
/// the guard test fails if the two ever disagree.
const BUNDLED: &[(&str, &[u8])] = &[
    (
        "nv_sos__llc_formation",
        include_bytes!("../../notation_templates/forms/nv_sos/nv_sos__llc_formation-2023-08.pdf"),
    ),
    (
        "nv_sos__profit_corp_formation",
        include_bytes!(
            "../../notation_templates/forms/nv_sos/nv_sos__profit_corp_formation-2024-05.pdf"
        ),
    ),
    (
        "nv_sos__business_trust_formation",
        include_bytes!(
            "../../notation_templates/forms/nv_sos/nv_sos__business_trust_formation-2023-08.pdf"
        ),
    ),
];

/// Parse the bundled ledger and join each entry to its bundled bytes.
///
/// # Errors
///
/// [`FormsError`] when the ledger fails to parse or the ledger and
/// [`BUNDLED`] disagree in either direction — a vendoring half-done.
pub fn registry() -> Result<Vec<Form>, FormsError> {
    let ledger: Ledger = toml::from_str(LEDGER_TOML)?;
    let mut forms = Vec::with_capacity(ledger.form.len());
    for meta in ledger.form {
        let bytes = BUNDLED
            .iter()
            .find(|(code, _)| *code == meta.form_code)
            .map(|(_, bytes)| *bytes)
            .ok_or_else(|| FormsError::MissingBytes(meta.form_code.clone()))?;
        forms.push(Form { meta, bytes });
    }
    for (code, _) in BUNDLED {
        if !forms.iter().any(|f| f.meta.form_code == *code) {
            return Err(FormsError::MissingLedgerEntry((*code).to_string()));
        }
    }
    Ok(forms)
}

/// Look up one vendored form by its stable `form_code`.
///
/// # Errors
///
/// Propagates [`registry`] errors; `Ok(None)` when the code is unknown.
pub fn get(form_code: &str) -> Result<Option<Form>, FormsError> {
    Ok(registry()?
        .into_iter()
        .find(|f| f.meta.form_code == form_code))
}

#[cfg(test)]
mod tests {
    use super::{get, registry};

    #[test]
    fn registry_joins_every_ledger_entry_to_bytes() {
        let forms = registry().expect("ledger parses and joins");
        assert_eq!(forms.len(), 3);
        for form in &forms {
            assert!(
                form.bytes.starts_with(b"%PDF"),
                "{} bundled bytes are not a PDF",
                form.meta.form_code
            );
        }
    }

    #[test]
    fn get_finds_known_and_misses_unknown() {
        assert!(get("nv_sos__llc_formation")
            .expect("registry loads")
            .is_some());
        assert!(get("nv_sos__annual_list")
            .expect("registry loads")
            .is_none());
    }
}
