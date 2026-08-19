//! The public-page chrome resolved from the process brand — the reusable
//! header + footer data every ported public marketing page needs (issue #641 /
//! #730 PR6).
//!
//! Extracted from the first page port (`team_nick`) once a second page needed
//! the same header nav + footer legal strip. The DTOs are wasm-safe (plain
//! serde); the resolver reads `views::brand` and so is server-only, called from
//! each page's `#[server]` view function.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{
    FooterAttorney, FooterBarLicense, FooterNavLink, FooterOffice, SiteFooterLegal,
};

/// One nav destination, resolved from the brand for the header.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ChromeNavLink {
    pub label: String,
    pub href: String,
}

/// The header's auth-aware utility links (Portal / role-gated Lawyer·Admin /
/// Sign out for a signed-in viewer, or Sign in for an anonymous visitor on a
/// law-firm brand), resolved from the request session. The wasm-safe request
/// extension the portal router injects (`portal::dioxus_app`), read by each
/// page's `#[server]` view function — the session lives on `portal`'s
/// `SessionData`, which `webapp` cannot see. Empty when no session and the
/// brand is not a law firm.
#[derive(Clone, Default)]
pub struct PublicUtility(pub Vec<ChromeNavLink>);

/// One published office, resolved from the brand for the footer.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ChromeOffice {
    pub state: String,
    pub address: String,
    /// A qualification published under the address, e.g. an admission that has
    /// not issued yet. Mirrors `views::brand::FirmOffice::note`.
    pub note: Option<String>,
}

/// One bar license, resolved from the brand for the footer.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ChromeBarLicense {
    pub jurisdiction: String,
    pub number: String,
    pub license_url: String,
}

/// One licensed attorney and their bar licenses, resolved from the brand.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ChromeAttorney {
    pub name: String,
    pub licenses: Vec<ChromeBarLicense>,
}

/// The public-page chrome: everything the [`crate::components::SiteHeader`] and
/// [`crate::components::SiteFooterLegal`] need, resolved from the process brand
/// per request so the wasm client never links the view layer.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct PublicChrome {
    pub brand_name: String,
    pub home_href: String,
    pub logo_href: String,
    /// The absolute URL of the brand's raster mark, for `og:image` — scrapers
    /// drop relative image URLs, so it is resolved against the site origin on
    /// the server.
    pub social_image: String,
    pub destinations: Vec<ChromeNavLink>,
    pub utility: Vec<ChromeNavLink>,
    /// The public pages the footer links rather than the header — Navigator,
    /// Blog, and Contact. Empty on Foundation chrome, which renders its own
    /// footer.
    pub footer_links: Vec<ChromeNavLink>,
    pub firm_name: String,
    pub foundation_name: String,
    /// The legal person the footer's copyright names. Empty on a non-firm
    /// deploy, which falls back to its own wordmark.
    pub legal_entity: String,
    pub disclaimer: String,
    pub copyright_year: i32,
    /// The firm's inbound support address — the footer's contact CTA.
    pub firm_email: String,
    /// The firm's published voice line, dialled from the footer's `tel:` link.
    pub firm_phone: String,
    /// Every office the firm publishes, in the order the brand gives them.
    pub offices: Vec<ChromeOffice>,
    /// The firm's attorneys and the bar licenses each holds.
    pub attorneys: Vec<ChromeAttorney>,
    /// The firm's own registered address — distinct from `offices`, the
    /// walk-in locations. Renders at the very bottom of the footer beside
    /// `foundation_postal_address`.
    pub firm_postal_address: String,
    /// The Foundation's own registered address, the other half of that
    /// closing line.
    pub foundation_postal_address: String,
    /// The public repository the platform is developed in, as both footers
    /// publish it: the `owner/name` a reader sees and the address it links to.
    /// Constants rather than brand fields, for the same reason the platform
    /// line names Neon Law Navigator outright — a white-label deployment wears
    /// its own wordmark but runs this software, developed here.
    pub source_repo: String,
    pub source_href: String,
    /// How many people have starred that repository, or `None` when the
    /// process has not fetched it yet.
    ///
    /// Read from `crate::source_repository`'s cache when the chrome is built,
    /// so a page render never waits on GitHub. `None` is the ordinary state
    /// before the first refresh lands and whenever GitHub is unreachable; the
    /// footer publishes the repository link without a number.
    pub source_stars: Option<u64>,
}

