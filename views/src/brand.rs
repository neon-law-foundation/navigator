// Brand-name prose (NeonLaw, NeonLaw Foundation) trips
// clippy::doc_markdown; the brand names are not code identifiers.
// Same precedent as views/src/components.rs.
#![allow(clippy::doc_markdown)]

//! Site brand: the strings and links that identify the product to
//! the visitor (name, copyright owner, nav targets).
//!
//! Two brands share one binary: [`FIRM_BRAND`] for the law firm,
//! [`FOUNDATION_BRAND`] for the 501(c)(3). Each page picks its brand
//! with `PageLayout::with_brand`; the layout never branches on the URL.
//!
//! ## Customizing the brand
//!
//! The names default to a generic "Navigator" / "Navigator Foundation"
//! so a fresh OSS clone never accidentally ships pre-branded under
//! another organization's name. Set the following env vars (typically
//! in your `.env`) before `web` starts to override:
//!
//! - `NAVIGATOR_BRAND_FIRM` — the firm's display name (default
//!   `"Navigator"`).
//! - `NAVIGATOR_BRAND_FOUNDATION` — the foundation's display name
//!   (default `"Navigator Foundation"`).
//!
//! The brand structs cache the resolved values in `LazyLock`, so the
//! env vars are read on first access and the resulting `&'static str`
//! is reused for the lifetime of the process.

use std::env;
use std::sync::LazyLock;

/// Bundle of strings + nav links that identify the running site.
///
/// `Copy` is preserved so the layout's `with_brand(SiteBrand)` API
/// continues to take the brand by value without a clone.
#[derive(Debug, Clone, Copy)]
pub struct SiteBrand {
    pub site_name: &'static str,
    pub tagline: &'static str,
    /// One-line postal address rendered in the footer. Differs per
    /// entity (the firm and the Foundation share a street but hold
    /// distinct private-mailbox suites). Overridable via env so an OSS
    /// fork can plug in its own registered address.
    pub postal_address: &'static str,
    /// Path to the brand mark SVG served under `/public/`.
    pub logo_href: &'static str,
    /// Path to a **raster** (PNG) brand mark served under `/public/`,
    /// used as the Open Graph / Twitter Card `og:image`. Social-share
    /// scrapers (iMessage, Slack, Facebook, X, LinkedIn) generally
    /// won't rasterize SVG, so the share card needs a PNG distinct
    /// from [`logo_href`].
    pub social_image: &'static str,
    pub nav: &'static [NavLink],
    /// When true, the layout renders firm-only portal links. Foundation
    /// pages (the 501(c)(3) doesn't practice law) leave this false. The
    /// legal-advice disclaimer is no longer gated here — the unified footer
    /// always shows the firm's via [`firm_disclaimer`].
    pub is_law_firm: bool,
    /// USPTO record URL for the brand wordmark when it is a registered
    /// trademark. When `Some`, the footer renders a linked `®` after the
    /// brand name. `None` for the Foundation (the registered mark is the
    /// firm's "NEON LAW") and for any OSS fork that rebrands via
    /// `NAVIGATOR_BRAND_FIRM` — a fork's own name is not our mark.
    pub trademark_registration_url: Option<&'static str>,
}

/// One header nav entry. A `NavLink` is either a leaf (no children)
/// or a dropdown (children populate a Pico `<details class="dropdown">`).
#[derive(Debug, Clone, Copy)]
pub struct NavLink {
    pub label: &'static str,
    pub href: &'static str,
    /// Optional sub-items. Empty slice means a plain leaf link.
    pub children: &'static [NavLink],
    /// Optional Bootstrap Icon name (the part after `bi-`, e.g.
    /// `"star-fill"`) shown before the label. `None` renders no icon.
    /// Used to denote each product in the Services dropdown.
    pub icon: Option<&'static str>,
}

impl NavLink {
    #[must_use]
    pub const fn leaf(label: &'static str, href: &'static str) -> Self {
        Self {
            label,
            href,
            children: &[],
            icon: None,
        }
    }

