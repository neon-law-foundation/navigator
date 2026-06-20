//! `/help` — routes readers to the advocates we stand with for
//! matters outside the firm's transactional practice.
//!
//! Renders under the [`FOUNDATION_BRAND`] brand because the
//! foundation maintains the partner list; the firm side does not
//! practice in these areas. Address the reader as someone already
//! fighting for their rights and choosing where to spend the next
//! call — not as a person in crisis to be triaged. The firm `/`
//! links here directly so the routing is one click; the page itself
//! sits under the foundation nav because that is who curates it.
//!
//! [`FOUNDATION_BRAND`]: crate::brand::FOUNDATION_BRAND

use std::collections::BTreeMap;

use maud::{html, Markup, PreEscaped};

use crate::brand::{foundation_email, FIRM_BRAND, FOUNDATION_BRAND};
use crate::{AuthState, PageLayout};

/// Minimal view model: one partner organization as the page needs it.
/// Owned by `views` so the crate doesn't depend on `web`.
pub struct PartnerEntry<'a> {
    pub slug: &'a str,
    pub org_name: &'a str,
    pub topic: &'a str,
    pub jurisdictions: &'a str,
    pub phone: &'a str,
    pub url: &'a str,
    pub scope: &'a str,
    pub body_html: &'a str,
}

