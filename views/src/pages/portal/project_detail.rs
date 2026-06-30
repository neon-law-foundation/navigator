//! Single-matter detail rendered at `GET /portal/projects/:id` for
//! clients. Intentionally thin — name, status chip, and a way back
//! to the matter list. Staff and admin hitting the same URL fall
//! through to the admin-chrome view (documents, upload)
//! per the role-aware dispatcher in
//! `web::admin::projects_detail_role_aware`.

use maud::{html, Markup};
use uuid::Uuid;

use crate::PageLayout;

/// One document the client can open to read and comment on.
pub struct ReviewDocRow<'a> {
    pub id: Uuid,
    pub title: &'a str,
    pub kind: &'a str,
    pub status: &'a str,
}

/// One of the matter's documents the client can take or ask to delete.
pub struct ClientDocRow<'a> {
    pub id: Uuid,
    pub filename: &'a str,
    /// `true` when a deletion request is already pending for this
    /// document — the client sees a status, not the control again.
    pub deletion_requested: bool,
}

/// One of the matter's notations (e.g. the retainer), named in plain
/// words for the client, with download links for whichever of its three
/// PDFs exist. The links point at the notation-documents route, gated by
/// the project-participation ACL — a participant may download; everyone
/// else gets 404.
pub struct NotationDocRow<'a> {
    pub id: Uuid,
    /// Plain-language name (the template title, e.g. "Retainer
    /// Agreement"), never a docket code.
    pub title: &'a str,
    /// Client-friendly status, e.g. "Signed" / "Awaiting your signature".
    pub status: &'a str,
    /// Whether the rendered (unsigned) PDF is present in storage.
    pub rendered_ready: bool,
    /// Whether the executed (signed) PDF is present.
    pub signed_ready: bool,
    /// Whether the Certificate of Completion is present.
    pub certificate_ready: bool,
}

/// The matter's invoice, read from the local Xero mirror (the portal
/// never calls Xero live). Amounts are pre-formatted by the handler; the
/// Xero invoice id is deliberately **not** carried here — it stays
/// server-side.
pub struct InvoiceView<'a> {
    /// Formatted total, e.g. `$3,333.00`.
    pub amount: &'a str,
    /// Provider status mirror (`AUTHORISED`, `PAID`, …).
    pub status: &'a str,
    /// `true` once reconcile has seen the invoice paid in full.
    pub paid: bool,
}