    /// A leaf link prefixed with a Bootstrap Icon. `icon` is the glyph
    /// name without the `bi-` prefix (e.g. `"shield-fill-check"`).
    #[must_use]
    pub const fn leaf_with_icon(
        label: &'static str,
        href: &'static str,
        icon: &'static str,
    ) -> Self {
        Self {
            label,
            href,
            children: &[],
            icon: Some(icon),
        }
    }

    #[must_use]
    pub const fn dropdown(label: &'static str, children: &'static [NavLink]) -> Self {
        Self {
            label,
            href: "#",
            children,
            icon: None,
        }
    }

    #[must_use]
    pub const fn is_dropdown(&self) -> bool {
        !self.children.is_empty()
    }
}

const FIRM_NAV: &[NavLink] = &[
    NavLink::leaf("Foundation", "/foundation"),
    // One flat "Services" link — no dropdown. `/services` is the DB-backed
    // catalog: every product and its list price on one page, the price a
    // prospect sees being the same row Xero invoices. Each card links out
    // to the product's `/services/<slug>` detail page.
    NavLink::leaf("Services", "/services"),
];

const FOUNDATION_NAV: &[NavLink] = &[
    NavLink::leaf("Firm", "/"),
    // The Foundation publishes the open-source Navigator, Notations,
    // and training for lawyers who want to wield both:
    // "Navigator" (the software: the LSP, CLI, MCP, and web app, each its
    // own package page under the `/foundation/navigator` hub), "Notations"
    // (the legal blueprints), and "Workshops" (hands-on training). No
    // "Learn" catch-all dropdown, no separate Presentations surface — a
    // talk is just another workshop.
    NavLink::leaf("Navigator", "/foundation/navigator"),
    NavLink::leaf("Notations", "/foundation/notations"),
    NavLink::leaf("Workshops", "/foundation/workshops"),
];

/// Read an env var or fall back to the default. The returned slice
/// is leaked exactly once (per env-driven brand value) so consumers
/// can keep the existing `&'static str` shape on `SiteBrand`. One
/// leak per env-var read is harmless: total memory cost is at most
/// a few hundred bytes per process lifetime.
fn env_or_static(key: &str, default: &'static str) -> &'static str {
    match env::var(key) {
        Ok(v) if !v.is_empty() => Box::leak(v.into_boxed_str()),
        _ => default,
    }
}

/// Firm inbound email. Defaults to NeonLaw's real address; override
/// via `NAVIGATOR_SUPPORT_EMAIL` for an OSS fork that wants its own
/// routing. Resolved once per process.
#[must_use]
pub fn firm_email() -> &'static str {
    static FIRM_EMAIL: LazyLock<&'static str> =
        LazyLock::new(|| env_or_static("NAVIGATOR_SUPPORT_EMAIL", "support@neonlaw.com"));
    *FIRM_EMAIL
}

/// Firm consultation booking URL — the calendar where a prospective
/// client books a flat-fee consultation. This is the link behind every
/// firm "Book a Consultation" CTA (the home + service-page footers, the
/// service hero fallback, the firm contact card). Defaults to NeonLaw's
/// real Google Calendar appointment page; override via
/// `NAVIGATOR_CONSULTATION_URL` so an OSS fork points at its own
/// scheduler without forking source. Resolved once per process.
#[must_use]
pub fn consultation_url() -> &'static str {
    static URL: LazyLock<&'static str> = LazyLock::new(|| {
        env_or_static(
            "NAVIGATOR_CONSULTATION_URL",
            "https://calendar.app.google/GueqKHiAuqXEwkRG8",
        )
    });
    *URL
}

/// Where the footer's "Terms" link points. Defaults to the in-app
/// `/terms` page (NeonLaw's bundled terms of use). A white-label deploy
/// — a firm whose own marketing site already hosts its terms — sets
/// `NAVIGATOR_TERMS_URL` to that off-site URL so Navigator links out
/// instead of serving someone else's binding legal text. Same `Copy`-
/// friendly `&'static str` shape as the other brand links; resolved once
/// per process.
#[must_use]
pub fn terms_url() -> &'static str {
    static URL: LazyLock<&'static str> =
        LazyLock::new(|| env_or_static("NAVIGATOR_TERMS_URL", "/terms"));
    *URL
}

