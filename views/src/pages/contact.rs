// NeonLaw / NeonLaw Foundation in prose doc comments trip
// clippy::doc_markdown; the names are not code identifiers.
#![allow(clippy::doc_markdown)]

//! Contact pages — `/contact` (firm) and `/foundation/contact`
//! (foundation). The two share the rendering scaffold but differ in
//! brand, email address, and the trailing GitHub link. The brand
//! display name comes from [`FIRM_BRAND`] / [`FOUNDATION_BRAND`]
//! (env-overridable); the email + GitHub URL come from env vars so a
//! fork can route inbound mail to its own address without rewriting
//! the page.

use std::env;
use std::sync::LazyLock;

use maud::{html, Markup};

use crate::brand::{foundation_github_url, SiteBrand, FIRM_BRAND, FOUNDATION_BRAND};
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

/// `/contact` — firm contact card. Routes a visitor to the firm's
/// configured support address for a flat-fee quote.
#[must_use]
pub fn render_firm(auth: AuthState) -> Markup {
    let title = format!("Contact {}", FIRM_BRAND.site_name);
    let intro = format!(
        "{} is accepting new clients. Email us with a short description of the matter — estate \
         planning, corporate formation, ongoing services. We respond \
         within one business day with a flat-fee quote and a calendar \
         link.",
        FIRM_BRAND.site_name,
    );
    render(
        &title,
        "Get a flat-fee quote from the firm.",
        *FIRM_BRAND,
        &FIRM_CONTACT_EMAIL,
        None,
        &intro,
        auth,
    )
}

/// `/foundation/contact` — foundation contact card. Points at the
/// configured foundation address and (optionally) a GitHub org.
#[must_use]
pub fn render_foundation(auth: AuthState) -> Markup {
    let description = format!(
        "Reach the {} about CLEs, Navigator, or partnerships.",
        FOUNDATION_BRAND.site_name,
    );
    let intro = "Email the foundation about CLE programming, contributions \
         to the Navigator open-source codebase, or partnership ideas \
         with bar associations and legal-aid organizations."
        .to_string();
    let github: Option<(&str, &str)> = foundation_github_url().map(|url| ("GitHub", url));
    render(
        "Contact the Foundation",
        &description,
        *FOUNDATION_BRAND,
        &FOUNDATION_CONTACT_EMAIL,
        github,
        &intro,
        auth,
    )
}

#[allow(clippy::too_many_arguments)]
fn render(
    title: &str,
    description: &str,
    brand: SiteBrand,
    email: &str,
    github: Option<(&str, &str)>,
    intro: &str,
    auth: AuthState,
) -> Markup {
    let body = html! {
        article {
            h1 { (title) }
            p { (intro) }
            dl {
                dt { "Email" }
                dd { a href=(format!("mailto:{email}")) { (email) } }
                @if let Some((label, href)) = github {
                    dt { (label) }
                    dd { a href=(href) { (href) } }
                }
            }
            // The firm's primary action is booking a flat-fee
            // consultation on the calendar the intro promises; the
            // Foundation card stays email-only. Off-site, so it routes
            // through `ExternalLink` for the new-tab + OWASP `rel` pair.
            @if brand.is_law_firm {
                p."fw-semibold" { (crate::i18n::t(crate::Locale::En, "cta.accepting_clients")) }
                p {
                    (ExternalLink::new(crate::brand::consultation_url())
                        .with_class("btn btn-primary")
                        .render(html! { (crate::i18n::t(crate::Locale::En, "cta.consultation")) }))
                }
            }
        }
    };
    PageLayout::new("Contact")
        .with_description(description)
        .with_brand(brand)
        .with_auth(auth)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render_firm, render_foundation, FIRM_CONTACT_EMAIL, FOUNDATION_CONTACT_EMAIL};
    use crate::brand::{FIRM_BRAND, FOUNDATION_BRAND};

    #[test]
    fn firm_contact_uses_firm_brand_and_email() {
        let html = render_firm(crate::AuthState::Anonymous).into_string();
        let title = format!("<title>{} | Contact</title>", FIRM_BRAND.site_name);
        assert!(html.contains(&title), "got: {html}");
        let mailto = format!("mailto:{}", &*FIRM_CONTACT_EMAIL);
        assert!(html.contains(&mailto));
    }

    #[test]
    fn firm_contact_offers_a_consultation_booking_button() {
        let html = render_firm(crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains(&format!("href=\"{}\"", crate::brand::consultation_url())),
            "firm contact should book a consultation: {html}"
        );
        assert!(html.contains("Book a Consultation"), "got: {html}");
        assert!(html.contains("Now accepting new clients."), "got: {html}");
    }

    #[test]
    fn foundation_contact_has_no_consultation_button() {
        // The booking calendar is the firm's; the Foundation card stays
        // email-only.
        let html = render_foundation(crate::AuthState::Anonymous).into_string();
        assert!(
            !html.contains(crate::brand::consultation_url()),
            "Foundation contact must not link the firm calendar: {html}"
        );
        assert!(
            !html.contains("Now accepting new clients."),
            "Foundation contact must not render firm intake copy: {html}"
        );
    }

    #[test]
    fn foundation_contact_uses_foundation_brand_and_email() {
        let html = render_foundation(crate::AuthState::Anonymous).into_string();
        let title = format!("<title>{} | Contact</title>", FOUNDATION_BRAND.site_name);
        assert!(html.contains(&title), "got: {html}");
        let mailto = format!("mailto:{}", &*FOUNDATION_CONTACT_EMAIL);
        assert!(html.contains(&mailto));
    }
}
