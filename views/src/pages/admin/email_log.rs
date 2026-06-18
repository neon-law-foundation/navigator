//! Admin /email-log page: read-only paginated view over `sent_emails`.
//!
//! The audit log of every outbound message that went through the
//! `EmailService` trait. Newest first. The body column is
//! intentionally not rendered on the index — that's a deeper grant
//! the page doesn't carry today; future "view body" lives behind its
//! own route + policy check.

use maud::{html, Markup};
use uuid::Uuid;

use crate::PageLayout;

pub struct Row<'a> {
    pub id: Uuid,
    pub recipient: &'a str,
    pub subject: &'a str,
    pub sender: &'a str,
    pub template_slug: Option<&'a str>,
    pub outcome: &'a str,
    pub sent_at: &'a str,
}

/// Pagination state shared between the handler and the view.
/// `page` is 1-indexed; `total_pages` is `max(1, ceil(total / per_page))`
/// so an empty table renders as page 1 of 1.
pub struct Pagination {
    pub page: u64,
    pub per_page: u64,
    pub total_pages: u64,
}

#[must_use]
pub fn list(rows: &[Row<'_>], pagination: &Pagination) -> Markup {
    let prev_page = pagination.page.saturating_sub(1).max(1);
    let next_page = (pagination.page + 1).min(pagination.total_pages);
    let body = html! {
        section.admin { div.container {
            h1 { "Email log" }
            p.subtle {
                "Every outbound message that went through the SendGrid path. "
                "Gmail mail from Workspace mailboxes is intentionally not logged here."
            }
            @if rows.is_empty() {
                p.empty {
                    "No outbound mail in the audit window — fresh signups will appear here."
                }
            } @else {
                table.admin-table {
                    thead {
                        tr {
                            th { "Sent at" }
                            th { "Recipient" }
                            th { "Subject" }
                            th { "From" }
                            th { "Template" }
                            th { "Outcome" }
                        }
                    }
                    tbody {
                        @for r in rows {
                            tr {
                                td { (r.sent_at) }
                                td { (r.recipient) }
                                td { (r.subject) }
                                td { (r.sender) }
                                td { (r.template_slug.unwrap_or("—")) }
                                td { (r.outcome) }
                            }
                        }
                    }
                }
                nav.pagination {
                    @if pagination.page > 1 {
                        a href={ "/portal/admin/email-log?page=" (prev_page) } { "← Previous" }
                    }
                    span { "Page " (pagination.page) " of " (pagination.total_pages) }
                    @if pagination.page < pagination.total_pages {
                        a href={ "/portal/admin/email-log?page=" (next_page) } { "Next →" }
                    }
                }
            }
        } }
    };
    PageLayout::new("Email log — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{list, Pagination, Row};
    use uuid::Uuid;

    const ID1: Uuid = Uuid::from_u128(1);
    const ID2: Uuid = Uuid::from_u128(2);

    fn page1() -> Pagination {
        Pagination {
            page: 1,
            per_page: 50,
            total_pages: 1,
        }
    }

    #[test]
    fn list_empty_state_explains_what_lands_here() {
        let html = list(&[], &page1()).into_string();
        assert!(html.contains("No outbound mail in the audit window"));
        // No pagination chrome when empty.
        assert!(!html.contains("Previous"));
        assert!(!html.contains("Next"));
    }

    #[test]
    fn list_renders_metadata_columns_but_not_body() {
        let rows = [Row {
            id: ID1,
            recipient: "aries@example.com",
            subject: "Welcome to the firm",
            sender: "support@example.com",
            template_slug: Some("welcome"),
            outcome: "sent",
            sent_at: "2026-05-24T20:00:00Z",
        }];
        let html = list(&rows, &page1()).into_string();
        assert!(html.contains("aries@example.com"));
        assert!(html.contains("Welcome to the firm"));
        assert!(html.contains("support@example.com"));
        assert!(html.contains("welcome"));
        assert!(html.contains("sent"));
        assert!(html.contains("2026-05-24T20:00:00Z"));
    }

    #[test]
    fn list_renders_em_dash_when_template_slug_is_none() {
        let rows = [Row {
            id: ID2,
            recipient: "x@y",
            subject: "Ad-hoc",
            sender: "support@example.com",
            template_slug: None,
            outcome: "sent",
            sent_at: "2026-05-24T20:00:00Z",
        }];
        let html = list(&rows, &page1()).into_string();
        assert!(html.contains(">—<"));
    }

    #[test]
    fn list_renders_pagination_chrome_when_multiple_pages() {
        let rows = [Row {
            id: ID1,
            recipient: "a@b",
            subject: "s",
            sender: "support@example.com",
            template_slug: Some("welcome"),
            outcome: "sent",
            sent_at: "t",
        }];
        let pagination = Pagination {
            page: 2,
            per_page: 50,
            total_pages: 5,
        };
        let html = list(&rows, &pagination).into_string();
        assert!(html.contains("Previous"));
        assert!(html.contains("Next"));
        assert!(html.contains("Page 2 of 5"));
        assert!(html.contains("/portal/admin/email-log?page=1"));
        assert!(html.contains("/portal/admin/email-log?page=3"));
    }

    #[test]
    fn list_omits_previous_link_on_first_page() {
        let rows = [Row {
            id: ID1,
            recipient: "a@b",
            subject: "s",
            sender: "support@example.com",
            template_slug: Some("welcome"),
            outcome: "sent",
            sent_at: "t",
        }];
        let pagination = Pagination {
            page: 1,
            per_page: 50,
            total_pages: 3,
        };
        let html = list(&rows, &pagination).into_string();
        assert!(!html.contains("Previous"));
        assert!(html.contains("Next"));
    }

    #[test]
    fn list_omits_next_link_on_last_page() {
        let rows = [Row {
            id: ID1,
            recipient: "a@b",
            subject: "s",
            sender: "support@example.com",
            template_slug: Some("welcome"),
            outcome: "sent",
            sent_at: "t",
        }];
        let pagination = Pagination {
            page: 3,
            per_page: 50,
            total_pages: 3,
        };
        let html = list(&rows, &pagination).into_string();
        assert!(html.contains("Previous"));
        assert!(!html.contains("Next"));
    }
}