/// Where the footer's "Privacy" link points. Defaults to the in-app
/// `/privacy` page; override with `NAVIGATOR_PRIVACY_URL` to point at a
/// deployer's own hosted privacy policy. See [`terms_url`].
#[must_use]
pub fn privacy_url() -> &'static str {
    static URL: LazyLock<&'static str> =
        LazyLock::new(|| env_or_static("NAVIGATOR_PRIVACY_URL", "/privacy"));
    *URL
}

/// Foundation inbound email. Defaults to NeonLaw Foundation's real
/// address (the `.org` mirror of the firm's `.com`); override via
/// `NAVIGATOR_FOUNDATION_EMAIL`. Resolved once per process.
#[must_use]
pub fn foundation_email() -> &'static str {
    static FOUNDATION_EMAIL: LazyLock<&'static str> =
        LazyLock::new(|| env_or_static("NAVIGATOR_FOUNDATION_EMAIL", "support@neonlaw.org"));
    *FOUNDATION_EMAIL
}

/// Foundation GitHub URL — the open-source Navigator repository.
/// Canonical across deploys so public chrome always points at the
/// Foundation-owned source.
#[must_use]
pub const fn foundation_github_url() -> &'static str {
    "https://github.com/neon-law-foundation/Navigator"
}

/// The firm's legal-advice disclaimer, shown in the footer of every
/// page. The unified footer is firm-anchored, so the disclaimer is
/// always the firm's — it lives here as a single resolved string rather
/// than a per-brand `SiteBrand` field (the Foundation never carried its
/// own). Names the firm via [`FIRM_BRAND`]; resolved once per process.
#[must_use]
pub fn firm_disclaimer() -> &'static str {
    static DISCLAIMER: LazyLock<&'static str> = LazyLock::new(|| {
        Box::leak(
            format!(
                "Nothing on this site is legal advice. An attorney-client \
                 relationship begins only with a signed retainer between \
                 you and {}. Every legal matter is different, and past \
                 results do not guarantee a similar result.",
                FIRM_BRAND.site_name,
            )
            .into_boxed_str(),
        )
    });
    *DISCLAIMER
}

/// The deployed release — the `YY.MM.DD` ghcr tag this image was
/// published under, baked into the web image by `deploy.yml` as
/// `NAVIGATOR_RELEASE_TAG` (the same value `GET /version` reports as
/// `release`). Rendered in the footer so a push is visible end-to-end:
/// the moment a new image is live on the site, the footer's version
/// changes. `None` on a local `cargo run` (the env var is unset, or the
/// build honestly reports `unknown`), so dev never shows a bogus
/// version. Resolved once per process.
#[must_use]
pub fn deployed_release() -> Option<&'static str> {
    static RELEASE: LazyLock<Option<&'static str>> =
        LazyLock::new(|| match env::var("NAVIGATOR_RELEASE_TAG") {
            Ok(v) if v.is_empty() || v == "unknown" => None,
            Ok(v) => Some(&*Box::leak(v.into_boxed_str())),
            Err(_) => None,
        });
    *RELEASE
}

/// Law-firm brand. Name overridable via `NAVIGATOR_BRAND_FIRM`. The
/// default matches `NeonLaw`'s canonical deployment; OSS forks set the
/// env var to rebrand without forking source.
pub static FIRM_BRAND: LazyLock<SiteBrand> = LazyLock::new(|| {
    let name = env_or_static("NAVIGATOR_BRAND_FIRM", "Neon Law");
    // The registered mark belongs to NeonLaw's canonical deployment. If a
    // fork overrides the firm name, its name is not our trademark, so the
    // footer drops the linked `®`.
    let firm_name_overridden = matches!(env::var("NAVIGATOR_BRAND_FIRM"), Ok(v) if !v.is_empty());
    SiteBrand {
        site_name: name,
        tagline: "A small firm built for access to justice.",
        postal_address: env_or_static(
            "NAVIGATOR_FIRM_ADDRESS",
            "5150 Mae Anne Ave Ste 405-9002, Reno, NV 89523",
        ),
        logo_href: "/public/logo-firm.svg",
        social_image: "/public/logo-firm.png",
        nav: FIRM_NAV,
        is_law_firm: true,
        trademark_registration_url: if firm_name_overridden {
            None
        } else {
            // USPTO Reg. No. 6,325,650 — wordmark "NEON LAW", serial 90039224.
            Some("https://tmsearch.uspto.gov/search/search-results/90039224")
        },
    }
});

