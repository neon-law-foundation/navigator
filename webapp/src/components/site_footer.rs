//! The site footer, as a Dioxus component (issue #641, Phase 2).
//!
//! Two bands. The contact band reaches the firm: the email CTA, the published
//! voice line, and every office it keeps. Below it the legal strip carries the
//! load-bearing, brand-driven lines every public page owes — the copyright that
//! names the legal person behind the site, which attorney holds which bar
//! licence, and the attorney-advertising disclaimer.
//!
//! It is prop-driven like [`crate::components::PricingSection`]: the process
//! brand (`views::brand`) is mapped onto [`SiteFooterLegal`]'s props per request
//! on the server, so the wasm client never links the view layer and a
//! white-label deploy emits its own identity. `crate::public_chrome::PublicFooter`
//! owns that mapping so no page restates it.
//!
//! The copy is legal-council-reviewed and must not drift. Bar disclosure is per
//! attorney and never firm-level: every published number links to the bar's own
//! record, so a visitor verifies a licence against the licensing jurisdiction
//! rather than trusting a summary line the page wrote about itself.

use dioxus::prelude::*;

use crate::components::{ExternalLink, GitHubStars};

/// One published office — the state it sits in and its street address.
/// Mirrors `views::brand::FirmOffice`.
#[derive(Clone, PartialEq, Eq)]
pub struct FooterOffice {
    pub state: String,
    pub address: String,
    /// A qualification rendered under the address — e.g. an admission that has
    /// not issued yet. `None` publishes the address unqualified. See
    /// `views::brand::FirmOffice::note` for why this rides the address.
    pub note: Option<String>,
}

/// Outlines of the states the firm publishes an office in, each derived from
/// the state's real border geometry rather than drawn by hand.
///
/// The source is Wikimedia Commons' `Blank US Map (states only).svg`, released
/// under CC0, whose per-state paths carry the actual border coordinates in one
/// shared projection. Each state's path was lifted from that file, simplified
/// with Douglas–Peucker to a few dozen vertices, and fitted — **at its true
/// aspect ratio**, centred — into this 100×100 box. Aspect ratio is the reason
/// the earlier hand-traced set read wrong: a state stretched to fill a square
/// stops being that state's silhouette, whatever its corners do.
///
/// Fitting to the larger dimension means a wide state (Washington) leaves space
/// above and below and a tall one (California, Nevada) leaves it to the sides,
/// so all four render at a consistent scale beside one another.
///
/// **Deliberately not the state flags.** New York's flag is the state coat of
/// arms and Washington's is the state seal; both states restrict use of those
/// devices in advertising, and a law-firm footer is advertising copy. An
/// outline carries the same "this is where we are" meaning and claims nothing
/// about who endorses the firm. It is also legible at one colour and one low
/// opacity, which four multi-colour flags behind body text would not be.
///
/// A state with no entry renders no watermark, so a white-label deploy that
/// publishes an office somewhere else simply gets the plain treatment.
const STATE_OUTLINES: &[(&str, &str)] = &[
    (
        "California",
        "M53.6 95 52.4 87.7 48.9 83.2 46.7 82.4 46.9 80.3 43.4 79.1 40 74.9 34.4 73.1 33.4 72.1 \
         34.5 67.1 31.3 61.3 29.8 56.3 28.7 55.3 28.9 52.5 30.6 50.9 30.4 49.7 29 49.3 27.5 46.7 \
         28 41.3 29 41.3 28.6 43.2 30.1 44.5 29.3 40.2 30.4 39.6 29.7 38.4 29.1 38.6 28.1 41 26 \
         38.6 25.8 34.9 23.3 29.8 24.6 21.7 22.4 17.4 22.5 15.3 25.9 11.4 27.4 7.3 27.8 3 53.7 \
         10.3 47.3 35.1 75.4 77.6 75 78.4 77.6 84.5 75.2 85.7 74.3 87.1 73.6 89.8 72.1 91.2 71.6 \
         93.9 73.2 95.2 72.9 96.2 70.8 97Z",
    ),
    (
        "Nevada",
        "M68.9 74.4 67.1 84 65.7 85.6 64.7 85.6 64 84.1 62 83.3 60.1 83.6 59.8 93.8 58.7 97 20.2 \
         39.5 19.6 37.6 28.6 3 80.4 14.7Z",
    ),
    // Two subpaths: the mainland, and Long Island east of the city.
    (
        "New York",
        "M73.5 84.3 75.7 84.8 83 81.7 80 82 86.4 79.6 97 71.1 92.5 73.4 91.3 72.1 90.5 75.5 89.1 \
         75.5 92.5 70.3 87.7 75 77 78.7 73.6 82.8ZM65.8 16.5 66.4 21.8 68 23.4 67.6 29.8 69.8 \
         34.6 69.2 35.8 69.9 37.4 70.5 36.3 71.9 37.3 74.4 49.5 74.1 60.2 76.6 72.5 77.8 73.5 \
         75.3 75.9 76.6 77.5 73.5 82.7 73.6 78 60.8 74 53.6 66.4 3.6 76.3 3 72 11.9 62.4 8.2 \
         57.6 8 55.3 13.4 52.5 19.3 51.4 25.3 52.4 33 49.9 36.4 45.8 38.6 45.2 38.9 42.9 37 40.5 \
         38.4 38.3 35.7 37.3 35.6 35.1 47.3 20.1 65.2 15.2Z",
    ),
    (
        "Washington",
        "M86.8 77.6 86.5 84.2 62.2 78.4 34.4 78.4 30.4 74.9 20.7 75.8 15.4 72.5 16 64.7 3 57 5 51 \
         4.8 56.2 8.4 51.4 5.5 49.8 5.6 46.4 9.6 46.7 5.3 45.5 5.5 22.9 7.4 19.5 13.7 25.6 24.1 \
         29.1 24.3 31.5 28.7 31.2 27.7 35 19.4 42 23 42.6 20.2 41.8 29.4 34.5 29.5 37.7 25.7 39.8 \
         25.6 46.2 24.7 43.6 22.9 46.6 23.3 43.3 19.2 47 22.9 48.6 28.8 45.3 29.1 39.4 33 35.2 \
         31.1 26 31.6 28.5 28.8 29.2 31.5 35.3 27.9 28.9 30.6 23.2 32.4 25.8 33.4 23.9 30.9 20.2 \
         32.3 15.8 97 32.6Z",
    ),
];