/// The public footer, mapped from an already-resolved [`PublicChrome`].
///
/// ONE footer, on every page of both faces. [`crate::components::SiteFooterLegal`]
/// is the only footer this renders; what differs between the two faces is the
/// DATA: [`foundation_public_chrome`] clears the firm's regulated fields, so a
/// Foundation page renders no bar number, no firm office, and no firm inbox
/// even though it renders the same component. Clearing beats not-rendering —
/// data that isn't there cannot leak through a page that reaches for it.
///
/// [`crate::components::SiteFooterFoundation`] survives as the standalone
/// component the design gallery drives with literal props; nothing routes to it.
///
/// Pages call `rsx! { PublicFooter { chrome } }` and pass the result to
/// [`crate::components::PublicShell`].
#[component]
pub fn PublicFooter(chrome: PublicChrome) -> Element {
    rsx! {
        // The firm's colour, hoisted here because this is the one place that
        // already knows which brand the page wears. Navigator's shared tokens
        // are teal and the Foundation keeps them; the firm is orange. Loading
        // it on this branch means every firm page gets the palette — including
        // `/contact`, `/blog`, and the legal pages, which hoist no marketing
        // layer — and no Foundation page can pick it up by accident.
        document::Stylesheet { href: crate::brand_style::BRAND_TOKENS_HREF }
        SiteFooterLegal {
            // The copyright names the legal person, which on the firm's deploy
            // is the entity that renders the legal services (Neon Law).
            // A deploy that names no legal entity has nothing better to notice
            // than its own brand, so it falls back to that.
            copyright_holder: if chrome.legal_entity.is_empty() {
                chrome.firm_name.clone()
            } else {
                chrome.legal_entity.clone()
            },
            disclaimer: chrome.disclaimer.clone(),
            copyright_year: chrome.copyright_year,
            logo_href: chrome.logo_href.clone(),
            // The wordmark beside the footer mark is the one the page trades
            // under, not the copyright holder: this is the brand saying whose
            // page this is, and the line naming the legal person is below.
            brand_name: chrome.brand_name.clone(),
            contact_email: chrome.firm_email.clone(),
            phone: chrome.firm_phone.clone(),
            offices: chrome
                .offices
                .iter()
                .map(|office| FooterOffice {
                    state: office.state.clone(),
                    address: office.address.clone(),
                    note: office.note.clone(),
                })
                .collect(),
            nav: chrome
                .footer_links
                .iter()
                .map(|link| FooterNavLink {
                    label: link.label.clone(),
                    href: link.href.clone(),
                })
                .collect(),
            attorneys: chrome
                .attorneys
                .iter()
                .map(|attorney| FooterAttorney {
                    name: attorney.name.clone(),
                    licenses: attorney
                        .licenses
                        .iter()
                        .map(|license| FooterBarLicense {
                            jurisdiction: license.jurisdiction.clone(),
                            number: license.number.clone(),
                            license_url: license.license_url.clone(),
                        })
                        .collect(),
                })
                .collect(),
            foundation: chrome.foundation_name.clone(),
            firm_postal_address: chrome.firm_postal_address.clone(),
            foundation_postal_address: chrome.foundation_postal_address.clone(),
            source_repo: chrome.source_repo.clone(),
            source_href: chrome.source_href.clone(),
            source_stars: chrome.source_stars,
        }
    }
}

/// Resolve the firm host's public chrome from the process brand. Server-only
/// (`views::brand` does not compile to wasm); each page's `#[server]` view
/// function calls this, and the macro stubs those bodies for the wasm client.
///
/// `utility` is the auth-aware header utility group the portal router resolves
/// from the request session (see [`PublicUtility`]) and the page's server
/// function passes through; it is empty for an anonymous visitor on a
/// non-firm brand.
///
#[cfg(feature = "server")]
#[must_use]
pub fn firm_public_chrome(utility: Vec<ChromeNavLink>) -> PublicChrome {
    chrome_for(&views::brand::FIRM_BRAND, utility)
}