/// Foundation brand. Name overridable via `NAVIGATOR_BRAND_FOUNDATION`.
pub static FOUNDATION_BRAND: LazyLock<SiteBrand> = LazyLock::new(|| {
    let name = env_or_static("NAVIGATOR_BRAND_FOUNDATION", "Neon Law Foundation");
    SiteBrand {
        site_name: name,
        tagline: "Open-source access to justice and attorney AI training.",
        postal_address: env_or_static(
            "NAVIGATOR_FOUNDATION_ADDRESS",
            "5150 Mae Anne Ave Ste 405-9999, Reno, NV 89523",
        ),
        logo_href: "/public/logo-foundation.svg",
        social_image: "/public/logo-foundation.png",
        nav: FOUNDATION_NAV,
        is_law_firm: false,
        trademark_registration_url: None,
    }
});

#[cfg(test)]
mod tests {
    use super::{NavLink, FIRM_BRAND, FOUNDATION_BRAND};

    #[test]
    fn firm_brand_defaults_to_neon_law() {
        // Without `NAVIGATOR_BRAND_FIRM` set, the firm brand is
        // NeonLaw's canonical name. OSS forks override via the env.
        assert!(
            FIRM_BRAND.site_name == "Neon Law" || std::env::var("NAVIGATOR_BRAND_FIRM").is_ok(),
            "default firm name should be 'Neon Law' unless overridden by env"
        );
        assert!(FIRM_BRAND.is_law_firm);
    }

    #[test]
    fn foundation_brand_is_not_a_law_firm() {
        assert!(!FOUNDATION_BRAND.is_law_firm);
    }

