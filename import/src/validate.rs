//! Structural validation — pure, no database. Produces diagnostics the
//! same way `rules` lints a source file, so this can run in a CLI, an
//! MCP tool, or (later) an LSP without a connection. Cross-reference,
//! shape, and canonicality are checked here; existence of a referenced
//! `entity_type` or `jurisdiction` is a database fact and surfaces at
//! [`crate::apply`] time as a per-row failure instead.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde::Serialize;

use crate::contract::{Payload, SUPPORTED_VERSION};

/// How serious a [`Diagnostic`] is. Any `Error` blocks the whole apply
/// (nothing is written); `Warning` is informational (e.g. a URL that
/// was accepted but rewritten to its canonical form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// One structural problem found in a [`Payload`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Location in the payload, e.g. `"people[2].email"`.
    pub pointer: String,
    pub message: String,
}

impl Diagnostic {
    fn error(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            pointer: pointer.into(),
            message: message.into(),
        }
    }

    fn warning(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            pointer: pointer.into(),
            message: message.into(),
        }
    }
}

/// Validate a payload's structure and cross-references. Returns every
/// problem found (it does not stop at the first). An empty result — or
/// one with only warnings — means [`crate::apply`] may proceed.
#[must_use]
pub fn validate(payload: &Payload) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    if payload.version != SUPPORTED_VERSION {
        out.push(Diagnostic::error(
            "version",
            format!(
                "unsupported contract version {} (this engine speaks {SUPPORTED_VERSION})",
                payload.version
            ),
        ));
    }

    let mut org_keys: BTreeSet<&str> = BTreeSet::new();
    for (i, org) in payload.organizations.iter().enumerate() {
        let at = |field: &str| format!("organizations[{i}].{field}");
        if org.key.trim().is_empty() {
            out.push(Diagnostic::error(at("key"), "key must not be empty"));
        } else if !org_keys.insert(org.key.as_str()) {
            out.push(Diagnostic::error(
                at("key"),
                format!("duplicate organization key `{}`", org.key),
            ));
        }
        if org.name.trim().is_empty() {
            out.push(Diagnostic::error(at("name"), "name must not be empty"));
        }
        if org.entity_type.trim().is_empty() {
            out.push(Diagnostic::error(
                at("entity_type"),
                "entity_type must not be empty",
            ));
        }
        if !is_jurisdiction_code(&org.jurisdiction) {
            out.push(Diagnostic::error(
                at("jurisdiction"),
                format!(
                    "jurisdiction `{}` is not a 2-letter code (e.g. `WA`)",
                    org.jurisdiction
                ),
            ));
        }
        if let Some(url) = &org.url {
            match canonical_url(url) {
                Ok(canonical) if canonical != url.trim() => out.push(Diagnostic::warning(
                    at("url"),
                    format!("url canonicalized to `{canonical}`"),
                )),
                Ok(_) => {}
                Err(why) => out.push(Diagnostic::error(at("url"), why)),
            }
        }
    }

    let mut person_keys: BTreeSet<&str> = BTreeSet::new();
    let mut emails: BTreeSet<String> = BTreeSet::new();
    for (i, person) in payload.people.iter().enumerate() {
        let at = |field: &str| format!("people[{i}].{field}");
        if person.key.trim().is_empty() {
            out.push(Diagnostic::error(at("key"), "key must not be empty"));
        } else if !person_keys.insert(person.key.as_str()) {
            out.push(Diagnostic::error(
                at("key"),
                format!("duplicate person key `{}`", person.key),
            ));
        }
        if person.name.trim().is_empty() {
            out.push(Diagnostic::error(at("name"), "name must not be empty"));
        }
        let email = person.email.trim().to_ascii_lowercase();
        if !is_email_shaped(&email) {
            out.push(Diagnostic::error(
                at("email"),
                format!("`{}` is not a valid email address", person.email),
            ));
        } else if !emails.insert(email) {
            out.push(Diagnostic::error(
                at("email"),
                format!("duplicate email `{}` within this payload", person.email),
            ));
        }
        if person.entity_role.trim().is_empty() {
            out.push(Diagnostic::error(
                at("entity_role"),
                "entity_role must not be empty",
            ));
        }
        if person.organization.trim().is_empty() {
            out.push(Diagnostic::error(
                at("organization"),
                "organization must reference an organization key",
            ));
        } else if !org_keys.contains(person.organization.as_str()) {
            out.push(Diagnostic::error(
                at("organization"),
                format!(
                    "organization `{}` is not defined in this payload's organizations",
                    person.organization
                ),
            ));
        }
    }

    out
}

/// Canonicalize a URL: require an `http(s)` scheme, upgrade `http` to
/// `https`, lowercase the host, and drop the query, fragment, and any
/// trailing slash. Returns the canonical string, or a human reason the
/// input could not be canonicalized.
///
/// `https://Example.org/?utm=x` → `https://example.org`.
pub fn canonical_url(input: &str) -> Result<String, String> {
    let parsed = url::Url::parse(input.trim()).map_err(|e| format!("not a valid URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("url scheme must be http or https, got `{other}`")),
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "url has no host".to_string())?;

    let mut canonical = format!("https://{host}");
    if let Some(port) = parsed.port() {
        let _ = write!(canonical, ":{port}");
    }
    canonical.push_str(parsed.path().trim_end_matches('/'));
    Ok(canonical)
}

/// Two ASCII letters — the shape of a US state / DC / territory code.
/// Existence is checked against the `jurisdictions` table at apply time.
fn is_jurisdiction_code(code: &str) -> bool {
    let code = code.trim();
    code.len() == 2 && code.bytes().all(|b| b.is_ascii_alphabetic())
}

/// A deliberately loose check: one `@`, a non-empty local part, and a
/// domain containing a dot. The database's unique constraint is the
/// real gate; this just catches obvious garbage early.
fn is_email_shaped(email: &str) -> bool {
    let mut parts = email.splitn(2, '@');
    let (Some(local), Some(domain)) = (parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}