pub struct Detail<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub status: &'a str,
    /// The matter's invoice from the local mirror, when one has been
    /// raised. `None` until the matter-close fee lands.
    pub invoice: Option<InvoiceView<'a>>,
    /// The matter's notations (retainer, etc.) with their executed PDFs,
    /// named in plain words and gated by project participation.
    pub notations: &'a [NotationDocRow<'a>],
    /// Attorney-advanced drafts the client may read and comment on.
    pub review_docs: &'a [ReviewDocRow<'a>],
    /// The matter's documents the client can download or request deletion
    /// of.
    pub documents: &'a [ClientDocRow<'a>],
    /// Per-session CSRF token for the deletion-request forms. Empty when
    /// no session is attached (the forms are then not actionable).
    pub csrf_token: &'a str,
    /// `true` when this is a Northstar estate matter waiting on the
    /// client's approval — every draft has been released for review and
    /// the client can now approve the whole plan. Renders the
    /// "Approve my plan" control.
    pub show_approve_plan: bool,
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn render(d: &Detail<'_>) -> Markup {
    let body = html! {
        section."portal portal-project" {
            nav."mb-3" {
                a href="/portal" { "← Your matters" }
            }
            h1."mb-2" { (d.name) }
            p."mb-4" {
                span."badge text-bg-secondary text-uppercase" { (d.status) }
            }
            p."text-body-secondary" {
                "Matter id: " code { (d.id) }
            }
            p."mb-4 d-flex gap-2" {
                a."btn btn-outline-secondary btn-sm"
                    href=(format!("/portal/projects/{}/documents.zip", d.id))
                    download {
                    "Download all my documents"
                }
                a."btn btn-outline-primary btn-sm"
                    href=(format!("/portal/projects/{}/conversation", d.id)) {
                    "Conversation"
                }
            }
            @if let Some(inv) = &d.invoice {
                section."mt-4" {
                    h2."h5 mb-3" { "Invoice" }
                    div."card" {
                        div."card-body d-flex justify-content-between align-items-center" {
                            div {
                                div."fs-5" { (inv.amount) }
                                div."text-body-secondary small text-uppercase" {
                                    "Status: " (inv.status)
                                }
                            }
                            @if inv.paid {
                                span."badge text-bg-success text-uppercase" { "Paid" }
                            } @else {
                                span."badge text-bg-warning text-uppercase" { "Due" }
                            }
                        }
                    }
                }
            }
            @if !d.notations.is_empty() {
                section."mt-4" {
                    h2."h5 mb-3" { "Your agreements" }
                    div."list-group" {
                        @for n in d.notations {
                            div."list-group-item d-flex justify-content-between align-items-center" {
                                span {
                                    (n.title)
                                    span."badge text-bg-secondary text-uppercase ms-2" { (n.status) }
                                }
                                span."d-flex gap-2" {
                                    @if n.rendered_ready {
                                        a."btn btn-outline-secondary btn-sm"
                                            href=(format!("/portal/admin/notations/{}/documents/retainer", n.id)) {
                                            "Agreement"
                                        }
                                    }
                                    @if n.signed_ready {
                                        a."btn btn-outline-primary btn-sm"
                                            href=(format!("/portal/admin/notations/{}/documents/signed", n.id)) {
                                            "Signed copy"
                                        }
                                    }
                                    @if n.certificate_ready {
                                        a."btn btn-outline-secondary btn-sm"
                                            href=(format!("/portal/admin/notations/{}/documents/certificate", n.id)) {
                                            "Certificate"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            @if !d.documents.is_empty() {
                section."mt-4" {
                    h2."h5 mb-3" { "Your documents" }
                    div."list-group" {
                        @for doc in d.documents {
                            div."list-group-item d-flex justify-content-between align-items-center" {
                                span { (doc.filename) }
                                @if doc.deletion_requested {
                                    span."badge text-bg-warning text-uppercase" { (crate::i18n::t(crate::Locale::En, "portal.deletion_requested")) }
                                } @else {
                                    form method="post"
                                        action=(format!("/portal/projects/{}/documents/{}/request-deletion", d.id, doc.id)) {
                                        input type="hidden" name="_csrf" value=(d.csrf_token);
                                        button."btn btn-outline-danger btn-sm" type="submit" {
                                            (crate::i18n::t(crate::Locale::En, "portal.delete_document"))
                                        }
                                    }
                                }
                            }
                        }
                    }
                    p."text-body-secondary small mt-2" {
                        "Deleting a document asks your attorney to permanently remove it. "
                        "It stays until they approve the request."
                    }
                }
            }
            @if d.review_docs.is_empty() {
                p."text-body-secondary" {
                    "Documents to review will appear here once your attorney has "
                    "prepared them."
                }
            } @else {
                section."mt-4" {
                    h2."h5 mb-3" { "Documents to review" }
                    div."list-group" {
                        @for doc in d.review_docs {
                            a."list-group-item list-group-item-action d-flex justify-content-between align-items-center"
                                href=(format!("/portal/projects/{}/review/{}", d.id, doc.id)) {
                                span {
                                    (doc.title)
                                    span."badge text-bg-light text-uppercase ms-2" { (doc.kind) }
                                }
                                span."badge text-bg-secondary text-uppercase" { (doc.status) }
                            }
                        }
                    }
                    @if d.show_approve_plan {
                        form."mt-3" method="post"
                            action=(format!("/portal/projects/{}/approve-plan", d.id)) {
                            input type="hidden" name="_csrf" value=(d.csrf_token);
                            p."text-body-secondary small mb-2" {
                                "When you have read each document and you are ready, approve your "
                                "plan. It then goes to signing."
                            }
                            button."btn btn-primary" type="submit" { "Approve my plan" }
                        }
                    }
                }
            }
        }
    };
    PageLayout::new(d.name)
        .with_description("Matter detail in your portal.")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render, Detail};
    use uuid::Uuid;

    /// The `<main>` body of a rendered page, excluding the shared chrome
    /// (head + footer). The no-git-jargon guard scans only this — the head's
    /// star-count script asset and the footer's open-source repo-star CTA
    /// legitimately name GitHub and are identical on every page.
    fn main_body(html: &str) -> &str {
        let start = html.find("<main").expect("main present");
        let end = html.find("</main>").expect("main close present") + "</main>".len();
        &html[start..end]
    }

    #[test]
    fn detail_renders_name_status_and_back_link() {
        let id = Uuid::now_v7();
        let html = render(&Detail {
            id,
            name: "Atlas LLC",
            status: "open",
            invoice: None,
            notations: &[],
            review_docs: &[],
            documents: &[],
            csrf_token: "",
            show_approve_plan: false,
        })
        .into_string();
        assert!(html.contains("Atlas LLC"));
        assert!(html.contains("open"));
        assert!(html.contains("href=\"/portal\""));
        assert!(html.contains(&id.to_string()));
        // No invoice raised yet → no invoice card.
        assert!(!html.contains("Invoice"));
    }

    #[test]
    fn renders_invoice_card_with_paid_badge() {
        use super::InvoiceView;
        let html = render(&Detail {
            id: Uuid::now_v7(),
            name: "Estate of Capricorn",
            status: "closed",
            invoice: Some(InvoiceView {
                amount: "$3,333.00",
                status: "PAID",
                paid: true,
            }),
            notations: &[],
            review_docs: &[],
            documents: &[],
            csrf_token: "",
            show_approve_plan: false,
        })
        .into_string();
        assert!(html.contains("Invoice"));
        assert!(html.contains("$3,333.00"));
        assert!(html.contains("PAID"));
        assert!(html.contains("Paid"), "paid badge expected: {html}");
    }

    #[test]
    fn renders_invoice_card_with_due_badge_when_unpaid() {
        use super::InvoiceView;
        let html = render(&Detail {
            id: Uuid::now_v7(),
            name: "Estate of Capricorn",
            status: "closed",
            invoice: Some(InvoiceView {
                amount: "$3,333.00",
                status: "AUTHORISED",
                paid: false,
            }),
            notations: &[],
            review_docs: &[],
            documents: &[],
            csrf_token: "",
            show_approve_plan: false,
        })
        .into_string();
        assert!(html.contains("Due"), "due badge expected: {html}");
    }

    #[test]
    fn offers_a_download_all_button_with_no_git_jargon() {
        let id = Uuid::now_v7();
        let html = render(&Detail {
            id,
            name: "Atlas LLC",
            status: "open",
            invoice: None,
            notations: &[],
            review_docs: &[],
            documents: &[],
            csrf_token: "",
            show_approve_plan: false,
        })
        .into_string();
        assert!(html.contains("Download all my documents"));
        assert!(html.contains(&format!("href=\"/portal/projects/{id}/documents.zip\"")));
        // The client never sees that documents are backed by a git repo.
        // Scope the scan to the <main> body — the shared chrome legitimately
        // names GitHub (the head's star-count script asset and the footer's
        // open-source repo-star CTA) and is identical on every page.
        let lower = main_body(&html).to_lowercase();
        for jargon in ["git", "clone", "branch", "packfile", "commit", "repository"] {
            assert!(
                !lower.contains(jargon),
                "client portal leaked git jargon: {jargon}"
            );
        }
    }

    #[test]
    fn documents_offer_delete_control_or_pending_status_without_git_jargon() {
        let id = Uuid::from_u128(1);
        let with_control = Uuid::from_u128(2);
        let already_requested = Uuid::from_u128(3);
        let html = render(&Detail {
            id,
            name: "Atlas LLC",
            status: "open",
            invoice: None,
            notations: &[],
            review_docs: &[],
            documents: &[
                super::ClientDocRow {
                    id: with_control,
                    filename: "engagement-letter.pdf",
                    deletion_requested: false,
                },
                super::ClientDocRow {
                    id: already_requested,
                    filename: "old-draft.pdf",
                    deletion_requested: true,
                },
            ],
            csrf_token: "tok",
            show_approve_plan: false,
        })
        .into_string();
        assert!(html.contains("Your documents"));
        assert!(html.contains("engagement-letter.pdf"));
        // A pending doc shows status, not the control.
        assert!(html.contains("Deletion requested"));
        // The actionable doc posts to the request-deletion route with CSRF.
        assert!(html.contains(&format!(
            "action=\"/portal/projects/{id}/documents/{with_control}/request-deletion\""
        )));
        assert!(html.contains("Delete this document"));
        assert!(html.contains("name=\"_csrf\" value=\"tok\""));
        // Still no git jargon on the client surface — scoped to the <main>
        // body; the shared chrome legitimately names GitHub.
        let lower = main_body(&html).to_lowercase();
        for jargon in [
            "git",
            "clone",
            "branch",
            "packfile",
            "repository",
            "expunge",
        ] {
            assert!(
                !lower.contains(jargon),
                "client portal leaked jargon: {jargon}"
            );
        }
    }

    #[test]
    fn lists_review_documents_with_links_when_present() {
        let id = Uuid::nil();
        let doc_id = Uuid::nil();
        let html = render(&Detail {
            id,
            name: "Libra estate plan",
            status: "open",
            invoice: None,
            notations: &[],
            review_docs: &[super::ReviewDocRow {
                id: doc_id,
                title: "Last Will and Testament",
                kind: "will",
                status: "pending_review",
            }],
            documents: &[],
            csrf_token: "",
            show_approve_plan: false,
        })
        .into_string();
        assert!(html.contains("Documents to review"));
        assert!(html.contains("Last Will and Testament"));
        assert!(html.contains(
            "href=\"/portal/projects/00000000-0000-0000-0000-000000000000/review/00000000-0000-0000-0000-000000000000\""
        ));
        // No approve control unless explicitly enabled.
        assert!(!html.contains("Approve my plan"));
    }

    #[test]
    fn approve_plan_control_renders_when_enabled() {
        let id = Uuid::from_u128(7);
        let html = render(&Detail {
            id,
            name: "Capricorn estate plan",
            status: "open",
            invoice: None,
            notations: &[],
            review_docs: &[super::ReviewDocRow {
                id: Uuid::from_u128(8),
                title: "Last Will and Testament",
                kind: "will",
                status: "pending_review",
            }],
            documents: &[],
            csrf_token: "tok",
            show_approve_plan: true,
        })
        .into_string();
        assert!(html.contains("Approve my plan"));
        assert!(html.contains(&format!("action=\"/portal/projects/{id}/approve-plan\"")));
        assert!(html.contains("name=\"_csrf\" value=\"tok\""));
    }
}
