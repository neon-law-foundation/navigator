//! The Foundation's own site footer, as a Dioxus component.
//!
//! A separate footer from the firm's [`crate::components::SiteFooterLegal`],
//! not a variant of it. The two entities are separate: the firm practises law
//! and owes regulated attorney-advertising copy, bar admissions, and
//! per-attorney licence numbers; the 501(c)(3) practises none and owes the
//! opposite assurance — that it cannot represent you.
//!
//! Legal-council reviewed (Capricorn + Scorpio). Three rules the copy encodes,
//! none of which may be quietly dropped:
//!
//! 1. **It identifies the corporation, not the wordmark.** `entity` is
//!    "Neon Law Foundation"; `views::brand::FOUNDATION_BRAND.site_name` is the
//!    brand "Neon Law" the header wears. A footer that names the legal person
//!    must not print a brand there.
//! 2. **It states the negative affirmatively.** "Nothing here is legal advice"
//!    is too weak on its own for a nonprofit that publishes legal templates and
//!    an AI assistant; the identity line says outright that the Foundation does
//!    not practise law and cannot represent the reader.
//! 3. **It notices no trademark and no bar licence.** The Foundation uses the
//!    NEON LAW mark under written permission from the firm rather than owning
//!    it, and it holds no bar admission — showing the firm's here would restore
//!    exactly the confusion this footer exists to remove.
//! 4. **It names the firm that supports it, and nothing more of the firm.**
//!    The Foundation discloses that Neon Law supports it and built this
//!    platform, by name and by link. That is a funding-and-authorship
//!    disclosure, not an advertisement: it says who stands behind the
//!    nonprofit, which a reader deciding whether to trust it is entitled to
//!    know. Rule 3 still binds everything else — naming the firm must not drag
//!    in its bar numbers, its admissions, or its trademark notice, and
//!    `naming_the_firm_pulls_in_none_of_its_regulated_disclosures` fails the
//!    build if it does. The line renders *after* "cannot represent you" so the
//!    two facts are read together.
//!
//! All four rules are settled by legal-council review. Rule 4 was added after
//! the first bench and reviewed by a second, which confirmed the
//! funding-and-authorship framing and the order: the disclosure names who
//! funds and who built, links so the claim is checkable, recommends no legal
//! service, and carries none of the firm's regulated disclosures. The
//! Foundation-to-firm direction is the only one of the two with private-benefit
//! exposure, and it stays clear of it precisely because it is disclosure rather
//! than a call to action — it must never become one.
//!
//! The platform attribution added later links onward to the firm's `/navigator`
//! page, which does carry a commercial offer. Reviewed and accepted: a
//! nonprofit buying or running services from a company is ordinary, and naming
//! the vendor whose software you run is what an attribution *is*. The line
//! states a fact about this deployment and solicits nothing; that is the
//! property to preserve if it is ever reworded.
//!
//! Prop-driven like the firm's footer: `crate::public_chrome::PublicFooter`
//! maps the resolved brand onto these props per request, so the wasm client
//! never links the view layer.

use dioxus::prelude::*;