/// Resolve the Foundation's public chrome from the process brand — the 501(c)(3)'s
/// header identity, on the one shared footer.
///
/// The header differs as it always did: the wordmark, the logo, the home link
/// (`/foundation`, not `/`), the social image, and the destination nav. The
/// two entities are separate, so the Foundation no longer borrows the firm's
/// regulated footer copy: no bar admissions, no per-attorney bar numbers, and
/// no firm street address.
#[cfg(feature = "server")]
#[must_use]
pub fn foundation_public_chrome(utility: Vec<ChromeNavLink>) -> PublicChrome {
    use views::brand::FOUNDATION_BRAND;

    let mut chrome = chrome_for(&FOUNDATION_BRAND, utility);
    // Strip the firm's REGULATED footer data rather than merely declining to
    // render it: the entity of record, the bar licences, and the firm's street
    // addresses. Data that isn't there cannot leak through a page that reaches
    // for the firm's footer directly, which matters more now that both faces
    // render the same footer component.
    //
    // `firm_name` survives because the joint copyright names both entities, and
    // the contact band survives too — an inbox and a phone number are not
    // regulated disclosures, and the firm is the entity a visitor on either
    // face calls or writes to.
    chrome.legal_entity = String::new();
    chrome.attorneys = Vec::new();
    chrome.offices = Vec::new();
    chrome
}

/// Build the public chrome for `brand`'s header, with the firm's footer.
///
/// `brand` supplies the header half (wordmark, logo, home link, social image,
/// destinations). The footer half always reads the firm brand, because this
/// builds the *firm's* footer; [`foundation_public_chrome`] replaces it
/// wholesale with the Foundation's own afterwards.
#[cfg(feature = "server")]
fn chrome_for(brand: &views::brand::SiteBrand, utility: Vec<ChromeNavLink>) -> PublicChrome {
    use views::brand::FIRM_BRAND;

    let destinations = brand
        .nav
        .iter()
        .map(|link| ChromeNavLink {
            label: link.label.to_string(),
            href: link.href.to_string(),
        })
        .collect();

    PublicChrome {
        brand_name: brand.site_name.to_string(),
        home_href: brand.home_href.to_string(),
        logo_href: brand.logo_href.to_string(),
        social_image: views::assets::absolute_url(brand.social_image),
        destinations,
        utility,
        footer_links: views::brand::firm_footer_nav()
            .iter()
            .map(|link| ChromeNavLink {
                label: link.label.to_string(),
                href: link.href.to_string(),
            })
            .collect(),
        firm_name: FIRM_BRAND.site_name.to_string(),
        // The corporation, not the wordmark: the firm's footer names the
        // nonprofit as a legal person, the same way the nonprofit's footer
        // names the partnership rather than "Neon Law".
        foundation_name: views::brand::foundation_entity().to_string(),
        legal_entity: FIRM_BRAND.legal_entity.to_string(),
        disclaimer: views::brand::firm_disclaimer().to_string(),
        // The footer fixes the joint-copyright year too; a deploy-time
        // value replaces the constant when the footer year is wired through.
        copyright_year: 2026,
        // The contact band is firm-anchored for the same reason the legal strip
        // is: one footer serves both brands, and the firm is the entity a
        // visitor calls, writes to, or walks in on.
        firm_email: views::brand::firm_email().to_string(),
        firm_phone: views::brand::firm_phone().to_string(),
        offices: views::brand::firm_offices()
            .iter()
            .map(|office| ChromeOffice {
                state: office.state.to_string(),
                address: office.address.to_string(),
                note: office.note.map(str::to_string),
            })
            .collect(),
        attorneys: views::brand::firm_attorneys()
            .iter()
            .map(|attorney| ChromeAttorney {
                name: attorney.name.to_string(),
                licenses: attorney
                    .licenses
                    .iter()
                    .map(|license| ChromeBarLicense {
                        jurisdiction: license.jurisdiction.to_string(),
                        number: license.number.to_string(),
                        license_url: license.license_url.to_string(),
                    })
                    .collect(),
            })
            .collect(),
        firm_postal_address: FIRM_BRAND.postal_address.to_string(),
        foundation_postal_address: views::brand::FOUNDATION_BRAND.postal_address.to_string(),
        // The repository the platform is developed in. Both faces publish it:
        // the code is open source under either wordmark, and the Foundation is
        // the org that owns the repository.
        //
        // The star count is a CACHE READ, deliberately. This function runs in
        // the request path — once per public page render — so it must not
        // reach the network: `source_repository::spawn_refresh` keeps the value
        // current from a background task, and an empty cache renders the
        // repository link with no number rather than delaying the page.
        source_repo: crate::source_repository::REPOSITORY_SLUG.to_string(),
        source_href: crate::source_repository::REPOSITORY_HREF.to_string(),
        source_stars: crate::source_repository::star_count(),
    }
}

