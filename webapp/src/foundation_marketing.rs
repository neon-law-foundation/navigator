//! The Foundation's public marketing surface: its home page (`/`) and the
//! three audience pages beneath it (`/education`, `/legal-aid`, `/attorneys`).
//!
//! These four pages are what a stranger sees when they arrive at
//! `/foundation`. They are the Foundation's own explanation of what it does —
//! it pairs legal aid centers with volunteer attorneys and AI, teaches the
//! CLEs, and gives every placed matter a case management workspace at no cost.
//!
//! **Why one module for four pages.** The home page and the audience pages are
//! the same handful of shapes in a different order: a hero, prose bands,
//! card grids, a numbered walk, and a closing call to action. Modelling those
//! shapes once ([`Band`]) and letting each page order them is what keeps a new
//! audience page a data change rather than a new component tree. The home page
//! keeps its own richer hero ([`HomeContent`]) because it carries the tagline
//! and the mission statement no audience page repeats.
//!
//! Copy lives in the Rust that renders it, per the workspace's English-only
//! rule: there is no catalog and no key lookup. The portal router resolves each
//! page's content at router-build time and injects it, so no page here resolves
//! per-request data.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{PublicShell, SiteHeader, SiteNavLink, SocialMeta};
use crate::public_chrome::{PublicChrome, PublicFooter};

/// The Foundation marketing stylesheet, hoisted alongside `theme.css` and the
/// Foundation's cyan token layer.
pub const FOUNDATION_MARKETING_STYLESHEET_HREF: &str = "/public/css/foundation-marketing.css";

/// One run of prose. `emphasis` renders it as `<strong>`.
///
/// Marketing copy leans on a bolded clause per paragraph — "the lawyer is
/// responsible, and the lawyer signs" — so a paragraph is a sequence of runs
/// rather than a string. Modelling it as data keeps the copy wasm-safe and
/// keeps markup out of the content.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct Run {
    pub text: String,
    pub emphasis: bool,
    /// When set, the run renders as an `<a>` to this href rather than as text.
    pub href: Option<String>,
}

impl Run {
    /// Plain prose.
    #[must_use]
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            emphasis: false,
            href: None,
        }
    }

    /// Prose the page bolds.
    #[must_use]
    pub fn strong(text: &str) -> Self {
        Self {
            text: text.to_string(),
            emphasis: true,
            href: None,
        }
    }

    /// An inline link — prose that navigates to `href`.
    #[must_use]
    pub fn link(text: &str, href: &str) -> Self {
        Self {
            text: text.to_string(),
            emphasis: false,
            href: Some(href.to_string()),
        }
    }
}

/// A paragraph: the runs that compose it.
pub type Paragraph = Vec<Run>;

/// One card in a card band — a program on the home page, or a commitment on an
/// audience page.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct Card {
    pub title: String,
    /// Short labels rendered as a chip row. Empty renders no row.
    pub chips: Vec<String>,
    pub body: Vec<Paragraph>,
    /// Optional deep link to the page that expands this card.
    pub href: Option<String>,
    pub href_label: Option<String>,
}

/// One entry in a numbered walk.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct Step {
    pub title: String,
    pub body: Vec<Paragraph>,
}

/// One horizontal band of a marketing page.
///
/// A page is an ordered list of these. Adding a band shape here is the only
/// way a page grows a new kind of section, which is what stops four pages from
/// drifting into four private layouts.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum Band {
    /// A centred statement: a large lead line over supporting prose. The
    /// mission band on the home page, and the opening argument on each
    /// audience page.
    Statement {
        /// Screen-reader heading for the band. Rendered visually hidden when
        /// `lead` is doing the visible work.
        heading: String,
        lead: String,
        body: Vec<Paragraph>,
    },
    /// A titled grid of cards.
    Cards {
        anchor: String,
        overline: String,
        heading: String,
        description: Option<String>,
        items: Vec<Card>,
    },
    /// A titled, numbered walk.
    Steps {
        anchor: String,
        overline: String,
        heading: String,
        description: Option<String>,
        items: Vec<Step>,
    },
    /// The closing call to action. The Foundation publishes one route in —
    /// its inbox — so this carries an address rather than a form.
    Cta {
        heading: String,
        body: Option<String>,
        email: String,
        /// Optional subject line prefilled in the recipient's email client.
        email_subject: Option<String>,
    },
}