/// The Foundation's footer.
///
/// - `entity`: the Foundation's corporate name (e.g. `Neon Law Foundation`).
/// - `jurisdiction_note`: what the corporation is, in one clause (e.g. `a
///   Nevada nonprofit corporation and a 501(c)(3) tax-exempt organization`).
/// - `disclaimer`: the Foundation's own legal-advice disclaimer.
/// - `copyright_year`: the Foundation's copyright line.
/// - `contact_email` / `office`: the Foundation's own inbound address and its
///   own registered office — never the firm's. Each renders only when non-empty.
/// - `transparency_href`: the public-disclosure page a 501(c)(3) owes under
///   IRC §6104(d). Empty renders no link.
#[component]
pub fn SiteFooterFoundation(
    entity: String,
    jurisdiction_note: String,
    disclaimer: String,
    copyright_year: i32,
    #[props(default)] contact_email: String,
    #[props(default)] office: String,
    #[props(default)] transparency_href: String,
    /// The firm that supports the Foundation and built this platform, named
    /// and linked. Resolved from the brand rather than written here so the
    /// firm's rename cannot leave a stale name on the nonprofit's footer.
    #[props(default)]
    supporter: String,
    #[props(default)] supporter_href: String,
    /// The platform attribution: the published release of Neon Law Navigator
    /// this deployment runs, and the page describing it. Software attribution,
    /// not a recommendation of the firm's legal services — rule 3 still binds,
    /// so this line carries no bar credential and makes no offer. Both empty
    /// renders no line.
    #[props(default)]
    navigator_version: String,
    #[props(default)] navigator_href: String,
) -> Element {
    let has_contact = !contact_email.is_empty() || !office.is_empty();
    rsx! {
        footer { class: "site-footer site-footer--foundation", role: "contentinfo",
            div { class: "site-footer__inner",
                if has_contact {
                    div { class: "site-footer__contact",
                        div { class: "site-footer__reach",
                            if !contact_email.is_empty() {
                                a {
                                    class: "nav-btn nav-btn--primary site-footer__cta",
                                    href: "mailto:{contact_email}",
                                    "Contact us — {contact_email}"
                                }
                            }
                            if !transparency_href.is_empty() {
                                a { class: "site-footer__transparency", href: "{transparency_href}",
                                    "Transparency & public disclosures"
                                }
                            }
                        }
                        if !office.is_empty() {
                            ul { class: "site-footer__offices",
                                li { class: "site-footer__office",
                                    span { class: "site-footer__office-label", "{entity}" }
                                    address { class: "site-footer__office-address", "{office}" }
                                }
                            }
                        }
                    }
                }
                div { class: "site-footer__legal",
                    // The affirmative separation: what the Foundation is, and
                    // what it is not. Both halves are load-bearing — the second
                    // is the sentence that keeps a reader from believing a
                    // 501(c)(3) publishing legal templates can act for them.
                    p { class: "site-footer__attribution",
                        "{entity} is {jurisdiction_note}. It does not practice law and cannot represent you."
                    }
                    // The firm's support, disclosed. It sits directly under the
                    // "cannot represent you" line on purpose: a reader who
                    // learns a law firm stands behind the Foundation should
                    // read that fact next to the sentence saying neither
                    // organization has become their lawyer by their being here.
                    //
                    // Guarded like every other optional prop on this footer. A
                    // deployment that publishes no supporter must render no
                    // line at all: an unguarded anchor emits `<a href=""></a>`,
                    // which is an empty link with no accessible name — a
                    // WCAG 2.4.4 failure the axe gate catches — wrapped in the
                    // sentence "… is supported by  who created this platform."
                    if !supporter.is_empty() && !supporter_href.is_empty() {
                        p { class: "site-footer__support",
                            "{entity} is supported by "
                            a { href: "{supporter_href}", "{supporter}" }
                            " who created this platform."
                        }
                    }
                    p { class: "site-footer__disclaimer", "{disclaimer}" }
                    p { class: "site-footer__copyright",
                        "© {copyright_year} {entity}"
                    }
                    // The platform attribution, last. It names the software the
                    // Foundation runs on and the page describing it — the same
                    // disclosure posture as the support line above, and equally
                    // not an offer of legal services.
                    if !navigator_version.is_empty() && !navigator_href.is_empty() {
                        p { class: "site-footer__platform",
                            "Powered by "
                            a { href: "{navigator_href}",
                                "Neon Law Navigator #{navigator_version}"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssr(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    fn html() -> String {
        fn app() -> Element {
            rsx! {
                SiteFooterFoundation {
                    entity: "Neon Law Foundation".to_string(),
                    jurisdiction_note:
                        "a Nevada nonprofit corporation and a 501(c)(3) tax-exempt organization"
                            .to_string(),
                    disclaimer:
                        "Nothing on this site is legal advice, and nothing here creates an \
                         attorney-client relationship."
                            .to_string(),
                    copyright_year: 2026,
                    contact_email: "support@neonlaw.org".to_string(),
                    office: "5150 Mae Anne Ave Ste 405-9999, Reno, NV 89523".to_string(),
                    transparency_href: "/transparency".to_string(),
                    supporter: "Neon Law".to_string(),
                    supporter_href: "https://www.neonlaw.com".to_string(),
                    navigator_version: "26.8.10".to_string(),
                    navigator_href: "https://www.neonlaw.com/navigator".to_string(),
                }
            }
        }
        ssr(app)
    }

    /// The Foundation discloses the firm that supports it and built the
    /// platform, by name and by link.
    ///
    /// This is the one place the two organizations appear together on a
    /// Foundation page, so where it sits matters as much as what it says: it
    /// renders *after* "does not practice law and cannot represent you", so a
    /// reader who learns a law firm stands behind the Foundation meets that
    /// fact next to the sentence saying neither has become their lawyer.
    #[test]
    fn discloses_the_supporting_firm_after_the_separation() {
        let out = html();
        assert!(
            out.contains("is supported by") && out.contains("who created this platform"),
            "the support disclosure is missing: {out}"
        );
        assert!(
            out.contains(r#"href="https://www.neonlaw.com""#),
            "the firm is linked, so the claim can be checked: {out}"
        );
        let separation = out
            .find("cannot represent you")
            .expect("the separation sentence");
        let support = out.find("is supported by").expect("the support sentence");
        assert!(
            separation < support,
            "the disclosure must follow the separation, not precede it: {out}"
        );
    }

    /// The support line names the firm, and no more than the firm.
    ///
    /// The Foundation's footer deliberately carries no bar number and no
    /// trademark notice — naming the firm must not drag the firm's regulated
    /// disclosures onto a page whose job is to keep the two apart.
    #[test]
    fn naming_the_firm_pulls_in_none_of_its_regulated_disclosures() {
        let out = html();
        for regulated in ["Bar No", "admitted in", "calbar", "mywsba", "nvbar"] {
            assert!(
                !out.contains(regulated),
                "the Foundation footer must not carry the firm's {regulated}: {out}"
            );
        }
    }

    /// The load-bearing sentence. A 501(c)(3) that publishes legal templates
    /// must say outright that it cannot act for the reader — "not legal advice"
    /// alone leaves the impression this footer exists to prevent.
    #[test]
    fn states_that_the_foundation_cannot_practice_law_or_represent_you() {
        let out = html();
        assert!(
            out.contains("does not practice law and cannot represent you"),
            "the affirmative separation is missing: {out}"
        );
        assert!(
            out.contains("501(c)(3) tax-exempt organization"),
            "the footer says what the corporation is: {out}"
        );
        assert!(
            out.contains("attorney-client relationship"),
            "the disclaimer survives: {out}"
        );
    }

    /// It identifies the corporation, not the wordmark the header wears.
    #[test]
    fn identifies_the_corporation_not_the_brand() {
        let out = html();
        let entity = out
            .find("Neon Law Foundation is a Nevada nonprofit corporation")
            .expect("the identity line names the corporation");
        // The copyright follows the identity line and names the Foundation.
        let copyright = out.find("©").expect("copyright present");
        assert!(entity < copyright, "identity precedes copyright: {out}");
    }

    /// The firm's regulated copy has no place here: no bar admissions, no bar
    /// numbers, no "legal services rendered by", and no registered mark the
    /// Foundation only uses under permission.
    #[test]
    fn carries_none_of_the_firms_regulated_copy() {
        let out = html();
        for forbidden in [
            "Admitted in",
            "Bar No.",
            "legal services rendered by",
            "®",
            "support@neonlaw.com",
        ] {
            assert!(
                !out.contains(forbidden),
                "a Foundation footer must not carry {forbidden:?}: {out}"
            );
        }
    }

    #[test]
    fn reaches_the_foundation_at_its_own_address_and_office() {
        let out = html();
        assert!(out.contains(r#"href="mailto:support@neonlaw.org""#));
        assert!(
            out.contains("Ste 405-9999"),
            "its own suite, not the firm's"
        );
        assert!(out.contains(r#"href="/transparency""#));
    }

    /// The platform attribution names the release and links to the page that
    /// describes it — software attribution, carrying no bar credential and
    /// making no offer, so rule 3 survives it.
    #[test]
    fn attributes_the_platform_and_its_published_release() {
        let out = html();
        assert!(
            out.contains("Powered by") && out.contains("Neon Law Navigator #26.8.10"),
            "the platform line names the running release: {out}"
        );
        assert!(
            out.contains(r#"href="https://www.neonlaw.com/navigator""#),
            "and links to the page describing it: {out}"
        );
    }

    /// An unstamped build prints no platform line.
    ///
    /// `NAVIGATOR_RELEASE_TAG` is absent from a local `cargo run`, and a footer
    /// reading "Neon Law Navigator #" is worse than no attribution. The same
    /// guard covers the support line: unguarded, it emitted `<a href=""></a>`
    /// — an empty link with no accessible name, which is a WCAG 2.4.4 failure
    /// and the violation the `/design` gallery tripped.
    #[test]
    fn omits_the_support_and_platform_lines_when_unpublished() {
        fn app() -> Element {
            rsx! {
                SiteFooterFoundation {
                    entity: "Acme Foundation".to_string(),
                    jurisdiction_note: "a nonprofit corporation".to_string(),
                    disclaimer: "Nothing here is legal advice.".to_string(),
                    copyright_year: 2026,
                }
            }
        }
        let out = ssr(app);
        assert!(
            !out.contains("is supported by"),
            "no supporter, no support line: {out}"
        );
        assert!(
            !out.contains("Powered by"),
            "no release, no platform line: {out}"
        );
        assert!(
            !out.contains(r#"href="""#),
            "and no empty anchor anywhere: {out}"
        );
    }

    /// A deploy that publishes no Foundation contact channels renders no empty
    /// band, exactly as the firm's footer degrades.
    #[test]
    fn omits_the_contact_band_when_nothing_is_published() {
        fn app() -> Element {
            rsx! {
                SiteFooterFoundation {
                    entity: "Acme Foundation".to_string(),
                    jurisdiction_note: "a nonprofit corporation".to_string(),
                    disclaimer: "Nothing here is legal advice.".to_string(),
                    copyright_year: 2026,
                }
            }
        }
        let out = ssr(app);
        assert!(!out.contains("site-footer__contact"), "no band: {out}");
        assert!(
            out.contains(r#"<div class="site-footer__legal""#),
            "the legal strip still renders: {out}"
        );
    }
}
