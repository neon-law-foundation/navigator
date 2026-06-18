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
    /// When true, the layout renders the "not accepting clients" banner
    /// above the header. Foundation pages (the 501(c)(3) doesn't
    /// practice law) leave this false. The legal-advice disclaimer is no
    /// longer gated here — the unified footer always shows the firm's via
    /// [`firm_disclaimer`].
    pub is_law_firm: bool,
    /// Banner above the header on firm-branded pages.
    pub firm_not_accepting_clients: &'static str,
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
    NavLink::leaf("Home", "/"),
    // One flat "Services" link — no dropdown. `/services` is the DB-backed
    // catalog: every product and its list price on one page, the price a
    // prospect sees being the same row Xero invoices. Each card links out
    // to the product's `/services/<slug>` detail page.
    NavLink::leaf("Services", "/services"),
];

const FOUNDATION_NAV: &[NavLink] = &[
    NavLink::leaf("Mission", "/foundation/mission"),
    // Workshops + Presentations + Navigator share one dropdown so the top
    // nav stays compact and never runs off the edge on desktop — the
    // same pattern the firm nav uses for "Services".
    NavLink::dropdown(
        "Learn",
        &[
            NavLink::leaf("Workshops", "/foundation/workshops/navigator"),
            NavLink::leaf("Presentations", "/foundation/presentations"),
            NavLink::leaf("Navigator", "/navigator"),
            // "Editor plugin" is the reader-facing label for the
            // `navigator-lsp` language server. It sits beside Navigator
            // — the standard — because the LSP is how you get that
            // standard's rules into your editor. The leaf points at the
            // existing install/download page (`/lsp`), which
            // hands you the binary and the per-editor setup. Plain words
            // over "LSP": a reader who edits markdown knows "editor
            // plugin," not the acronym.
            NavLink::leaf("Editor plugin", "/lsp"),
            // Nimbus is the done-for-you sibling of the self-host story:
            // Workshops + Navigator teach you to run it yourself; Nimbus is
            // the Foundation installing it on your own cloud and training
            // your team. It sits beside them rather than in a fourth
            // top-level item so the mobile nav stays at three.
            NavLink::leaf("Nimbus", "/foundation/nimbus"),
        ],
    ),
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

/// Foundation GitHub URL — the open-source Navigator repository. Defaults
/// to NeonLaw Foundation's real repo; set `NAVIGATOR_FOUNDATION_GITHUB_URL`
/// to a fork's repo, or to `""` to suppress the GitHub call-to-action
/// entirely. Single source for both the `/foundation` hero and the
/// `/foundation/contact` card. Resolved once per process.
#[must_use]
pub fn foundation_github_url() -> Option<&'static str> {
    static URL: LazyLock<Option<&'static str>> =
        LazyLock::new(|| match env::var("NAVIGATOR_FOUNDATION_GITHUB_URL") {
            Ok(v) if v.is_empty() => None,
            Ok(v) => Some(&*Box::leak(v.into_boxed_str())),
            Err(_) => Some("https://github.com/neon-law-foundation/Navigator"),
        });
    *URL
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

/// Law-firm brand. Name overridable via `NAVIGATOR_BRAND_FIRM`. The
/// default matches `NeonLaw`'s canonical deployment; OSS forks set the
/// env var to rebrand without forking source.
pub static FIRM_BRAND: LazyLock<SiteBrand> = LazyLock::new(|| {
    let name = env_or_static("NAVIGATOR_BRAND_FIRM", "Neon Law");
    // The registered mark belongs to NeonLaw's canonical deployment. If a
    // fork overrides the firm name, its name is not our trademark, so the
    // footer drops the linked `®`.
    let firm_name_overridden = matches!(env::var("NAVIGATOR_BRAND_FIRM"), Ok(v) if !v.is_empty());
    let banner = Box::leak(format!("{name} is currently not accepting clients.").into_boxed_str());
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
        firm_not_accepting_clients: banner,
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
        firm_not_accepting_clients: "",
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
    fn firm_not_accepting_clients_banner_states_the_firm_is_closed() {
        assert!(FIRM_BRAND
            .firm_not_accepting_clients
            .contains(FIRM_BRAND.site_name));
        assert!(FIRM_BRAND
            .firm_not_accepting_clients
            .contains("not accepting clients"));
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
    fn firm_top_nav_holds_three_visible_items_for_mobile() {
        let labels: Vec<&str> = FIRM_BRAND.nav.iter().map(|n| n.label).collect();
        assert_eq!(labels, ["Home", "Services"]);
    }

    #[test]
    fn foundation_top_nav_surfaces_mission_and_learn() {
        let labels: Vec<&str> = FOUNDATION_BRAND.nav.iter().map(|n| n.label).collect();
        // Workshops + Presentations + Navigator live inside the "Learn"
        // dropdown so the visible top-level count stays compact.
        assert_eq!(labels, ["Mission", "Learn"]);
    }

    #[test]
    fn foundation_nav_workshops_points_at_single_canonical_workshop() {
        let learn = FOUNDATION_BRAND
            .nav
            .iter()
            .find(|n| n.label == "Learn")
            .expect("Learn dropdown present");
        assert!(
            learn.is_dropdown(),
            "Learn groups Workshops + Presentations"
        );
        let workshops = learn
            .children
            .iter()
            .find(|n| n.label == "Workshops")
            .expect("Workshops leaf inside Learn");
        assert!(!workshops.is_dropdown());
        assert_eq!(workshops.href, "/foundation/workshops/navigator");
    }

    #[test]
    fn foundation_nav_learn_dropdown_surfaces_presentations() {
        let learn = FOUNDATION_BRAND
            .nav
            .iter()
            .find(|n| n.label == "Learn")
            .expect("Learn dropdown present");
        let presentations = learn
            .children
            .iter()
            .find(|n| n.label == "Presentations")
            .expect("Presentations leaf inside Learn");
        assert!(!presentations.is_dropdown());
        assert_eq!(presentations.href, "/foundation/presentations");
    }

    #[test]
    fn foundation_nav_nests_navigator_inside_learn() {
        let learn = FOUNDATION_BRAND
            .nav
            .iter()
            .find(|n| n.label == "Learn")
            .expect("Learn dropdown present");
        let navigator = learn
            .children
            .iter()
            .find(|n| n.label == "Navigator")
            .expect("Navigator leaf inside Learn");
        assert!(!navigator.is_dropdown());
        assert_eq!(navigator.href, "/navigator");
    }

    #[test]
    fn foundation_nav_offers_editor_plugin_download_under_learn() {
        // The navigator-lsp language server is reachable from "Learn" as
        // "Editor plugin" — plain copy over the "LSP" acronym — and links
        // to the existing install/download page at `/lsp`.
        let learn = FOUNDATION_BRAND
            .nav
            .iter()
            .find(|n| n.label == "Learn")
            .expect("Learn dropdown present");
        let plugin = learn
            .children
            .iter()
            .find(|n| n.label == "Editor plugin")
            .expect("Editor plugin leaf inside Learn");
        assert!(!plugin.is_dropdown());
        assert_eq!(plugin.href, "/lsp");
    }

    #[test]
    fn foundation_nav_offers_nimbus_install_under_learn() {
        // Nimbus (the white-label two-week install) lives inside "Learn"
        // beside Navigator + Workshops — the self-host family — so the
        // top nav stays at three visible items.
        let learn = FOUNDATION_BRAND
            .nav
            .iter()
            .find(|n| n.label == "Learn")
            .expect("Learn dropdown present");
        let nimbus = learn
            .children
            .iter()
            .find(|n| n.label == "Nimbus")
            .expect("Nimbus leaf inside Learn");
        assert!(!nimbus.is_dropdown());
        assert_eq!(nimbus.href, "/foundation/nimbus");
    }

    #[test]
    fn foundation_nav_surfaces_mission_page() {
        assert!(FOUNDATION_BRAND
            .nav
            .iter()
            .any(|n| n.label == "Mission" && n.href == "/foundation/mission"));
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