/// The home page's hero: the name, the tagline, the standfirst, and the line
/// the Foundation closes its argument on.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct Hero {
    /// The badge above the title (`501(c)(3) nonprofit`). Empty renders none.
    pub badge: String,
    pub title: String,
    pub tagline: String,
    pub body: Vec<Paragraph>,
    /// The pulled-out line under the standfirst. Empty renders none.
    pub pullquote: String,
    /// The address the hero's one action opens.
    pub email: String,
}

/// Everything the Foundation home page renders.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct HomeContent {
    pub head_title: String,
    pub meta_description: String,
    pub hero: Hero,
    pub bands: Vec<Band>,
}

/// Everything one audience page renders. No hero badge and no pullquote: an
/// audience page opens on its argument, not on the Foundation's identity.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct PageContent {
    pub head_title: String,
    pub meta_description: String,
    /// The page's `<h1>`.
    pub title: String,
    /// The line under the title.
    pub tagline: String,
    pub bands: Vec<Band>,
}

/// The [`HomeContent`] the portal router injects for `/`.
#[derive(Clone, Default)]
pub struct InjectedFoundationHome(pub HomeContent);

/// The [`PageContent`] the portal router injects for one audience page.
#[derive(Clone, Default)]
pub struct InjectedFoundationPage(pub PageContent);

/// The home page's resolved view.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct FoundationHomeView {
    pub chrome: PublicChrome,
    pub content: HomeContent,
}

/// An audience page's resolved view.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct FoundationPageView {
    pub chrome: PublicChrome,
    pub content: PageContent,
}

/// Resolve the Foundation chrome and the static home copy.
#[server]
pub async fn foundation_home_view() -> Result<FoundationHomeView, ServerFnError> {
    let content = consume_context::<InjectedFoundationHome>().0;
    Ok(FoundationHomeView {
        chrome: crate::public_chrome::foundation_public_chrome_from_context().await,
        content,
    })
}

/// Resolve the Foundation chrome and one audience page's static copy.
#[server]
pub async fn foundation_page_view() -> Result<FoundationPageView, ServerFnError> {
    let content = consume_context::<InjectedFoundationPage>().0;
    Ok(FoundationPageView {
        chrome: crate::public_chrome::foundation_public_chrome_from_context().await,
        content,
    })
}

/// Resolve the **firm's** chrome and one marketing page's static copy.
///
/// Same band layout, other brand. The band vocabulary in this module is a
/// page-layout system rather than a Foundation-only one, so the firm's
/// `/navigator` page reuses it wholesale and differs in exactly one thing: it
/// resolves firm chrome, and so wears the firm's header and its regulated
/// footer instead of the nonprofit's.
#[server]
pub async fn firm_marketing_page_view() -> Result<FoundationPageView, ServerFnError> {
    let content = consume_context::<InjectedFoundationPage>().0;
    Ok(FoundationPageView {
        chrome: crate::public_chrome::firm_public_chrome_from_context().await,
        content,
    })
}

/// A firm marketing page's route entry.
#[component]
pub fn FirmMarketingPageEntry() -> Element {
    let resource = use_server_future(firm_marketing_page_view)?;
    let view = match &*resource.read() {
        Some(Ok(view)) => view.clone(),
        _ => return rsx! {},
    };
    rsx! {
        FoundationPage { chrome: view.chrome, content: view.content }
    }
}