/// Resolve the firm host's public chrome, reading the auth-aware utility group
/// from the request's injected [`PublicUtility`] extension (empty when the
/// portal router injected none). The single entry point each public page's
/// `#[server]` view function calls.
#[cfg(feature = "server")]
pub async fn firm_public_chrome_from_context() -> PublicChrome {
    // Prefer the chrome the portal pre-layer (`inject_public_utility`) resolved
    // on the request task, where the brand `task_local` is live — this server-fn
    // runs on a task that does not inherit it, so building the chrome here would
    // read the default brand under a mounted white-label bundle. Fall back to
    // building it from context if the extension is absent.
    if let Ok(axum::Extension(chrome)) =
        dioxus_fullstack_core::FullstackContext::extract::<axum::Extension<PublicChrome>, _>().await
    {
        return chrome;
    }
    let utility =
        dioxus_fullstack_core::FullstackContext::extract::<axum::Extension<PublicUtility>, _>()
            .await
            .map_or_else(|_| Vec::new(), |axum::Extension(utility)| utility.0);
    firm_public_chrome(utility)
}

/// Resolve the Foundation host's public chrome from the request context — the
/// Foundation twin of [`firm_public_chrome_from_context`], and the single entry
/// point each Foundation page's `#[server]` view function calls.
///
/// The portal pre-layer (`inject_foundation_chrome`) resolves the chrome on the
/// request task, where the brand `task_local` is live, and injects it; this
/// server-fn runs on a task that does not inherit it, so the extension is the
/// authority and building it here is only the fallback.
#[cfg(feature = "server")]
pub async fn foundation_public_chrome_from_context() -> PublicChrome {
    if let Ok(axum::Extension(chrome)) =
        dioxus_fullstack_core::FullstackContext::extract::<axum::Extension<PublicChrome>, _>().await
    {
        return chrome;
    }
    let utility =
        dioxus_fullstack_core::FullstackContext::extract::<axum::Extension<PublicUtility>, _>()
            .await
            .map_or_else(|_| Vec::new(), |axum::Extension(utility)| utility.0);
    foundation_public_chrome(utility)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssr(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    /// A firm chrome fixture carrying the firm's regulated footer copy.
    fn firm_chrome() -> PublicChrome {
        PublicChrome {
            logo_href: "/public/logo-firm.svg".to_string(),
            firm_name: "Neon Law".to_string(),
            foundation_name: "Neon Law".to_string(),
            legal_entity: "Neon Law".to_string(),
            disclaimer: "This is an attorney advertisement.".to_string(),
            copyright_year: 2026,
            firm_email: "support@neonlaw.com".to_string(),
            firm_phone: "+1 555 010 0100".to_string(),
            attorneys: vec![ChromeAttorney {
                name: "Ada Lovelace".to_string(),
                licenses: vec![ChromeBarLicense {
                    jurisdiction: "California".to_string(),
                    number: "100001".to_string(),
                    license_url: "https://example.com/bar/100001".to_string(),
                }],
            }],
            ..PublicChrome::default()
        }
    }

    /// The same chrome with the Foundation's header identity — what
    /// `foundation_public_chrome` produces.
    fn foundation_chrome() -> PublicChrome {
        PublicChrome {
            // What `foundation_public_chrome` clears, cleared here too. The
            // fixture has to model the resolver: both faces render the same
            // footer component now, so a fixture that left the firm's
            // regulated data in place would prove nothing about the page the
            // resolver actually builds — it would only prove the component
            // ignores it, which it no longer does.
            legal_entity: String::new(),
            attorneys: Vec::new(),
            offices: Vec::new(),
            ..firm_chrome()
        }
    }

    /// Firm chrome renders the firm's footer: the copyright that names the
    /// regulated entity, and the firm's own contact channels.
    #[test]
    fn firm_chrome_renders_the_firm_footer() {
        fn app() -> Element {
            rsx! { PublicFooter { chrome: firm_chrome() } }
        }
        let out = ssr(app);
        assert!(out.contains("\u{a9} 2026 Neon Law"), "{out}");
        assert!(out.contains("mailto:support@neonlaw.com"), "{out}");
        assert!(
            out.contains(r#"class="site-footer__logo" src="/public/logo-firm.svg" alt="""#),
            "the firm footer carries the header mark: {out}"
        );
        assert!(!out.contains("site-footer--foundation"), "{out}");
    }

    /// The firm footer's copyright names the legal person, not the wordmark:
    /// "Neon Law" is a brand and cannot hold a copyright. A deploy that
    /// names no legal entity has only its brand to notice, so it falls back.
    #[test]
    fn the_firm_copyright_names_the_legal_entity() {
        fn app() -> Element {
            rsx! { PublicFooter { chrome: firm_chrome() } }
        }
        fn unincorporated() -> Element {
            rsx! {
                PublicFooter {
                    chrome: PublicChrome {
                        legal_entity: String::new(),
                        ..firm_chrome()
                    },
                }
            }
        }
        let out = ssr(app);
        assert!(out.contains("© 2026 Neon Law"), "{out}");
        assert!(ssr(unincorporated).contains("© 2026 Neon Law"));
    }

    /// Foundation chrome renders the one shared footer — and none of the
    /// firm's REGULATED copy.
    ///
    /// There is no footer swap any more. Both faces render
    /// [`crate::components::SiteFooterLegal`]; what differs is the data the
    /// chrome resolver puts in it. That is a stronger guarantee than the swap
    /// was, not a weaker one: a new Foundation page cannot ship a bar number by
    /// forgetting to opt out, because the number is not in its chrome at all.
    #[test]
    fn foundation_chrome_renders_the_shared_footer_without_the_firms_regulated_copy() {
        fn app() -> Element {
            rsx! { PublicFooter { chrome: foundation_chrome() } }
        }
        let out = ssr(app);
        // One footer, and it is the shared one.
        assert!(
            !out.contains("site-footer--foundation"),
            "the Foundation-only footer is retired: {out}"
        );
        assert_eq!(
            out.matches(r#"role="contentinfo""#).count(),
            1,
            "exactly one footer landmark: {out}"
        );
        // The firm's REGULATED copy must not travel onto this face: a bar
        // admission or a bar number here would read as the Foundation holding
        // a licence, which is the confusion the two-entity split exists to
        // remove.
        for firm_only in ["Admitted in", "Bar No."] {
            assert!(
                !out.contains(firm_only),
                "Foundation chrome must not carry {firm_only:?}: {out}"
            );
        }
        // The firm's inbox is NOT in that set. The contact band is shared and
        // firm-anchored on both faces — an email address is not a regulated
        // disclosure, and the firm is who a visitor on either page writes to.
        assert!(
            out.contains("mailto:support@neonlaw.com"),
            "the shared contact band still reaches the firm: {out}"
        );
    }

    /// The firm's face renders the one shared footer, with the firm's own
    /// disclaimer.
    #[test]
    fn the_firm_face_carries_its_disclaimer() {
        fn app() -> Element {
            rsx! { PublicFooter { chrome: firm_chrome() } }
        }
        let out = ssr(app);
        assert_eq!(
            out.matches(r#"role="contentinfo""#).count(),
            1,
            "exactly one footer landmark: {out}"
        );
        assert!(
            out.contains("attorney advertisement") || out.contains("attorney advertising"),
            "the firm's own disclaimer is there: {out}"
        );
    }
}
