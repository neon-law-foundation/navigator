// NeonLaw / NeonLaw Foundation in prose doc comments trip
// clippy::doc_markdown; the names are not code identifiers.
#![allow(clippy::doc_markdown)]

//! Contact page — `/contact`. It gives visitors one place to reach the
//! firm about matters and the Foundation about CLEs, open-source
//! contributions, and partnerships. Brand names come from
//! [`FIRM_BRAND`] / [`FOUNDATION_BRAND`] (env-overridable); emails and
//! the GitHub URL come from env vars so a fork can route inbound mail
//! without rewriting the page.

use std::env;
use std::sync::LazyLock;

use maud::{html, Markup};

use crate::brand::{foundation_github_url, FIRM_BRAND, FOUNDATION_BRAND};
use crate::components::ExternalLink;
use crate::{AuthState, PageLayout};

/// Firm contact email. Defaults to NeonLaw's real address; override
/// via `NAVIGATOR_SUPPORT_EMAIL` (shared with `views::brand::firm_email`).
static FIRM_CONTACT_EMAIL: LazyLock<String> = LazyLock::new(|| {
    env::var("NAVIGATOR_SUPPORT_EMAIL").unwrap_or_else(|_| "support@neonlaw.com".into())
});

/// Foundation contact email. Defaults to NeonLaw Foundation's real
/// address; override via `NAVIGATOR_FOUNDATION_EMAIL` (shared with
/// `views::brand::foundation_email`).
static FOUNDATION_CONTACT_EMAIL: LazyLock<String> = LazyLock::new(|| {
    env::var("NAVIGATOR_FOUNDATION_EMAIL").unwrap_or_else(|_| "support@neonlaw.org".into())
});

/// `/contact` — combined firm and Foundation contact page.
#[must_use]
pub fn render(auth: AuthState) -> Markup {
    let title = "Contact";
    let description = format!(
        "Reach {} for legal matters or {} for CLEs, Neon Law Navigator, and partnerships.",
        FIRM_BRAND.site_name, FOUNDATION_BRAND.site_name,
    );
    let firm_intro = format!(
        "Email {} with a short description of the matter — estate planning, corporate \
         formation, ongoing services. We respond within one business day with a flat-fee \
         quote and a calendar link.",
        FIRM_BRAND.site_name,
    );
    let foundation_intro = "Email the foundation about CLE programming, contributions \
         to the Neon Law Navigator open-source codebase, or partnership ideas with bar \
         associations and legal-aid organizations.";
    let github = foundation_github_url();
    let body = html! {
        article {
            h1 { (title) }
            section {
                h2 { (FIRM_BRAND.site_name) }
                p { (firm_intro) }
                dl {
                    dt { "Email" }
                    dd { a href=(format!("mailto:{}", &*FIRM_CONTACT_EMAIL)) { (&*FIRM_CONTACT_EMAIL) } }
                }
                p {
                    (ExternalLink::new(crate::brand::consultation_url())
                        .with_class("btn btn-primary")
                        .render(html! { (crate::i18n::t(crate::Locale::En, "cta.consultation")) }))
                }
            }
            section {
                h2 { (FOUNDATION_BRAND.site_name) }
                p { (foundation_intro) }
                dl {
                    dt { "Email" }
                    dd {
                        a href=(format!("mailto:{}", &*FOUNDATION_CONTACT_EMAIL)) {
                            (&*FOUNDATION_CONTACT_EMAIL)
                        }
                    }
                    dt { "GitHub" }
                    dd { a href=(github) { (github) } }
                }
            }
        }
    };
    PageLayout::new("Contact")
        .with_description(&description)
        .with_brand(*FIRM_BRAND)
        .with_auth(auth)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render, FIRM_CONTACT_EMAIL, FOUNDATION_CONTACT_EMAIL};
    use crate::brand::{FIRM_BRAND, FOUNDATION_BRAND};

    #[test]
    fn contact_uses_firm_brand_and_email() {
        let html = render(crate::AuthState::Anonymous).into_string();
        let title = format!("<title>{} | Contact</title>", FIRM_BRAND.site_name);
        assert!(html.contains(&title), "got: {html}");
        let mailto = format!("mailto:{}", &*FIRM_CONTACT_EMAIL);
        assert!(html.contains(&mailto));
    }

    #[test]
    fn contact_offers_a_consultation_booking_button() {
        let html = render(crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains(&format!("href=\"{}\"", crate::brand::consultation_url())),
            "firm contact should book a consultation: {html}"
        );
        assert!(html.contains("Book a Consultation"), "got: {html}");
    }

    #[test]
    fn foundation_section_uses_foundation_brand_and_email() {
        let html = render(crate::AuthState::Anonymous).into_string();
        assert!(html.contains(FOUNDATION_BRAND.site_name), "got: {html}");
        let mailto = format!("mailto:{}", &*FOUNDATION_CONTACT_EMAIL);
        assert!(html.contains(&mailto));
        assert!(html.contains("github.com/neon-law-foundation"));
    }

    #[test]
    fn contact_has_one_consultation_button() {
        let html = render(crate::AuthState::Anonymous).into_string();
        let consultation_href = format!("href=\"{}\"", crate::brand::consultation_url());
        assert!(
            html.matches(&consultation_href).count() == 1,
            "only the firm section should link the booking calendar: {html}"
        );
    }
}