/// The home page's route entry.
#[component]
pub fn FoundationHomeEntry() -> Element {
    let resource = use_server_future(foundation_home_view)?;
    let view = match &*resource.read() {
        Some(Ok(view)) => view.clone(),
        _ => return rsx! {},
    };
    rsx! {
        FoundationHome { chrome: view.chrome, content: view.content }
    }
}

/// An audience page's route entry.
#[component]
pub fn FoundationPageEntry() -> Element {
    let resource = use_server_future(foundation_page_view)?;
    let view = match &*resource.read() {
        Some(Ok(view)) => view.clone(),
        _ => return rsx! {},
    };
    rsx! {
        FoundationPage { chrome: view.chrome, content: view.content }
    }
}

/// The chrome both page components wrap their bands in.
///
/// Taken as rendered `Element`s for the same reason [`PublicShell`] does: the
/// brand is resolved server-side, and the shell never branches on it.
#[component]
fn MarketingShell(
    chrome: PublicChrome,
    title: String,
    description: String,
    children: Element,
) -> Element {
    let header = rsx! {
        SiteHeader {
            brand_name: chrome.brand_name.clone(),
            home_href: chrome.home_href.clone(),
            logo_href: chrome.logo_href.clone(),
            destinations: chrome
                .destinations
                .iter()
                .map(|link| SiteNavLink::new(link.label.clone(), link.href.clone()))
                .collect(),
            utility: chrome
                .utility
                .iter()
                .map(|link| SiteNavLink::new(link.label.clone(), link.href.clone()))
                .collect(),
        }
    };
    let footer = rsx! {
        PublicFooter { chrome: chrome.clone() }
    };
    rsx! {
        document::Title { "{title}" }
        document::Meta { name: "description", content: "{description}" }
        SocialMeta {
            title: title.clone(),
            description: description.clone(),
            site_name: chrome.brand_name.clone(),
            image: chrome.social_image.clone(),
        }
        // Only this page's own rules. The palette comes from the shared token
        // layer `PublicShell` hoists, and there is no per-brand colour
        // stylesheet to order against it.
        document::Stylesheet { href: FOUNDATION_MARKETING_STYLESHEET_HREF }
        PublicShell { header, footer, {children} }
    }
}

/// The Foundation home page. Prop-driven, so it server-renders and unit-tests
/// without a server future.
#[component]
pub fn FoundationHome(chrome: PublicChrome, content: HomeContent) -> Element {
    rsx! {
        MarketingShell {
            chrome: chrome.clone(),
            title: content.head_title.clone(),
            description: content.meta_description.clone(),
            section { class: "fm-hero",
                div { class: "fm-hero__inner",
                    if !content.hero.badge.is_empty() {
                        p { class: "fm-badge",
                            span { class: "fm-badge__dot", aria_hidden: "true" }
                            "{content.hero.badge}"
                        }
                    }
                    h1 { class: "fm-hero__title", "{content.hero.title}" }
                    p { class: "fm-hero__tagline", "{content.hero.tagline}" }
                    div { class: "fm-hero__body",
                        for paragraph in content.hero.body.iter() {
                            Prose { runs: paragraph.clone() }
                        }
                    }
                    if !content.hero.pullquote.is_empty() {
                        p { class: "fm-hero__pullquote", "{content.hero.pullquote}" }
                    }
                    MailAction { email: content.hero.email.clone() }
                }
            }
            Bands { items: content.bands.clone() }
        }
    }
}

/// One Foundation audience page.
#[component]
pub fn FoundationPage(chrome: PublicChrome, content: PageContent) -> Element {
    rsx! {
        MarketingShell {
            chrome: chrome.clone(),
            title: content.head_title.clone(),
            description: content.meta_description.clone(),
            section { class: "fm-hero fm-hero--page",
                div { class: "fm-hero__inner",
                    h1 { class: "fm-hero__title", "{content.title}" }
                    p { class: "fm-hero__tagline", "{content.tagline}" }
                }
            }
            Bands { items: content.bands.clone() }
        }
    }
}