/// The outline for a published state, or `None` for one this component does not
/// carry a tracing of.
fn state_outline(state: &str) -> Option<&'static str> {
    STATE_OUTLINES
        .iter()
        .find(|(name, _)| *name == state)
        .map(|(_, outline)| *outline)
}

/// One page the footer links. Mirrors `views::brand::NavLink`, narrowed to what
/// a flat footer row renders — the footer has no dropdowns.
#[derive(Clone, PartialEq, Eq)]
pub struct FooterNavLink {
    pub label: String,
    pub href: String,
}

/// One bar license an attorney holds. Mirrors `views::brand::BarLicense`.
#[derive(Clone, PartialEq, Eq)]
pub struct FooterBarLicense {
    pub jurisdiction: String,
    pub number: String,
    pub license_url: String,
}

/// One licensed attorney and the set of bar licenses they hold. Mirrors
/// `views::brand::FirmAttorney`.
#[derive(Clone, PartialEq, Eq)]
pub struct FooterAttorney {
    pub name: String,
    pub licenses: Vec<FooterBarLicense>,
}

/// The lines a published address is set over.
///
/// An address is published as one string, here and in a white-label manifest
/// (`brand.firm_offices[].address`), because that is how a firm writes its own
/// address. The footer sets it the way an envelope carries it — street, then
/// unit, then city — so the suite gets its own line and the city a reader is
/// scanning for starts one, instead of the whole address running together and
/// breaking wherever the narrow column happens to end.
///
/// Every comma starts a new line, except the one between the city and its
/// state: `Walnut Creek, CA 94596` is one line, because a city split from its
/// ZIP stops reading as a place. So the last two of three or more
/// comma-separated parts are the final line, and each part before them is a
/// line of its own. An address written with no comma at all is published as the
/// one line it was written as, rather than broken at a guess.
fn address_lines(address: &str) -> Vec<&str> {
    let mut commas = address.rmatch_indices(',').map(|(index, _)| index);
    // The last comma separates the city from `ST ZIP` and is not a break; the
    // one before it ends the last line above the city.
    commas.next();
    let (above, city) = match commas.next() {
        Some(index) => (&address[..index], Some(address[index + 1..].trim_start())),
        None => (address, None),
    };
    let mut lines: Vec<&str> = above
        .split(',')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    lines.extend(city);
    lines
}

/// `tel:` dials digits, not the human spacing a number is written with.
fn tel_href(phone: &str) -> String {
    format!(
        "tel:{}",
        phone
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '+')
            .collect::<String>()
    )
}

