//! `/portal/admin/expunge-requests` — the staff/admin review queue of
//! pending client document-deletion requests (git-repos surfaces
//! Task 2).
//!
//! Each row names the matter, the document, and who asked. **Authorize**
//! is admin-only — it runs the governed expunge — so the button is shown
//! only to admins; **Deny** is available to staff and admins and deletes
//! nothing.

use maud::{html, Markup};
use uuid::Uuid;

use crate::PageLayout;

/// One pending request, with its display fields pre-resolved.
pub struct Row {
    pub id: Uuid,
    pub matter: String,
    pub filename: String,
    pub requester: String,
    pub requested_at: String,
}

#[must_use]
pub fn list(rows: &[Row], csrf_token: &str, is_admin: bool) -> Markup {
    let body = html! {
        section.admin { div.container {
            header.page-header {
                h1 { "Document deletion requests" }
                p.muted {
                    "Clients who have asked to delete a document. Authorizing runs an "
                    "irreversible deletion that rewrites the matter's history."
                }
            }

            @if rows.is_empty() {
                p.muted { "No pending requests." }
            } @else {
                table.admin-table {
                    thead { tr {
                        th { "Matter" }
                        th { "Document" }
                        th { "Requested by" }
                        th { "Requested" }
                        th { "Action" }
                    } }
                    tbody {
                        @for row in rows {
                            tr {
                                td { (row.matter) }
                                td.mono { (row.filename) }
                                td { (row.requester) }
                                td { (row.requested_at) }
                                td {
                                    @if is_admin {
                                        form."d-inline" method="post"
                                            action=(format!("/portal/admin/expunge-requests/{}/authorize", row.id)) {
                                            input type="hidden" name="_csrf" value=(csrf_token);
                                            button.btn.btn-danger.btn-sm type="submit" { "Authorize deletion" }
                                        }
                                        " "
                                    }
                                    form."d-inline" method="post"
                                        action=(format!("/portal/admin/expunge-requests/{}/deny", row.id)) {
                                        input type="hidden" name="_csrf" value=(csrf_token);
                                        button.btn.btn-secondary.btn-sm type="submit" { "Deny" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } }
    };
    PageLayout::new("Document deletion requests — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{list, Row};
    use uuid::Uuid;

    fn a_row() -> Row {
        Row {
            id: Uuid::from_u128(1),
            matter: "Libra estate plan".into(),
            filename: "privileged.pdf".into(),
            requester: "Libra".into(),
            requested_at: "2026-06-04T10:00:00Z".into(),
        }
    }

    #[test]
    fn admin_sees_both_authorize_and_deny() {
        let html = list(&[a_row()], "tok", true).into_string();
        assert!(html.contains("Libra estate plan"));
        assert!(html.contains("privileged.pdf"));
        assert!(html.contains(
            "/portal/admin/expunge-requests/00000000-0000-0000-0000-000000000001/authorize"
        ));
        assert!(html
            .contains("/portal/admin/expunge-requests/00000000-0000-0000-0000-000000000001/deny"));
        assert!(html.contains("Authorize deletion"));
        assert!(html.contains("name=\"_csrf\" value=\"tok\""));
    }

    #[test]
    fn non_admin_sees_deny_but_not_authorize() {
        let html = list(&[a_row()], "tok", false).into_string();
        assert!(html.contains("Deny"));
        assert!(!html.contains("Authorize deletion"));
    }

    #[test]
    fn empty_queue_renders_a_friendly_message() {
        let html = list(&[], "tok", true).into_string();
        assert!(html.contains("No pending requests."));
    }
}