    #[test]
    fn each_brand_carries_a_distinct_raster_social_image() {
        // The Open Graph card needs a PNG (scrapers won't render the
        // SVG favicon) and each brand shares its own mark.
        let is_png = |path: &str| {
            std::path::Path::new(path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        };
        assert!(is_png(FIRM_BRAND.social_image));
        assert!(is_png(FOUNDATION_BRAND.social_image));
        assert_ne!(FIRM_BRAND.social_image, FOUNDATION_BRAND.social_image);
    }

    #[test]
    fn firm_and_foundation_carry_distinct_suite_addresses() {
        // Same street, distinct private-mailbox suites; unless an OSS
        // fork overrides them via env, these are NeonLaw's registered
        // addresses (firm 405-9002, Foundation 405-9999).
        assert!(
            FIRM_BRAND.postal_address.contains("405-9002")
                || std::env::var("NAVIGATOR_FIRM_ADDRESS").is_ok(),
            "firm address should carry suite 405-9002 unless overridden"
        );
        assert!(
            FOUNDATION_BRAND.postal_address.contains("405-9999")
                || std::env::var("NAVIGATOR_FOUNDATION_ADDRESS").is_ok(),
            "foundation address should carry suite 405-9999 unless overridden"
        );
    }

    #[test]
    fn firm_disclaimer_names_the_firm_and_disclaims_advice() {
        let disclaimer = super::firm_disclaimer();
        assert!(disclaimer.contains(FIRM_BRAND.site_name));
        assert!(disclaimer.contains("Nothing on this site is legal advice"));
        assert!(disclaimer.contains("signed retainer"));
        assert!(disclaimer.contains("past results do not guarantee a similar result"));
    }

    #[test]
    fn firm_nav_services_is_a_flat_link_to_the_catalog() {
        // The Services dropdown was collapsed to a single flat link: one
        // `/services` page that is the DB-backed product catalog. Each card
        // there links out to its `/services/<slug>` detail page.
        let services = FIRM_BRAND
            .nav
            .iter()
            .find(|n| n.label == "Services")
            .expect("Services link present");
        assert!(
            !services.is_dropdown(),
            "Services is a flat leaf, no dropdown"
        );
        assert_eq!(services.href, "/services");
    }

    #[test]
    fn firm_top_nav_starts_with_foundation_cross_link() {
        let labels: Vec<&str> = FIRM_BRAND.nav.iter().map(|n| n.label).collect();
        assert_eq!(labels, ["Foundation", "Services"]);
        assert_eq!(FIRM_BRAND.nav[0].href, "/foundation");
    }

    #[test]
    fn foundation_top_nav_is_four_flat_leaves() {
        let labels: Vec<&str> = FOUNDATION_BRAND.nav.iter().map(|n| n.label).collect();
        // The Foundation nav stays terse: firm cross-link, software,
        // Notations, and training.
        assert_eq!(labels, ["Firm", "Navigator", "Notations", "Workshops"]);
        assert_eq!(FOUNDATION_BRAND.nav[0].href, "/");
        assert!(
            FOUNDATION_BRAND.nav.iter().all(|n| !n.is_dropdown()),
            "the Foundation nav no longer carries any dropdown"
        );
    }

    #[test]
    fn foundation_nav_navigator_points_at_the_package_hub() {
        // "Navigator" is a flat top-level leaf at the hub that fans out to
        // the per-package pages (lsp / cli / mcp / web).
        let navigator = FOUNDATION_BRAND
            .nav
            .iter()
            .find(|n| n.label == "Navigator")
            .expect("Navigator leaf present");
        assert!(!navigator.is_dropdown());
        assert_eq!(navigator.href, "/foundation/navigator");
    }

    #[test]
    fn foundation_nav_notations_points_at_the_readme_page() {
        let templates = FOUNDATION_BRAND
            .nav
            .iter()
            .find(|n| n.label == "Notations")
            .expect("Notations leaf present");
        assert!(!templates.is_dropdown());
        assert_eq!(templates.href, "/foundation/notations");
    }

    #[test]
    fn foundation_nav_workshops_points_at_the_top_level_overview() {
        let workshops = FOUNDATION_BRAND
            .nav
            .iter()
            .find(|n| n.label == "Workshops")
            .expect("Workshops leaf present");
        assert!(!workshops.is_dropdown());
        assert_eq!(workshops.href, "/foundation/workshops");
    }

    #[test]
    fn foundation_nav_has_no_presentations_or_learn_surface() {
        // Presentations folded into Workshops; the "Learn" catch-all is gone.
        assert!(
            !FOUNDATION_BRAND
                .nav
                .iter()
                .any(|n| n.label == "Learn" || n.label == "Presentations"),
            "Learn / Presentations must not appear in the Foundation nav"
        );
    }

    #[test]
    fn foundation_email_defaults_to_neonlaw_org() {
        assert!(
            super::foundation_email() == "support@neonlaw.org"
                || std::env::var("NAVIGATOR_FOUNDATION_EMAIL").is_ok(),
            "default foundation email should be support@neonlaw.org unless overridden"
        );
    }

    #[test]
    fn firm_email_defaults_to_neonlaw_com() {
        assert!(
            super::firm_email() == "support@neonlaw.com"
                || std::env::var("NAVIGATOR_SUPPORT_EMAIL").is_ok(),
            "default firm email should be support@neonlaw.com unless overridden"
        );
    }

    #[test]
    fn terms_and_privacy_links_default_to_the_in_app_pages() {
        // Unset → the bundled `/terms` and `/privacy` routes. A white-label
        // deploy overrides these to its own off-site legal pages so
        // Navigator never serves a deployer's binding legal text.
        assert!(
            super::terms_url() == "/terms" || std::env::var("NAVIGATOR_TERMS_URL").is_ok(),
            "terms link defaults to /terms unless overridden"
        );
        assert!(
            super::privacy_url() == "/privacy" || std::env::var("NAVIGATOR_PRIVACY_URL").is_ok(),
            "privacy link defaults to /privacy unless overridden"
        );
    }

    #[test]
    fn leaf_constructor_yields_no_children() {
        let n = NavLink::leaf("Home", "/");
        assert!(!n.is_dropdown());
        assert_eq!(n.href, "/");
    }

    #[test]
    fn dropdown_constructor_carries_children() {
        const CHILDREN: &[NavLink] = &[NavLink::leaf("A", "/a")];
        let n = NavLink::dropdown("Group", CHILDREN);
        assert!(n.is_dropdown());
        assert_eq!(n.children.len(), 1);
    }
}