/// Render `/help`. `groups` maps `topic` → entries, both sorted by
/// the caller (`HelpIndex::by_topic` does this).
#[must_use]
pub fn render(groups: &BTreeMap<String, Vec<PartnerEntry<'_>>>, auth: AuthState) -> Markup {
    let intro = format!(
        "{} is a transactional firm — we handle estate planning, \
         investment LLCs, and flat-fee general counsel for early-stage \
         companies. We do not practice immigration, removal, eviction, \
         custody, or benefits law. The advocates below do, and we \
         stand with them. If you are fighting one of these matters, \
         these are the people we would call.",
        FIRM_BRAND.site_name,
    );
    let email = foundation_email();
    let mailto = format!("mailto:{email}");
    let small_email = email;
    let body = html! {
        article {
            h1 { "Advocates for matters outside our practice" }
            p { (intro) }
            p {
                small {
                    "This page is maintained by the "
                    a href="/foundation" { (FOUNDATION_BRAND.site_name) }
                    ". If a listing looks wrong or out of date, email "
                    (small_email) " and the foundation will update it."
                }
            }
            p {
                small {
                    "Nothing here is legal advice. These listings are a public resource, "
                    "not a substitute for speaking with an attorney about your situation."
                }
            }

            @if groups.is_empty() {
                (empty_state())
            } @else {
                @for (topic, entries) in groups {
                    section {
                        h2 { (humanize_topic(topic)) }
                        @for entry in entries {
                            (render_entry(entry))
                        }
                    }
                }
            }

            footer {
                p {
                    "If a listing is wrong or out of date, email "
                    a href=(mailto) { (email) }
                    " — the foundation will re-verify and update the page."
                }
            }
        }
    };
    let description = format!(
        "Legal-aid organizations and civil-rights advocates {} stands with \
         for matters outside our practice — immigration, eviction, removal, \
         custody, benefits. Curated by the {}.",
        FIRM_BRAND.site_name, FOUNDATION_BRAND.site_name,
    );
    PageLayout::new("Help")
        .with_description(&description)
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

fn render_entry(entry: &PartnerEntry<'_>) -> Markup {
    html! {
        article.partner {
            h3 { (entry.org_name) }
            dl {
                @if !entry.scope.is_empty() {
                    dt { "Helps with" }
                    dd { (entry.scope) }
                }
                @if !entry.jurisdictions.is_empty() {
                    dt { "Jurisdictions" }
                    dd { (entry.jurisdictions) }
                }
                @if !entry.phone.is_empty() {
                    dt { "Phone" }
                    dd { a href=(format!("tel:{}", entry.phone)) { (entry.phone) } }
                }
                @if !entry.url.is_empty() {
                    dt { "Website" }
                    dd { a href=(entry.url) { (entry.url) } }
                }
            }
            @if !entry.body_html.is_empty() {
                (PreEscaped(entry.body_html))
            }
        }
    }
}

fn empty_state() -> Markup {
    let email = foundation_email();
    let mailto = format!("mailto:{email}");
    html! {
        section.empty-state {
            h2 { "Partner list coming soon" }
            p {
                "The foundation is still curating this page. Email "
                a href=(mailto) { (email) }
                " and we will name the right advocates for your matter by hand."
            }
            p {
                small {
                    "We chose to ship this page empty rather than fill it with "
                    "unvetted referrals. A wrong phone number here would be "
                    "worse than no page at all — and you deserve advocates we "
                    "actually know."
                }
            }
        }
    }
}

/// `immigration` → `Immigration`, `removal_defense` → `Removal defense`.
/// Keeps the topic slug stable while rendering a humane heading.
fn humanize_topic(topic: &str) -> String {
    let mut chars = topic.replace('_', " ");
    if let Some(c) = chars.get_mut(..1) {
        c.make_ascii_uppercase();
    }
    chars
}

#[cfg(test)]
mod tests {
    use super::{humanize_topic, render, PartnerEntry};
    use crate::brand::{foundation_email, FIRM_BRAND, FOUNDATION_BRAND};
    use std::collections::BTreeMap;

    fn empty_groups() -> BTreeMap<String, Vec<PartnerEntry<'static>>> {
        BTreeMap::new()
    }

    #[test]
    fn empty_state_renders_when_no_partners_are_loaded() {
        let html = render(&empty_groups(), crate::AuthState::Anonymous).into_string();
        assert!(html.contains("Partner list coming soon"));
        let email = foundation_email();
        assert!(html.contains(&format!("mailto:{email}")));
        // Honest framing — we don't pretend to have partners.
        assert!(
            html.contains("wrong phone number on this page would"),
            "got: {html}"
        );
    }

    #[test]
    fn page_renders_under_foundation_brand() {
        let html = render(&empty_groups(), crate::AuthState::Anonymous).into_string();
        assert!(html.contains(&format!(
            "<title>{} | Help</title>",
            FOUNDATION_BRAND.site_name
        )));
        // Foundation nav links back to the firm.
        assert!(html.contains(&format!(">{}</a>", FIRM_BRAND.site_name)));
    }

    #[test]
    fn page_disclaims_what_the_firm_cannot_take() {
        let html = render(&empty_groups(), crate::AuthState::Anonymous).into_string();
        // The visitor must learn, on the page, that the firm itself
        // cannot represent in these matters. Otherwise the page reads
        // like the firm IS the help.
        assert!(html.contains("We do not practice"));
        assert!(html.contains("immigration"));
    }

    #[test]
    fn page_disclaims_legal_advice_in_the_help_copy() {
        let html = render(&empty_groups(), crate::AuthState::Anonymous).into_string();
        assert!(html.contains("Nothing here is legal advice"));
        assert!(html.contains("not a substitute for speaking with an attorney"));
    }

    #[test]
    fn renders_partners_grouped_by_topic_in_alphabetical_order() {
        let mut groups: BTreeMap<String, Vec<PartnerEntry<'_>>> = BTreeMap::new();
        groups.insert(
            "immigration".into(),
            vec![PartnerEntry {
                slug: "x",
                org_name: "Acme Legal Aid",
                topic: "immigration",
                jurisdictions: "Nevada",
                phone: "1-800-555-0199",
                url: "https://example.org",
                scope: "removal defense",
                body_html: "<p>extra notes</p>",
            }],
        );
        let html = render(&groups, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("<h2>Immigration</h2>"));
        assert!(html.contains("Acme Legal Aid"));
        assert!(html.contains("tel:1-800-555-0199"));
        assert!(html.contains("https://example.org"));
        assert!(html.contains("removal defense"));
        assert!(html.contains("<p>extra notes</p>"));
    }

    #[test]
    fn humanize_topic_capitalizes_and_replaces_underscores() {
        assert_eq!(humanize_topic("immigration"), "Immigration");
        assert_eq!(humanize_topic("removal_defense"), "Removal defense");
        assert_eq!(humanize_topic(""), "");
    }
}
