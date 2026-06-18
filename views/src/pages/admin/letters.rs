//! Admin /letters/:id detail page.
//!
//! Read-only detail view for a single piece of mail. The list page
//! at `/portal/admin/letters` resolves the mailroom foreign key already;
//! this view goes further and shows the full mailroom (address +
//! name) alongside the letter so a paralegal can scan the routing
//! context without an extra click.

use maud::{html, Markup};
use uuid::Uuid;

use crate::PageLayout;

pub struct LetterDetail<'a> {
    pub id: Uuid,
    pub direction: &'a str,
    pub sender: &'a str,
    pub recipient: &'a str,
    pub summary: &'a str,
    pub mailroom_name: &'a str,
    pub mailroom_address: &'a str,
}

#[must_use]
pub fn detail(d: &LetterDetail<'_>) -> Markup {
    let body = html! {
        section.admin { div.container {
            header.page-header {
                h1 { "Letter #" (d.id) }
                p { a href="/portal/admin/letters" { "← Back to letters" } }
            }
            dl.admin-detail {
                dt { "Direction" }
                dd { (d.direction) }
                dt { "Sender" }
                dd { (d.sender) }
                dt { "Recipient" }
                dd { (d.recipient) }
                dt { "Summary" }
                dd { (d.summary) }
                dt { "Mailroom" }
                dd { (d.mailroom_name) }
                dt { "Mailroom address" }
                dd { (d.mailroom_address) }
            }
        } }
    };
    PageLayout::new(&format!("Letter #{} — Admin", d.id))
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// Page rendered when `/portal/admin/letters/:id` resolves to no row.
#[must_use]
pub fn not_found(id: Uuid) -> Markup {
    let body = html! {
        section.admin { div.container {
            h1 { "Letter not found" }
            p { "No letter exists with id " code { (id) } "." }
            p { a href="/portal/admin/letters" { "← Back to letters" } }
        } }
    };
    PageLayout::new("Letter not found — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{detail, not_found, LetterDetail};
    use uuid::Uuid;

    const ID7: Uuid = Uuid::from_u128(7);
    const ID99: Uuid = Uuid::from_u128(99);

    #[test]
    fn detail_renders_every_field_and_back_link() {
        let html = detail(&LetterDetail {
            id: ID7,
            direction: "incoming",
            sender: "IRS",
            recipient: "Acme Trust",
            summary: "EIN confirmation letter",
            mailroom_name: "HQ",
            mailroom_address: "123 Main, Reno, NV",
        })
        .into_string();
        assert!(html.contains(&format!(
            "<title>{} | Letter #{ID7} — Admin</title>",
            crate::brand::FIRM_BRAND.site_name
        )));
        assert!(html.contains(&format!("Letter #{ID7}")));
        assert!(html.contains("incoming"));
        assert!(html.contains("IRS"));
        assert!(html.contains("Acme Trust"));
        assert!(html.contains("EIN confirmation letter"));
        assert!(html.contains("HQ"));
        assert!(html.contains("123 Main, Reno, NV"));
        assert!(html.contains("href=\"/portal/admin/letters\""));
    }

    #[test]
    fn not_found_renders_a_helpful_back_link() {
        let html = not_found(ID99).into_string();
        assert!(html.contains("Letter not found"));
        assert!(html.contains(&format!("<code>{ID99}</code>")));
        assert!(html.contains("href=\"/portal/admin/letters\""));
    }
}