/// Render a page's bands in order.
#[component]
fn Bands(items: Vec<Band>) -> Element {
    rsx! {
        for band in items.iter() {
            match band {
                Band::Statement { heading, lead, body } => rsx! {
                    section { class: "fm-band fm-band--statement",
                        div { class: "fm-band__inner",
                            h2 { class: "fm-visually-hidden", "{heading}" }
                            if !lead.is_empty() {
                                p { class: "fm-statement__lead", "{lead}" }
                            }
                            div { class: "fm-statement__body",
                                for paragraph in body.iter() {
                                    Prose { runs: paragraph.clone() }
                                }
                            }
                        }
                    }
                },
                Band::Cards { anchor, overline, heading, description, items } => rsx! {
                    section { class: "fm-band fm-band--cards", id: "{anchor}",
                        div { class: "fm-band__inner",
                            BandHeading {
                                overline: overline.clone(),
                                heading: heading.clone(),
                                description: description.clone(),
                            }
                            ul { class: "fm-cards",
                                for card in items.iter() {
                                    li { class: "fm-card",
                                        h3 { class: "fm-card__title", "{card.title}" }
                                        if !card.chips.is_empty() {
                                            ul { class: "fm-chips",
                                                for chip in card.chips.iter() {
                                                    li { class: "fm-chip", "{chip}" }
                                                }
                                            }
                                        }
                                        div { class: "fm-card__body",
                                            for paragraph in card.body.iter() {
                                                Prose { runs: paragraph.clone() }
                                            }
                                        }
                                        if let (Some(href), Some(label)) =
                                            (card.href.as_ref(), card.href_label.as_ref())
                                        {
                                            a { class: "fm-card__link", href: "{href}", "{label}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Band::Steps { anchor, overline, heading, description, items } => rsx! {
                    section { class: "fm-band fm-band--steps", id: "{anchor}",
                        div { class: "fm-band__inner",
                            BandHeading {
                                overline: overline.clone(),
                                heading: heading.clone(),
                                description: description.clone(),
                            }
                            ol { class: "fm-steps",
                                for step in items.iter() {
                                    li { class: "fm-step",
                                        h3 { class: "fm-step__title", "{step.title}" }
                                        div { class: "fm-step__body",
                                            for paragraph in step.body.iter() {
                                                Prose { runs: paragraph.clone() }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Band::Cta { heading, body, email, email_subject } => rsx! {
                    section { class: "fm-band fm-band--cta",
                        div { class: "fm-band__inner",
                            h2 { class: "fm-cta__heading", "{heading}" }
                            if let Some(body) = body.as_ref() {
                                p { class: "fm-cta__body", "{body}" }
                            }
                            MailAction { email: email.clone(), subject: email_subject.clone() }
                        }
                    }
                },
            }
        }
    }
}

/// A band's overline, heading, and optional standfirst.
#[component]
fn BandHeading(overline: String, heading: String, description: Option<String>) -> Element {
    rsx! {
        div { class: "fm-band__heading",
            p { class: "fm-overline", "{overline}" }
            h2 { class: "fm-band__title", "{heading}" }
            if let Some(description) = description.as_ref() {
                p { class: "fm-band__description", "{description}" }
            }
        }
    }
}

/// One paragraph of runs.
#[component]
fn Prose(runs: Paragraph) -> Element {
    rsx! {
        p {
            for run in runs.iter() {
                if let Some(href) = run.href.as_ref() {
                    a { class: "fm-prose__link", href: "{href}", "{run.text}" }
                } else if run.emphasis {
                    strong { "{run.text}" }
                } else {
                    "{run.text}"
                }
            }
        }
    }
}

/// The Foundation's one call to action: its inbox.
///
/// Rendered as a `mailto:` anchor rather than a form. The Foundation takes
/// intake by conversation, and a contact form on a nonprofit's front door
/// implies a queue it does not run.
#[component]
fn MailAction(email: String, subject: Option<String>) -> Element {
    let href = subject.map_or_else(
        || format!("mailto:{email}"),
        |subject| {
            let subject =
                percent_encoding::utf8_percent_encode(&subject, percent_encoding::NON_ALPHANUMERIC);
            format!("mailto:{email}?subject={subject}")
        },
    );
    rsx! {
        p { class: "fm-action",
            a { class: "fm-action__link", href: "{href}", "{email}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome() -> PublicChrome {
        PublicChrome {
            brand_name: "Neon Law Foundation".to_string(),
            home_href: "/".to_string(),
            logo_href: "/public/logo-foundation.svg".to_string(),
            social_image: "https://example.test/og.png".to_string(),
            ..PublicChrome::default()
        }
    }

    fn render(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    fn sample_home() -> HomeContent {
        HomeContent {
            head_title: "Neon Law Foundation".to_string(),
            meta_description: "A 501(c)(3).".to_string(),
            hero: Hero {
                badge: "501(c)(3) nonprofit".to_string(),
                title: "Neon Law Foundation".to_string(),
                tagline: "Everyone should be able to exercise their legal rights.".to_string(),
                body: vec![vec![
                    Run::plain("We pair centers with "),
                    Run::strong("volunteer attorneys"),
                ]],
                pullquote: "A lawyer does the deciding.".to_string(),
                email: "support@neonlaw.org".to_string(),
            },
            bands: vec![
                Band::Statement {
                    heading: "Our mission".to_string(),
                    lead: "Not a shortage of law. A shortage of hours.".to_string(),
                    body: vec![vec![Run::plain("Centers turn people away.")]],
                },
                Band::Cards {
                    anchor: "what-we-do".to_string(),
                    overline: "The programs".to_string(),
                    heading: "What we do".to_string(),
                    description: None,
                    items: vec![Card {
                        title: "Education and CLEs".to_string(),
                        chips: vec!["Continuing legal education".to_string()],
                        body: vec![vec![Run::plain("We teach it directly.")]],
                        href: Some("/education".to_string()),
                        href_label: Some("See the curriculum".to_string()),
                    }],
                },
                Band::Steps {
                    anchor: "how-it-works".to_string(),
                    overline: "The pairing".to_string(),
                    heading: "How it works".to_string(),
                    description: Some("From a matter to a resolution.".to_string()),
                    items: vec![Step {
                        title: "A center brings us a matter".to_string(),
                        body: vec![vec![Run::plain("From the center's own intake.")]],
                    }],
                },
                Band::Cta {
                    heading: "Tell us about the matter.".to_string(),
                    body: None,
                    email: "support@neonlaw.org".to_string(),
                    email_subject: None,
                },
            ],
        }
    }

    fn home_html() -> String {
        fn app() -> Element {
            rsx! { FoundationHome { chrome: chrome(), content: sample_home() } }
        }
        render(app)
    }

    #[test]
    fn home_renders_the_hero_identity_and_tagline() {
        let out = home_html();
        assert!(out.contains("501(c)(3) nonprofit"), "badge: {out}");
        assert!(out.contains("<h1"), "the home page owns an h1: {out}");
        assert!(
            out.contains("Everyone should be able to exercise their legal rights."),
            "tagline: {out}"
        );
    }

    #[test]
    fn home_bolds_the_runs_marked_for_emphasis() {
        let out = home_html();
        assert!(
            out.contains("<strong>volunteer attorneys</strong>"),
            "an emphasised run renders as <strong>: {out}"
        );
    }

    #[test]
    fn linked_runs_render_as_inline_anchors_and_empty_leads_are_skipped() {
        fn app() -> Element {
            rsx! {
                Bands {
                    items: vec![Band::Statement {
                        heading: "Business filings".to_string(),
                        lead: String::new(),
                        body: vec![vec![
                            Run::plain("Business filings included in our "),
                            Run::link("fractional GC", "/fractional-gc"),
                            Run::plain(" projects."),
                        ]],
                    }],
                }
            }
        }
        let out = render(app);
        assert!(
            out.contains(r#"class="fm-prose__link""#) && out.contains(r#"href="/fractional-gc""#),
            "an inline link run renders as an anchor: {out}"
        );
        assert!(
            out.contains("fractional GC</a>"),
            "the anchor carries the run's text: {out}"
        );
        assert!(
            !out.contains(r#"class="fm-statement__lead""#),
            "an empty lead renders no paragraph: {out}"
        );
    }

    #[test]
    fn home_renders_every_band_in_order() {
        let out = home_html();
        let statement = out.find("shortage of hours").expect("statement band");
        let cards = out.find("What we do").expect("cards band");
        let steps = out.find("How it works").expect("steps band");
        let cta = out.find("Tell us about the matter.").expect("cta band");
        assert!(
            statement < cards && cards < steps && steps < cta,
            "bands render in the order the content lists them: {out}"
        );
    }

    #[test]
    fn card_bands_carry_their_anchor_so_the_nav_can_link_them() {
        // The header links `/#what-we-do` and `/#how-it-works`. A band that
        // renders no id turns both into no-ops that scroll nowhere.
        let out = home_html();
        assert!(out.contains(r#"id="what-we-do""#), "cards anchor: {out}");
        assert!(out.contains(r#"id="how-it-works""#), "steps anchor: {out}");
    }

    #[test]
    fn a_card_deep_links_to_the_page_that_expands_it() {
        let out = home_html();
        assert!(
            out.contains(r#"href="/education""#),
            "the program card links its audience page: {out}"
        );
        assert!(out.contains("See the curriculum"), "link label: {out}");
    }

    #[test]
    fn every_call_to_action_opens_the_foundations_inbox() {
        // The Foundation publishes one route in. A CTA that renders no
        // `mailto:` is a dead end on the page whose whole job is to start a
        // conversation.
        let out = home_html();
        assert_eq!(
            out.matches(r#"href="mailto:support@neonlaw.org""#).count(),
            2,
            "the hero and the closing CTA both open the inbox: {out}"
        );
    }

    #[test]
    fn a_call_to_action_can_prefill_an_email_subject() {
        fn app() -> Element {
            rsx! { MailAction {
                email: "contact@example.com".to_string(),
                subject: Some("Co-Counseling for Good with AI".to_string()),
            } }
        }

        let out = render(app);
        assert!(
            out.contains(
                r#"href="mailto:contact@example.com?subject=Co%2DCounseling%20for%20Good%20with%20AI""#
            ),
            "the mailto link carries the supplied subject: {out}"
        );
    }

    #[test]
    fn the_steps_band_is_an_ordered_list() {
        // "How it works" is a sequence, and a screen reader should hear it as
        // one. `<ul>` would drop the ordering the copy depends on.
        let out = home_html();
        assert!(out.contains(r#"<ol class="fm-steps""#), "ordered: {out}");
    }

    #[test]
    fn an_audience_page_renders_its_title_and_bands_without_a_badge() {
        fn app() -> Element {
            let content = PageContent {
                head_title: "For attorneys — Neon Law Foundation".to_string(),
                meta_description: "Take a pro bono matter.".to_string(),
                title: "For volunteer attorneys".to_string(),
                tagline: "Work that arrives scoped.".to_string(),
                bands: vec![Band::Cta {
                    heading: "Take a matter.".to_string(),
                    body: Some("Tell us your practice areas.".to_string()),
                    email: "support@neonlaw.org".to_string(),
                    email_subject: None,
                }],
            };
            rsx! { FoundationPage { chrome: chrome(), content } }
        }
        let out = render(app);
        assert!(out.contains("For volunteer attorneys"), "title: {out}");
        assert!(out.contains("Work that arrives scoped."), "tagline: {out}");
        assert!(out.contains("Take a matter."), "cta: {out}");
        assert!(
            !out.contains("fm-badge"),
            "an audience page opens on its argument, not the 501(c)(3) badge: {out}"
        );
    }
}