/// The footer legal strip. Every field is resolved from the deploy's brand and
/// handed in per request, so the component itself is pure presentation.
///
/// - `copyright_holder`: the legal person that owns the site and the words on
///   it — `Neon Law` on the firm's deploy. It heads the legal strip, and
///   it is the only line naming the entity behind the site. Deliberately a
///   separate dial from the wordmark the page trades under, even where a
///   deploy sets both to the same words: a copyright notice has to name an
///   entity that can hold one, and a white-label bundle renames the wordmark
///   without renaming the copyright holder.
/// - `disclaimer`: the attorney-advertising disclaimer copy.
/// - `copyright_year`: the year on the copyright line, resolved by the
///   server per request so the reusable component never carries a stale year.
/// - `contact_email` / `phone` / `offices`: the firm's published contact
///   channels. Each is independently optional — an empty value renders
///   nothing, so a deploy that publishes no voice line or no walk-in office
///   simply omits it rather than rendering an empty label. With all three
///   empty the contact band disappears and only the legal strip remains.
/// - `attorneys`: the licensed attorneys and the bar licences each holds, one
///   line per attorney, every number linked to that bar's own record. Empty
///   renders no licence list.
///
/// The whole footer is width-constrained to the same 72rem column the
/// [`crate::components::SiteHeader`] nav uses, so its content lines up with
/// the navbar above it instead of running to the viewport edge. The rule and
/// background stay full-bleed, matching the header's `border-bottom`.
#[component]
pub fn SiteFooterLegal(
    copyright_holder: String,
    disclaimer: String,
    copyright_year: i32,
    #[props(default)] logo_href: String,
    #[props(default)] contact_email: String,
    #[props(default)] phone: String,
    #[props(default)] offices: Vec<FooterOffice>,
    #[props(default)] attorneys: Vec<FooterAttorney>,
    #[props(default)] brand_name: String,
    #[props(default)] nav: Vec<FooterNavLink>,
    /// The nonprofit the firm supports, and its site. The Foundation's footer
    /// names the firm; this is the other half of that attribution, so a
    /// visitor arriving at either can reach the other. Empty renders no line.
    #[props(default)]
    foundation: String,
    #[props(default)] foundation_href: String,
    /// The nonprofit's own identity block, rendered inside this footer rather
    /// than as a second one.
    ///
    /// One site holds both organizations, so every page carries both
    /// disclosures. A reader cannot be assumed to know which door they arrived
    /// through: one who landed on `/foundation` used to meet no
    /// attorney-advertising disclosure at all, and one on a firm page never met
    /// the nonprofit's "cannot represent you".
    ///
    /// The wording is [`crate::components::SiteFooterFoundation`]'s, verbatim,
    /// and is legal-council reviewed — see that module's four rules. It renders
    /// BELOW the firm's bar disclosures and its disclaimer so the nonprofit's
    /// name can never be read as one of the firm's credentials, which is rule 3
    /// of that review. Empty `nonprofit_entity` renders nothing.
    #[props(default)]
    nonprofit_entity: String,
    #[props(default)] nonprofit_jurisdiction_note: String,
    #[props(default)] nonprofit_disclaimer: String,
    /// The nonprofit's OWN inbox and registered office, never the firm's. They
    /// sit in this block rather than in the contact band above, which is
    /// firm-anchored, so a reader can tell which organization each address
    /// reaches. Each renders only when non-empty.
    #[props(default)]
    nonprofit_contact_email: String,
    #[props(default)] nonprofit_office: String,
    #[props(default)] nonprofit_transparency_href: String,
    /// The platform attribution: the published release of Neon Law Navigator
    /// this deployment runs, and the page describing it. Both empty renders no
    /// line — a deployment that cannot name its release must not print `#`
    /// followed by nothing.
    #[props(default)]
    navigator_version: String,
    #[props(default)] navigator_href: String,
    /// The public repository the platform is developed in — how it is named
    /// (`owner/name`), where it lives, and how many people have starred it.
    ///
    /// The pair below the platform attribution and above nothing: it says the
    /// software the line above names is open source, and gives the address to
    /// go read it. Both strings empty renders no line, the way an unstamped
    /// release renders no attribution.
    ///
    /// `source_stars` is independently optional, and `None` is the ordinary
    /// case rather than a failure — see
    /// [`crate::source_repository`]. It renders the link with no count.
    #[props(default)]
    source_repo: String,
    #[props(default)] source_href: String,
    #[props(default)] source_stars: Option<u64>,
) -> Element {
    let has_contact = !contact_email.is_empty() || !phone.is_empty() || !offices.is_empty();
    let supports_foundation = !foundation.is_empty() && !foundation_href.is_empty();
    let states_nonprofit = !nonprofit_entity.is_empty() && !nonprofit_jurisdiction_note.is_empty();
    rsx! {
        footer { class: "site-footer", role: "contentinfo",
            div { class: "site-footer__inner",
                // The mark and the name it belongs to. The logo alone left the
                // footer opening on an unlabelled glyph; the wordmark is the
                // same one the header carries, so the bottom of the page says
                // whose page it is. The image stays decorative (`alt=""`)
                // because the text beside it is the label.
                if !logo_href.is_empty() || !brand_name.is_empty() {
                    div { class: "site-footer__brand",
                        if !logo_href.is_empty() {
                            img {
                                class: "site-footer__logo",
                                src: "{logo_href}",
                                alt: ""
                            }
                        }
                        if !brand_name.is_empty() {
                            strong { class: "site-footer__wordmark", "{brand_name}" }
                        }
                    }
                }
                // The pages the header does not carry. A flat row of the firm's
                // own routes, first thing in the footer, so a reader who
                // scrolled to the bottom looking for the Blog, Contact, or the
                // platform page finds them before the contact band's detail.
                if !nav.is_empty() {
                    nav { class: "site-footer__nav", "aria-label": "More pages",
                        for link in nav.iter() {
                            a {
                                class: "site-footer__nav-link",
                                key: "{link.href}",
                                href: "{link.href}",
                                "{link.label}"
                            }
                        }
                    }
                }
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
                            if !phone.is_empty() {
                                a {
                                    class: "site-footer__phone",
                                    href: tel_href(&phone),
                                    "{phone}"
                                }
                            }
                        }
                        if !offices.is_empty() {
                            ul { class: "site-footer__offices",
                                for office in offices.iter() {
                                    li { class: "site-footer__office", key: "{office.state}",
                                        // The state behind its own address, as
                                        // a watermark. Decorative and
                                        // `aria-hidden`: the label above it
                                        // already names the state, so a screen
                                        // reader that announced this too would
                                        // hear the office twice.
                                        if let Some(outline) = state_outline(&office.state) {
                                            svg {
                                                class: "site-footer__office-map",
                                                "viewBox": "0 0 100 100",
                                                "aria-hidden": "true",
                                                "focusable": "false",
                                                fill: "currentColor",
                                                path { d: outline }
                                            }
                                        }
                                        span { class: "site-footer__office-label", "{office.state}" }
                                        // `<address>` is the semantic element
                                        // for the contact details of its
                                        // nearest ancestor section — here, the
                                        // firm that owns the page. Set line by
                                        // line — street, unit, city — the way
                                        // the envelope would carry it.
                                        address { class: "site-footer__office-address",
                                            for line in address_lines(&office.address) {
                                                span {
                                                    class: "site-footer__office-line",
                                                    key: "{line}",
                                                    "{line}"
                                                }
                                            }
                                        }
                                        // The qualification sits inside the
                                        // same `<li>` as the address it
                                        // qualifies, so no reader can pair it
                                        // with the wrong office.
                                        if let Some(note) = office.note.as_ref() {
                                            span { class: "site-footer__office-note", "{note}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "site-footer__legal",
                    p { class: "site-footer__copyright",
                        // The site and the words on it belong to the firm's
                        // legal person, which is what a copyright notice names
                        // — the wordmark cannot hold one. This heads the legal
                        // strip rather than trailing it, because it is the only
                        // line naming the entity behind the site.
                        "© {copyright_year} {copyright_holder} and {foundation}"
                    }
                    // Who is licensed, where, and under what number — each
                    // linked to the bar's own record so a visitor can verify
                    // the licence rather than take the site's word for it. This
                    // is the footer's only bar disclosure: a firm-level
                    // "Admitted in …" line said nothing these rows do not, in
                    // the jurisdictions they already name.
                    if !attorneys.is_empty() {
                        ul { class: "site-footer__licenses",
                            for attorney in attorneys.iter() {
                                li { class: "site-footer__licensee", key: "{attorney.name}",
                                    span { class: "site-footer__licensee-name", "{attorney.name}" }
                                    " — "
                                    for (index, license) in attorney.licenses.iter().enumerate() {
                                        if index > 0 {
                                            " · "
                                        }
                                        ExternalLink {
                                            href: license.license_url.clone(),
                                            class: "link-secondary".to_string(),
                                            "{license.jurisdiction} Bar No. {license.number}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    p { class: "site-footer__disclaimer", "{disclaimer}" }
                    // The other half of the attribution the Foundation's footer
                    // carries, and the last line on the page. It sits after the
                    // bar disclosures and after the disclaimer, so a reader has
                    // been told who is licensed and where before meeting a
                    // second organization's name — the nonprofit holds no
                    // licence and this must not read as one of the firm's
                    // credentials. "supporter of" rather than anything implying
                    // the two are one organization — they are separate entities,
                    // which the sentence conveys by naming the nonprofit's own
                    // status rather than by calling it "separate". The trailing
                    // clause sits outside the anchor so the hover underline stops
                    // at the Foundation's name.
                    if supports_foundation {
                        p { class: "site-footer__foundation",
                            "{copyright_holder} is a proud supporter of the "
                            ExternalLink {
                                href: foundation_href.clone(),
                                class: "link-secondary".to_string(),
                                "{foundation}"
                            }
                            ", a 501(c)(3) nonprofit."
                        }
                    }
                    // The nonprofit's own identity, on every page of the shared
                    // site. Two sentences, both load-bearing: what the
                    // corporation is, and — affirmatively — that it does not
                    // practise law and cannot act for the reader. The second is
                    // what stops a visitor believing a 501(c)(3) that publishes
                    // legal templates and an AI assistant can represent them.
                    //
                    // Below the bar disclosures and the firm's disclaimer on
                    // purpose: the reader has met who is licensed and where
                    // before a second organization is named, so the nonprofit
                    // cannot be read as one of the firm's credentials.
                    if states_nonprofit {
                        p { class: "site-footer__attribution",
                            "{nonprofit_entity} is {nonprofit_jurisdiction_note}. It does not practice law and cannot represent you."
                        }
                        if !nonprofit_disclaimer.is_empty() {
                            p { class: "site-footer__nonprofit-disclaimer", "{nonprofit_disclaimer}" }
                        }
                        // The nonprofit's own registered office and inbox, so a
                        // reader who wants the Foundation rather than the firm
                        // has an address that reaches it. Kept out of the
                        // contact band above, which is the firm's.
                        if !nonprofit_office.is_empty() {
                            address { class: "site-footer__nonprofit-office", "{nonprofit_office}" }
                        }
                        if !nonprofit_contact_email.is_empty() {
                            a {
                                class: "site-footer__nonprofit-email",
                                href: "mailto:{nonprofit_contact_email}",
                                "{nonprofit_contact_email}"
                            }
                        }
                        // The public-disclosure page a 501(c)(3) owes under
                        // IRC §6104(d). Guarded: an unguarded anchor would emit
                        // an empty link with no accessible name.
                        if !nonprofit_transparency_href.is_empty() {
                            a {
                                class: "site-footer__transparency",
                                href: "{nonprofit_transparency_href}",
                                "Transparency & public disclosures"
                            }
                        }
                    }
                    // The platform attribution, last in the strip. It names
                    // software, not a legal service, so it sits below the
                    // regulated disclosures rather than among them — a reader
                    // meets the bar records and the advertising disclaimer
                    // before a line about what the site is built on.
                    if !navigator_version.is_empty() && !navigator_href.is_empty() {
                        p { class: "site-footer__platform",
                            "Powered by "
                            ExternalLink {
                                href: navigator_href.clone(),
                                class: "link-secondary".to_string(),
                                "Neon Law Navigator #{navigator_version}"
                            }
                        }
                    }
                    // Where to go read the software the line above names. It
                    // closes the strip for the same reason the platform line
                    // sits above it: a source repository is a developer
                    // surface, and a visitor meets every regulated disclosure
                    // before the page mentions one.
                    //
                    // It is deliberately a separate line from the attribution
                    // rather than a link appended to it. The attribution names
                    // the *release this deployment runs*; this names the
                    // *project*, which is a standing fact about the software
                    // and is published whether or not the build was stamped.
                    if !source_repo.is_empty() && !source_href.is_empty() {
                        p { class: "site-footer__source",
                            "Open source — "
                            GitHubStars {
                                href: source_href.clone(),
                                repo: source_repo.clone(),
                                stars: source_stars,
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

    /// The firm's legal strip: the copyright line that names the entity, the
    /// attorney-advertising disclaimer, and nothing else the deploy did not
    /// hand in.
    fn legal_html() -> String {
        fn app() -> Element {
            rsx! {
                SiteFooterLegal {
                    copyright_holder: "Neon Law".to_string(),
                    disclaimer: "This is an attorney advertisement.".to_string(),
                    copyright_year: 2026,
                    foundation: "Neon Law Foundation".to_string(),
                    foundation_href: "/foundation".to_string(),
                    navigator_version: "26.8.10".to_string(),
                    navigator_href: "https://www.neonlaw.com/navigator".to_string(),
                    source_repo: "neon-law-foundation/navigator".to_string(),
                    source_href: "https://github.com/neon-law-foundation/navigator".to_string(),
                    source_stars: 1234u64,
                }
            }
        }
        ssr(app)
    }

    /// The open-source line names the repository, links it, and prints the
    /// star count — and it closes the strip, below the platform attribution.
    ///
    /// Order is the substance here, as it is for every other line in this
    /// strip: the repository is a developer surface, so a reader meets the bar
    /// records, the advertising disclaimer, and the nonprofit's statement
    /// before the page mentions where the code lives.
    #[test]
    fn closes_the_strip_with_the_source_repository_and_its_stars() {
        let out = legal_html();
        assert!(
            out.contains(r#"href="https://github.com/neon-law-foundation/navigator""#),
            "the repository is linked: {out}"
        );
        assert!(
            out.contains("Open source") && out.contains("neon-law-foundation/navigator"),
            "and named as the project's source: {out}"
        );
        assert!(
            out.contains("1,234") && out.contains("<title>GitHub stars</title>"),
            "the star count renders under its own accessible name: {out}"
        );
        let disclaimer = out.find("attorney advertisement").expect("the disclaimer");
        let platform = out.find("Powered by").expect("the platform line");
        let source = out.find("site-footer__source").expect("the source line");
        assert!(
            disclaimer < platform && platform < source,
            "the source line closes the strip, under the platform attribution: {out}"
        );
    }

    /// A deploy that publishes no repository renders no line, and one whose
    /// star count has not been fetched yet renders the link without a number.
    ///
    /// The second half is the ordinary case, not an edge one: the count comes
    /// from a cache a background task fills after boot, so every render before
    /// the first fetch — and every render in a process that never spawned the
    /// refresh, which is every test — takes this path.
    #[test]
    fn omits_the_source_line_when_unset_and_the_count_when_unfetched() {
        fn app() -> Element {
            rsx! {
                SiteFooterLegal {
                    copyright_holder: "Neon Law".to_string(),
                    disclaimer: "This is an attorney advertisement.".to_string(),
                    copyright_year: 2026,
                }
            }
        }
        fn unfetched() -> Element {
            rsx! {
                SiteFooterLegal {
                    copyright_holder: "Neon Law".to_string(),
                    disclaimer: "This is an attorney advertisement.".to_string(),
                    copyright_year: 2026,
                    source_repo: "neon-law-foundation/navigator".to_string(),
                    source_href: "https://github.com/neon-law-foundation/navigator".to_string(),
                }
            }
        }

        let out = ssr(app);
        assert!(
            !out.contains("site-footer__source") && !out.contains("Open source"),
            "no repository, no line: {out}"
        );
        assert!(!out.contains(r#"href="""#), "no empty anchor: {out}");

        let out = ssr(unfetched);
        assert!(
            out.contains(r#"href="https://github.com/neon-law-foundation/navigator""#),
            "an unknown count still publishes the repository: {out}"
        );
        assert!(
            !out.contains("GitHub stars"),
            "and prints no number in place of one it does not have: {out}"
        );
    }

    /// The platform attribution closes the strip.
    ///
    /// It names software rather than a legal service, so it must sit below the
    /// bar records and the advertising disclaimer — a visitor meets who is
    /// licensed, and the advertising notice, before a line about what the site
    /// runs on.
    #[test]
    fn attributes_the_platform_below_the_regulated_disclosures() {
        let out = legal_html();
        assert!(
            out.contains("Powered by") && out.contains("Neon Law Navigator #26.8.10"),
            "the platform line names the running release: {out}"
        );
        let disclaimer = out.find("attorney advertisement").expect("the disclaimer");
        let platform = out.find("Powered by").expect("the platform line");
        assert!(
            disclaimer < platform,
            "the regulated disclaimer precedes the platform attribution: {out}"
        );
    }

    /// An unstamped build prints no platform line rather than a bare `#`, and
    /// leaves no empty anchor behind.
    #[test]
    fn omits_the_platform_line_when_the_release_is_unknown() {
        fn app() -> Element {
            rsx! {
                SiteFooterLegal {
                    copyright_holder: "Neon Law".to_string(),
                    disclaimer: "This is an attorney advertisement.".to_string(),
                    copyright_year: 2026,
                }
            }
        }
        let out = ssr(app);
        assert!(!out.contains("Powered by"), "no release, no line: {out}");
        assert!(!out.contains(r#"href="""#), "no empty anchor: {out}");
    }

    /// The two footers link to each other. The Foundation's names the firm that
    /// supports it and built the platform; this is the other half, so a visitor
    /// who lands on either site can reach the other.
    #[test]
    fn links_the_nonprofit_the_firm_supports() {
        let out = contactable_html();
        assert!(
            out.contains("is a proud supporter of the") && out.contains("Neon Law Foundation"),
            "the firm names the nonprofit it supports: {out}"
        );
        assert!(
            out.contains(r#"href="/foundation""#),
            "and links it, so the claim can be followed: {out}"
        );
        // The nonprofit's own status, said plainly: the two are different kinds
        // of organization and a reader on a law firm's site should not have to
        // infer that from the name.
        assert!(
            out.contains("a 501(c)(3) nonprofit"),
            "the relationship is stated, not implied: {out}"
        );
    }

    /// The copyright names the entity that holds it — the firm's legal person,
    /// not the wordmark it trades under.
    ///
    /// The Foundation may now appear elsewhere in this footer, as the
    /// attribution pairing with the one its own footer carries. What must not
    /// drift is the copyright line itself: the nonprofit is a separate
    /// corporation and holds nothing here.
    #[test]
    fn names_both_organizations_in_the_copyright() {
        let out = legal_html();
        let copyright = out
            .split(r#"<p class="site-footer__copyright">"#)
            .nth(1)
            .and_then(|rest| rest.split("</p>").next())
            .expect("the copyright line renders");
        assert!(
            copyright.contains("\u{a9} 2026 Neon Law"),
            "the copyright names the holding entity: {copyright}"
        );
        // Both organizations, because one footer serves both faces: the firm
        // renders the legal services and the Foundation runs the programmes,
        // and a shared page cannot credit only one of them. The *supporter*
        // line is the one that must name the firm alone — asserted separately —
        // or it reads as the nonprofit supporting itself.
        assert!(
            copyright.contains("Neon Law Foundation"),
            "the copyright names both organizations: {copyright}"
        );
    }

    /// The copyright heads the legal strip and there is exactly one of it.
    ///
    /// Asserted on the copyright element rather than on the entity's name. The
    /// name now appears twice on purpose: the supporter line at the foot of the
    /// page says "Neon Law is a proud supporter of …", which is the
    /// firm's own wording. What must not recur is the *copyright*, which once
    /// trailed a firm-level attribution line saying the same thing.
    #[test]
    fn renders_the_copyright_once_at_the_head_of_the_strip() {
        let out = legal_html();
        assert_eq!(
            out.matches(r#"class="site-footer__copyright""#).count(),
            1,
            "one copyright line: {out}"
        );
        let legal = out
            .find(r#"<div class="site-footer__legal""#)
            .expect("the legal strip renders");
        let copyright = out
            .find(r#"<p class="site-footer__copyright""#)
            .expect("the copyright renders");
        let disclaimer = out
            .find(r#"<p class="site-footer__disclaimer""#)
            .expect("the disclaimer renders");
        assert!(
            legal < copyright && copyright < disclaimer,
            "the copyright heads the strip: {out}"
        );
    }

    /// The firm-level "Admitted in California \u{b7} Washington \u{b7} Nevada" line is
    /// gone. A jurisdiction is published per attorney, with the bar number, by
    /// the licence list below — never as a firm-wide claim.
    #[test]
    fn publishes_no_firm_level_admissions_line() {
        let out = legal_html();
        assert!(
            !out.contains("Admitted in"),
            "the firm-level admissions line is retired: {out}"
        );
        let licensed = contactable_html();
        assert!(
            licensed.contains("Nevada Bar No."),
            "the jurisdiction is still published as an attorney licence: {licensed}"
        );
    }

    /// A footer carrying every contact channel: the CTA, the voice line, the
    /// offices, and the per-attorney bar licenses.
    fn contactable_html() -> String {
        fn app() -> Element {
            rsx! {
                SiteFooterLegal {
                    copyright_holder: "Neon Law".to_string(),
                    disclaimer: "This is an attorney advertisement.".to_string(),
                    copyright_year: 2026,
                    logo_href: "/public/logo-firm.svg".to_string(),
                    brand_name: "Neon Law".to_string(),
                    contact_email: "support@neonlaw.com".to_string(),
                    phone: "+1 510 707 6036".to_string(),
                    // The real addresses, as `views::brand` publishes them: a
                    // suite is its own comma-separated part, which is what the
                    // footer breaks a line on.
                    offices: [
                        ("California", "1990 N California Blvd, Ste 800, Walnut Creek, CA 94596", None),
                        ("Nevada", "5150 Mae Anne Ave, Ste 405-9777, Reno, NV 89523", None),
                        (
                            "New York",
                            "12 E 49th St, 18th Floor, New York, NY 10017",
                            Some("Bar admission pending"),
                        ),
                        ("Washington", "720 Seneca St, Ste 107-715, Seattle, WA 98101", None),
                    ]
                    .into_iter()
                    .map(|(state, address, note)| FooterOffice {
                        state: state.to_string(),
                        address: address.to_string(),
                        note: note.map(str::to_string),
                    })
                    .collect(),
                    attorneys: vec![
                        FooterAttorney {
                            name: "Nicholas Richard Shook".to_string(),
                            licenses: vec![FooterBarLicense {
                                jurisdiction: "Nevada".to_string(),
                                number: "13400".to_string(),
                                license_url: "https://nvbar.org/find-a-lawyer/?usearch=13400"
                                    .to_string(),
                            }],
                        },
                    ],
                    nav: [("Team", "/team"), ("Blog", "/blog"), ("Contact", "/contact")]
                        .into_iter()
                        .map(|(label, href)| FooterNavLink {
                            label: label.to_string(),
                            href: href.to_string(),
                        })
                        .collect(),
                    foundation: "Neon Law Foundation".to_string(),
                    foundation_href: "/foundation".to_string(),
                }
            }
        }
        ssr(app)
    }

    /// The footer's content sits in the same width-capped column the header nav
    /// uses, so it lines up with the navbar rather than running to the edge.
    #[test]
    fn wraps_its_content_in_the_navbar_aligned_column() {
        let out = contactable_html();
        assert!(
            out.contains(r#"<footer class="site-footer""#),
            "the rule/background element is the outer footer: {out}"
        );
        let inner = out
            .find(r#"<div class="site-footer__inner""#)
            .expect("the width-capped column is present");
        let legal = out.find("site-footer__legal").expect("legal strip present");
        assert!(
            inner < legal,
            "the legal strip sits inside the column: {out}"
        );
    }

    #[test]
    fn renders_the_contact_cta_and_voice_line() {
        let out = contactable_html();
        assert!(
            out.contains(r#"class="site-footer__logo" src="/public/logo-firm.svg" alt="""#),
            "the supplied brand mark renders as decorative footer identity: {out}"
        );
        assert!(
            out.contains(r#"href="mailto:support@neonlaw.com""#),
            "the CTA mails the firm's inbound address: {out}"
        );
        // `tel:` dials digits only — the human spacing would not dial.
        assert!(
            out.contains(r#"href="tel:+15107076036""#),
            "the voice line is dialable: {out}"
        );
        assert!(
            out.contains("+1 510 707 6036"),
            "the number is shown as written: {out}"
        );
    }

    /// Each office is labelled by its state and they render in the given
    /// order. The assertion anchors on the street addresses rather than on the
    /// labels: "Walnut Creek" and "Seattle" still occur inside their own
    /// addresses, so a label-only check would pass even if the labels had never
    /// changed.
    #[test]
    fn lists_every_office_in_order() {
        let out = contactable_html();
        let positions: Vec<usize> = [
            "1990 N California Blvd",
            "5150 Mae Anne Ave",
            "12 E 49th St",
            "720 Seneca St",
        ]
        .iter()
        .map(|address| {
            out.find(address)
                .unwrap_or_else(|| panic!("{address} present: {out}"))
        })
        .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "offices render in the given order: {out}"
        );
        for label in ["California", "Nevada", "New York", "Washington"] {
            assert!(
                out.contains(&format!(r#"class="site-footer__office-label">{label}<"#)),
                "{label} labels its office: {out}"
            );
        }
    }

    /// Each address is set line by line — street, unit, then city with its state
    /// and ZIP — so the suite has its own line and the city starts one instead
    /// of landing wherever the narrow footer column happened to wrap.
    #[test]
    fn sets_each_address_over_a_street_a_unit_and_a_city_line() {
        let out = contactable_html();
        for (street, unit, city) in [
            (
                "1990 N California Blvd",
                "Ste 800",
                "Walnut Creek, CA 94596",
            ),
            ("5150 Mae Anne Ave", "Ste 405-9777", "Reno, NV 89523"),
            ("12 E 49th St", "18th Floor", "New York, NY 10017"),
            ("720 Seneca St", "Ste 107-715", "Seattle, WA 98101"),
        ] {
            let line =
                |text: &str| format!(r#"<span class="site-footer__office-line">{text}</span>"#);
            assert!(
                out.contains(&format!("{}{}{}", line(street), line(unit), line(city))),
                "{street} / {unit} / {city} are three lines: {out}"
            );
        }
        assert_eq!(
            out.matches("site-footer__office-line").count(),
            12,
            "three lines for each of the four offices: {out}"
        );
    }

    /// The city keeps its state and ZIP, every other comma is a break, and an
    /// address with no city to lift out publishes as the one line it was written
    /// as rather than broken at a guess.
    #[test]
    fn breaks_on_every_comma_but_the_one_holding_a_city_to_its_state() {
        assert_eq!(
            super::address_lines("1990 N California Blvd, Ste 800, Walnut Creek, CA 94596"),
            [
                "1990 N California Blvd",
                "Ste 800",
                "Walnut Creek, CA 94596"
            ],
        );
        // A firm that writes its address on one line still gets a city line.
        assert_eq!(
            super::address_lines("1 Main St, Boise, ID 83702"),
            ["1 Main St", "Boise, ID 83702"],
        );
        // Four lines above the city, if that is how the address is written.
        assert_eq!(
            super::address_lines("Attn: Mail, Bldg 4, Ste 400, 1 Main St, Boise, ID 83702"),
            [
                "Attn: Mail",
                "Bldg 4",
                "Ste 400",
                "1 Main St",
                "Boise, ID 83702"
            ],
        );
        // A city with no state beside it is still a line of its own.
        assert_eq!(
            super::address_lines("1 Main St, Boise"),
            ["1 Main St", "Boise"],
        );
        // Nothing to break: published as written.
        assert_eq!(
            super::address_lines("General Delivery"),
            ["General Delivery"]
        );
        assert!(super::address_lines("").is_empty(), "no address, no line");
    }

    /// Every published state carries its outline, and the outline is
    /// decoration rather than content: it is `aria-hidden`, so a screen reader
    /// hears the state once from the label rather than twice.
    #[test]
    fn draws_each_state_behind_its_own_office() {
        let out = contactable_html();
        assert_eq!(
            out.matches("site-footer__office-map").count(),
            4,
            "one watermark per office: {out}"
        );
        for state in ["California", "Nevada", "New York", "Washington"] {
            assert!(
                super::state_outline(state).is_some(),
                "{state} has a traced outline"
            );
        }
        // The SVG is hidden from assistive technology and unfocusable.
        assert!(
            out.contains(r#"focusable="false""#),
            "not a tab stop: {out}"
        );
        // Colour comes from the stylesheet, never from the component — the
        // outline inherits it so one token themes all four.
        assert!(
            out.contains(r#"fill="currentColor""#),
            "the outline inherits its colour: {out}"
        );
    }

    /// Every point of an outline, read out of its `d` attribute.
    fn outline_points(state: &str) -> Vec<(f64, f64)> {
        let d = super::state_outline(state).expect("the state has an outline");
        let numbers: Vec<f64> = d
            .replace(['M', 'Z'], " ")
            .split_whitespace()
            .map(|token| token.parse::<f64>().expect("a coordinate"))
            .collect();
        assert_eq!(numbers.len() % 2, 0, "{state} has whole coordinate pairs");
        numbers.chunks(2).map(|pair| (pair[0], pair[1])).collect()
    }

    /// The outlines are the real state geometry, fitted at true aspect ratio —
    /// not four silhouettes stretched to fill a square.
    ///
    /// This is the whole substance of the tracing: a border scaled unequally on
    /// its two axes stops being that state's shape no matter how faithful its
    /// corners are, and it is exactly the failure a hand-drawn replacement
    /// reintroduces. So each outline has to fill the padded box on its long
    /// axis, fall short of it on the short one, and stand the way the state
    /// actually stands.
    #[test]
    fn fits_each_outline_at_the_state_real_aspect_ratio() {
        // Portrait or landscape, as the state itself is: California and Nevada
        // run north–south, New York and Washington east–west.
        for (state, landscape) in [
            ("California", false),
            ("Nevada", false),
            ("New York", true),
            ("Washington", true),
        ] {
            let points = outline_points(state);
            let width = points.iter().map(|p| p.0).fold(f64::MIN, f64::max)
                - points.iter().map(|p| p.0).fold(f64::MAX, f64::min);
            let height = points.iter().map(|p| p.1).fold(f64::MIN, f64::max)
                - points.iter().map(|p| p.1).fold(f64::MAX, f64::min);
            assert!(
                points
                    .iter()
                    .all(|(x, y)| (0.0..=100.0).contains(x) && (0.0..=100.0).contains(y)),
                "{state} stays inside the 100×100 box"
            );
            assert!(
                (width.max(height) - 94.0).abs() < 1.0,
                "{state} fills the padded box on its long axis: {width}×{height}"
            );
            assert!(
                width.min(height) < 90.0,
                "{state} is fitted, not stretched square: {width}×{height}"
            );
            assert_eq!(
                width > height,
                landscape,
                "{state} stands the way the state does: {width}×{height}"
            );
        }
    }

    /// Team, Blog, and Contact left the header for the footer, so the footer has
    /// to actually link them. A route dropped from the nav and not picked up
    /// here is a page reachable only by typing its URL.
    #[test]
    fn links_the_pages_the_header_no_longer_carries() {
        let out = contactable_html();
        for (label, href) in [
            ("Team", "/team"),
            ("Blog", "/blog"),
            ("Contact", "/contact"),
        ] {
            assert!(
                out.contains(&format!(r#"href="{href}""#)),
                "the footer links {label}: {out}"
            );
        }
        assert!(
            out.contains(r#"aria-label="More pages""#),
            "the row is a labelled landmark: {out}"
        );
    }

    /// The supporter line names the firm, says what it is, and leaves the site
    /// for the Foundation's own domain — as the last line on the page, under the
    /// disclaimer, ending in a full stop that is not part of the link.
    #[test]
    fn names_the_foundation_it_supports() {
        let out = contactable_html();
        assert!(
            out.contains("Neon Law is a proud supporter of the"),
            "the supporter line renders: {out}"
        );
        let disclaimer = out
            .find("site-footer__disclaimer")
            .expect("the disclaimer renders");
        let supporter = out
            .find("site-footer__foundation")
            .expect("the supporter line renders");
        assert!(
            disclaimer < supporter,
            "it is the last line, under the disclaimer: {out}"
        );
        assert!(
            out.contains("</a>, a 501(c)(3) nonprofit.</p>"),
            "the clause and its period sit outside the link: {out}"
        );
        assert!(
            out.contains(r#"href="/foundation""#),
            "and links the Foundation's own site: {out}"
        );
        // Two separate organizations, so the link leaves the site the way every
        // other outbound link here does.
        assert!(
            out.contains(r#"rel="noopener noreferrer""#),
            "the outbound link is hardened: {out}"
        );
    }

    /// A deploy that publishes neither — no footer routes and no Foundation —
    /// renders neither, rather than an empty row or a dangling sentence.
    #[test]
    fn omits_the_nav_row_and_supporter_line_when_unset() {
        // Its own fixture: the shared `legal_html` names a Foundation, which is
        // the state this test is the counterpart to.
        fn app() -> Element {
            rsx! {
                SiteFooterLegal {
                    copyright_holder: "Neon Law".to_string(),
                    disclaimer: "This is an attorney advertisement.".to_string(),
                    copyright_year: 2026,
                }
            }
        }
        let out = ssr(app);
        assert!(!out.contains("site-footer__nav"), "no empty row: {out}");
        assert!(
            !out.contains("proud supporter"),
            "no dangling sentence: {out}"
        );
    }

    /// A deploy that publishes an office in a state this component carries no
    /// tracing of renders the plain treatment rather than a stray box.
    #[test]
    fn omits_the_watermark_for_an_untraced_state() {
        assert!(super::state_outline("Idaho").is_none());
        assert!(super::state_outline("").is_none());
    }

    /// An office note is a qualification on one specific address, so it must
    /// render inside that office's own `<li>` — between the address it
    /// qualifies and the next city. A note that escapes into the neighbouring
    /// entry attaches a pending admission to the wrong jurisdiction.
    #[test]
    fn renders_an_office_note_beneath_the_address_it_qualifies() {
        let out = contactable_html();
        // The last line of the New York address, so the note has to follow the
        // whole of it rather than slotting between two of its lines.
        let address = out
            .find("New York, NY 10017")
            .expect("the New York address renders");
        let note = out
            .find("Bar admission pending")
            .expect("the New York note renders");
        let next_office = out.find("720 Seneca St").expect("the next office renders");
        assert!(
            address < note && note < next_office,
            "the note sits under its own address, before the next office: {out}"
        );
        assert!(
            out.contains(r#"class="site-footer__office-note""#),
            "the note is styled as a qualification rather than a line of the address: {out}"
        );
        // The unqualified offices publish no note element at all, rather than
        // an empty one that reserves space under every address.
        assert_eq!(
            out.matches("site-footer__office-note").count(),
            1,
            "only the qualified office renders a note: {out}"
        );
    }

    /// The bar number is the point of the licence line: it must render beside
    /// its jurisdiction and link to that bar's own record, so a visitor can
    /// verify the licence rather than trust the page.
    #[test]
    fn names_each_attorney_with_their_bar_number_and_record() {
        let out = contactable_html();
        assert!(
            out.contains("Nicholas Richard Shook"),
            "the licensed attorney is named: {out}"
        );
        assert!(
            out.contains("Nevada Bar No. 13400"),
            "the bar number renders beside its jurisdiction: {out}"
        );
        assert!(
            out.contains("nvbar.org/find-a-lawyer/?usearch=13400"),
            "the number links to the bar's own record: {out}"
        );
    }

    /// A deploy that publishes no contact channels renders no empty band — and
    /// its legal strip is still the footer's first child, so the CSS drops the
    /// second rule.
    #[test]
    fn omits_the_contact_band_when_nothing_is_published() {
        let out = legal_html();
        assert!(
            !out.contains("site-footer__contact"),
            "no contact band without contact details: {out}"
        );
        assert!(
            !out.contains("site-footer__licenses"),
            "no licence list without attorneys: {out}"
        );
        assert!(
            out.contains(r#"<div class="site-footer__legal""#),
            "the legal strip still renders: {out}"
        );
    }
}
