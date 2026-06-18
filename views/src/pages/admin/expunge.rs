//! `/portal/admin/documents/:doc_id/expunge` — the admin-only
//! governed-expunge surface (design §9; git-repos surfaces Task 1).
//!
//! Two screens: a [`confirm`] form that names the document and demands a
//! category + optional note before the irreversible act, and a
//! [`result`] page that shows the resulting audit-row id. Expunge
//! rewrites the matter repo's history — the confirmation says so
//! plainly, because it invalidates every existing clone.

use maud::{html, Markup};
use uuid::Uuid;

use crate::PageLayout;

/// The expunge categories, mirrored from
/// `store::entity::expunge_record::CATEGORY_*`. Kept as a local table so
/// the view crate stays free of a `store` dependency; the route handler
/// re-validates the posted value against the canonical constants.
pub const CATEGORIES: [(&str, &str); 3] = [
    (
        "privilege",
        "Privilege clawback — privileged material committed in error",
    ),
    ("sealing", "Court sealing order"),
    ("client_request", "Client lawful-deletion request"),
];

/// Inputs to the [`confirm`] screen.
pub struct Confirm<'a> {
    pub doc_id: Uuid,
    pub project_id: Uuid,
    /// The repo path that will be removed from all history — the
    /// document's filename.
    pub filename: &'a str,
    /// The object-storage key whose bytes will be deleted.
    pub storage_key: &'a str,
    pub csrf_token: &'a str,
    /// Set when a prior submit was rejected (e.g. an unknown category).
    pub error: Option<&'a str>,
}

#[must_use]
pub fn confirm(c: &Confirm<'_>) -> Markup {
    let action = format!("/portal/admin/documents/{}/expunge", c.doc_id);
    let project_href = format!("/portal/projects/{}", c.project_id);
    let body = html! {
        section.admin { div.container {
            header.page-header {
                h1 { "Expunge document" }
                p.muted {
                    "Project: " a href=(project_href) { (c.project_id) }
                }
            }

            div.alert.alert-danger role="alert" {
                strong { "This rewrites the matter's history and cannot be undone." }
                " The document is removed from every commit, its stored bytes are deleted, "
                "and existing clones of the repository become invalid. Only the audit record "
                "of the expunge — who, when, and why — is kept."
            }

            @if let Some(e) = c.error {
                p.error role="alert" { (e) }
            }

            dl.admin-detail {
                dt { "Document" } dd.mono { (c.filename) }
                dt { "Stored at" } dd.mono { (c.storage_key) }
            }

            form method="post" action=(action) {
                input type="hidden" name="_csrf" value=(c.csrf_token);

                p {
                    label for="category" { "Reason (required)" }
                    br;
                    select id="category" name="category" required {
                        option value="" disabled selected { "Choose a category…" }
                        @for (value, label) in CATEGORIES {
                            option value=(value) { (label) }
                        }
                    }
                }

                p {
                    label for="note" { "Note (optional — a docket reference, not document content)" }
                    br;
                    textarea id="note" name="note" rows="2" cols="60" {}
                }

                p {
                    button.btn.btn-danger type="submit" { "Expunge permanently" }
                    " "
                    a.btn.btn-secondary href=(project_href) { "Cancel" }
                }
            }
        } }
    };
    PageLayout::new("Expunge document — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// Inputs to the [`result`] screen shown after a completed expunge.
pub struct Result<'a> {
    pub record_id: Uuid,
    pub project_id: Uuid,
    pub filename: &'a str,
    pub category: &'a str,
}

#[must_use]
pub fn result(r: &Result<'_>) -> Markup {
    let project_href = format!("/portal/projects/{}", r.project_id);
    let body = html! {
        section.admin { div.container {
            header.page-header {
                h1 { "Document expunged" }
            }
            p {
                strong.mono { (r.filename) }
                " has been removed from the matter's history and storage."
            }
            dl.admin-detail {
                dt { "Audit record" } dd.mono { (r.record_id) }
                dt { "Category" } dd { (r.category) }
            }
            p { a href=(project_href) { "Back to the matter" } }
        } }
    };
    PageLayout::new("Document expunged — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{confirm, result, Confirm, Result};
    use uuid::Uuid;

    const DOC: Uuid = Uuid::from_u128(7);
    const PROJ: Uuid = Uuid::from_u128(9);

    #[test]
    fn confirm_names_the_doc_warns_and_lists_every_category() {
        let html = confirm(&Confirm {
            doc_id: DOC,
            project_id: PROJ,
            filename: "privileged.pdf",
            storage_key: "blobs/deadbeef",
            csrf_token: "tok",
            error: None,
        })
        .into_string();
        assert!(html.contains("privileged.pdf"));
        assert!(html.contains("blobs/deadbeef"));
        assert!(html.contains("rewrites the matter's history"));
        assert!(html.contains("existing clones"));
        assert!(html.contains(&format!("action=\"/portal/admin/documents/{DOC}/expunge\"")));
        assert!(html.contains("name=\"_csrf\" value=\"tok\""));
        // every category option is offered
        assert!(html.contains("value=\"privilege\""));
        assert!(html.contains("value=\"sealing\""));
        assert!(html.contains("value=\"client_request\""));
    }

    #[test]
    fn confirm_renders_an_error_when_set() {
        let html = confirm(&Confirm {
            doc_id: DOC,
            project_id: PROJ,
            filename: "x.pdf",
            storage_key: "blobs/x",
            csrf_token: "tok",
            error: Some("Unknown category."),
        })
        .into_string();
        assert!(html.contains("Unknown category."));
    }

    #[test]
    fn result_shows_the_audit_row_id() {
        let rid = Uuid::from_u128(42);
        let html = result(&Result {
            record_id: rid,
            project_id: PROJ,
            filename: "privileged.pdf",
            category: "sealing",
        })
        .into_string();
        assert!(html.contains(&rid.to_string()));
        assert!(html.contains("privileged.pdf"));
        assert!(html.contains("sealing"));
    }
}
