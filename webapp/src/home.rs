//! The firm home page (`/`) — the photographic hero the practice statement sits
//! on, then the practice statement over the three practice cards.
//!
//! The page still publishes no service catalog: the firm takes litigation and
//! flat-fee transactional work, and every fee is quoted through `/contact`. It
//! does carry the firm's own presentation of those practices — the litigation
//! card with its record and the areas it litigates, then the transactional and
//! regulatory cards under it — which is the section `www.neonlaw.com` opens
//! "What we do" with. The only state is the static
//! copy ([`HomeContent`]), resolved by the portal router at router-build time
//! and injected via `ServeConfig::context_providers`; the page resolves no
//! per-request data.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{PublicShell, SiteHeader, SiteNavLink, SocialMeta};
use crate::public_chrome::{PublicChrome, PublicFooter};

/// The self-contained home stylesheet, hoisted alongside `theme.css`.
pub const HOME_STYLESHEET_HREF: &str = "/public/css/home.css";

/// One run of practice prose; `emphasis` renders it as `<strong>`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct CopyRun {
    pub text: String,
    pub emphasis: bool,
}

/// The litigation header — the eyebrow, the practice name, the areas litigated,
/// and the prose under them.
///
/// There is deliberately no record strip. The figures it carried were claims
/// about the firm's own matters, and every one of them has now come off the
/// page: see `home_publishes_no_amount_in_controversy_and_no_co_counsel_claim`
/// in `server/tests/firm_routes.rs` for the ones that are guarded against
/// returning.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct LitigationHeader {
    pub eyebrow: String,
    pub heading: String,
    pub areas: Vec<String>,
    pub body: Vec<Vec<CopyRun>>,
}

/// Which mark a practice card carries. Presentation data rather than markup, so
/// the copy stays wasm-safe and the drawing stays in the view.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum PracticeMark {
    /// The shield and check, for the counsel the firm keeps on retainer.
    #[default]
    Shield,
    /// The globe, for work that crosses jurisdictions.
    Globe,
}

/// One practice under the litigation header — the mark, the practice name, the
/// areas it covers, and the prose under them. Areas of practice, not a priced
/// catalog: every fee is still quoted through `/contact`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct PracticeCard {
    pub mark: PracticeMark,
    pub heading: String,
    pub areas: Vec<String>,
    pub body: Vec<Vec<CopyRun>>,
}

/// One `<source>` of the hero `<picture>` — the MIME type the browser tests
/// for, and the width-keyed candidates it chooses from.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct HeroSource {
    pub mime: String,
    pub srcset: String,
}

/// The hero photograph, resolved to plain URLs.
///
/// Resolved server-side rather than here: the variant URLs come from
/// `views::assets`, which reads `NAVIGATOR_ASSET_BASE_URL` to decide whether
/// the bytes live on the local `/public` mount or in the deployment's public
/// assets bucket. A wasm view cannot answer that question, so the router
/// answers it once and injects the result.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct HeroPicture {
    /// `<source>` elements in negotiation order: AVIF, WebP, JPEG.
    pub sources: Vec<HeroSource>,
    /// The `<img>` `src` every browser understands.
    pub fallback_src: String,
    /// What the photograph shows. A real description rather than an empty
    /// `alt`: the picture is the page's subject, not decoration behind it.
    pub alt: String,
    pub sizes: String,
}

/// The static home copy — resolved brand-safely at router-build time and
/// injected into the render context.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct HomeContent {
    pub head_title: String,
    pub meta_description: String,
    /// The hero photograph the page opens on. `None` when the deployment
    /// publishes no hero: the page then opens on the statement alone
    /// rather than over a broken image.
    pub hero: Option<HeroPicture>,
    /// The practice statement under the hero.
    pub heading: String,
    pub lead: String,
    pub contact_href: String,
    pub contact_label: String,
    pub litigation: LitigationHeader,
    /// The practices listed under the litigation header, in site order.
    pub practices: Vec<PracticeCard>,
}

/// The [`HomeContent`] injected into the render context by the portal router.
#[derive(Clone, Default)]
pub struct InjectedHome(pub HomeContent);

/// Everything the page renders.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct HomePageView {
    pub chrome: PublicChrome,
    pub content: HomeContent,
}

/// Resolve the chrome and the static home content.
#[server]
pub async fn home_page_view() -> Result<HomePageView, ServerFnError> {
    let content = consume_context::<InjectedHome>().0;
    Ok(HomePageView {
        chrome: crate::public_chrome::firm_public_chrome_from_context().await,
        content,
    })
}

/// The page's route entry.
#[component]
pub fn HomePageEntry() -> Element {
    let resource = use_server_future(home_page_view)?;
    let view = match &*resource.read() {
        Some(Ok(view)) => view.clone(),
        _ => return rsx! {},
    };
    rsx! {
        HomePage { chrome: view.chrome, content: view.content }
    }
}

/// The pure home page. Prop-driven, so it server-renders and unit-tests without
/// a server future.
#[component]
pub fn HomePage(chrome: PublicChrome, content: HomeContent) -> Element {
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
        document::Title { "{content.head_title}" }
        document::Meta { name: "description", content: "{content.meta_description}" }
        SocialMeta {
            title: content.head_title.clone(),
            description: content.meta_description.clone(),
            site_name: chrome.brand_name.clone(),
            image: chrome.social_image.clone(),
        }
        document::Stylesheet { href: crate::brand_style::BRAND_STYLESHEET_HREF }
        document::Stylesheet { href: HOME_STYLESHEET_HREF }
        PublicShell { header, footer,
            // The hero: the photograph, and nothing over it. The wordmark used
            // to sit on the picture, which said the firm's name a third time —
            // the header mark and the browser tab already do — and cost the
            // photograph its middle. The page's `<h1>` is the practice
            // statement below it, which is the first thing on the page that
            // says something a reader does not already know.
            section { class: "home-hero",
                if let Some(hero) = content.hero.as_ref() {
                    picture { class: "home-hero__picture",
                        for source in hero.sources.iter() {
                            // `srcset`/`sizes` are not in Dioxus's `source`
                            // element definition, so they are written as raw
                            // attributes rather than typed ones.
                            source {
                                r#type: "{source.mime}",
                                "srcset": "{source.srcset}",
                                "sizes": "{hero.sizes}",
                            }
                        }
                        img {
                            class: "home-hero__image",
                            src: "{hero.fallback_src}",
                            alt: "{hero.alt}",
                            sizes: "{hero.sizes}",
                            // The hero is the largest paint on the page; keep
                            // it out of lazy loading so it is not deferred
                            // behind the fold.
                            fetchpriority: "high",
                        }
                    }
                }
            }
            section { class: "home-statement",
                // No glow behind the statement. The hero above it is now the
                // page's decoration, and the wash bled past the photograph's
                // edge into the margin, which read as a rendering fault rather
                // than as lighting. The litigation and transactional pages keep
                // theirs — they open on type, not on a picture.
                h1 { class: "home-statement__heading", "{content.heading}" }
                p { class: "home-statement__lead", "{content.lead}" }
                a {
                    class: "nav-btn nav-btn--primary home-statement__cta",
                    href: "{content.contact_href}",
                    "{content.contact_label}"
                }
            }
            LitigationSection { content: content.litigation.clone() }
            if !content.practices.is_empty() {
                div { class: "practice-grid",
                    for (index , card) in content.practices.iter().enumerate() {
                        PracticeSection { index, card: card.clone() }
                    }
                }
            }
        }
    }
}

/// The litigation header: the practice the firm leads with, its record, and the
/// areas it litigates.
#[component]
fn LitigationSection(content: LitigationHeader) -> Element {
    rsx! {
        section { class: "neon-card litigation", "aria-labelledby": "litigation-heading",
            div { class: "litigation__head",
                div { class: "litigation__mark", "aria-hidden": "true",
                    // The scales of justice, drawn at the card's text colour.
                    svg { "viewBox": "0 0 24 24", fill: "none", stroke: "currentColor",
                        "stroke-width": "1.25", "stroke-linecap": "round",
                        "stroke-linejoin": "round",
                        path { d: "m16 16 3-8 3 8c-.87.65-1.92 1-3 1s-2.13-.35-3-1Z" }
                        path { d: "m2 16 3-8 3 8c-.87.65-1.92 1-3 1s-2.13-.35-3-1Z" }
                        path { d: "M7 21h10" }
                        path { d: "M12 3v18" }
                        path { d: "M3 7h2c2 0 5-1 7-2 2 1 5 2 7 2h2" }
                    }
                }
                div {
                    p { class: "firm-eyebrow", "{content.eyebrow}" }
                    h2 { id: "litigation-heading", class: "litigation__heading", "{content.heading}" }
                }
            }
            div { class: "litigation__detail",
                ul { class: "firm-chips",
                    for area in content.areas.iter() {
                        li { class: "firm-chip", "{area}" }
                    }
                }
                for paragraph in content.body.iter() {
                    p { class: "litigation__paragraph",
                        for run in paragraph.iter() {
                            if run.emphasis {
                                strong { "{run.text}" }
                            } else {
                                "{run.text}"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One practice under the litigation header: its mark, the areas it covers, and
/// the prose under them. `index` names the heading the card labels itself by.
#[component]
fn PracticeSection(index: usize, card: PracticeCard) -> Element {
    let heading_id = format!("practice-heading-{index}");
    rsx! {
        section { class: "neon-card practice", "aria-labelledby": "{heading_id}",
            div { class: "practice__head",
                div { class: "practice__mark", "aria-hidden": "true",
                    svg { "viewBox": "0 0 24 24", fill: "none", stroke: "currentColor",
                        "stroke-width": "1.25", "stroke-linecap": "round",
                        "stroke-linejoin": "round",
                        match card.mark {
                            // A shield closed by a check.
                            PracticeMark::Shield => rsx! {
                                path { d: "M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67 0C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1Z" }
                                path { d: "m9 12 2 2 4-4" }
                            },
                            // A globe: the meridian and the equator.
                            PracticeMark::Globe => rsx! {
                                circle { cx: "12", cy: "12", r: "10" }
                                path { d: "M12 2a15 15 0 0 0 0 20 15 15 0 0 0 0-20" }
                                path { d: "M2 12h20" }
                            },
                        }
                    }
                }
                h3 { id: "{heading_id}", class: "practice__heading", "{card.heading}" }
            }
            ul { class: "firm-chips",
                for area in card.areas.iter() {
                    li { class: "firm-chip", "{area}" }
                }
            }
            for paragraph in card.body.iter() {
                p { class: "practice__paragraph",
                    for run in paragraph.iter() {
                        if run.emphasis {
                            strong { "{run.text}" }
                        } else {
                            "{run.text}"
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

    fn html() -> String {
        fn app() -> Element {
            rsx! {
                HomePage {
                    chrome: PublicChrome::default(),
                    content: HomeContent {
                        practices: vec![
                            PracticeCard {
                                mark: PracticeMark::Shield,
                                heading: "Fractional general counsel & transactional".to_string(),
                                areas: vec!["Financings".to_string()],
                                body: vec![vec![
                                    CopyRun {
                                        text: "We serve as ".to_string(),
                                        emphasis: false,
                                    },
                                    CopyRun {
                                        text: "fractional outside general counsel".to_string(),
                                        emphasis: true,
                                    },
                                ]],
                            },
                            PracticeCard {
                                mark: PracticeMark::Globe,
                                heading: "AI & regulatory counseling".to_string(),
                                areas: vec!["National security".to_string()],
                                body: vec![vec![CopyRun {
                                    text: "We counsel clients on AI regulation.".to_string(),
                                    emphasis: false,
                                }]],
                            },
                        ],
                        head_title: "Home".to_string(),
                        meta_description: "Litigation and flat-fee transactional work."
                            .to_string(),
                        hero: Some(HeroPicture {
                            sources: vec![
                                HeroSource {
                                    mime: "image/avif".to_string(),
                                    srcset: "/public/img/berkeley-bay/berkeley-bay-400w.avif 400w, \
                                             /public/img/berkeley-bay/berkeley-bay-1200w.avif 1200w"
                                        .to_string(),
                                },
                                HeroSource {
                                    mime: "image/jpeg".to_string(),
                                    srcset: "/public/img/berkeley-bay/berkeley-bay-1200w.jpg 1200w"
                                        .to_string(),
                                },
                            ],
                            fallback_src: "/public/img/berkeley-bay/berkeley-bay-1200w.jpg"
                                .to_string(),
                            alt: "The San Francisco Bay seen from the Berkeley hills.".to_string(),
                            sizes: "100vw".to_string(),
                        }),
                        heading: "Litigation and flat-fee transactional work".to_string(),
                        lead: "Every fee is quoted per engagement.".to_string(),
                        contact_href: "/contact".to_string(),
                        contact_label: "Contact us".to_string(),
                        litigation: LitigationHeader {
                            eyebrow: "The practice".to_string(),
                            heading: "Litigation".to_string(),
                            areas: vec!["Trade secrets & trademarks".to_string()],
                            body: vec![vec![
                                CopyRun {
                                    text: "We are comfortable on both sides of the v: "
                                        .to_string(),
                                    emphasis: false,
                                },
                                CopyRun {
                                    text: "Plaintiff and Defense".to_string(),
                                    emphasis: true,
                                },
                            ]],
                        },
                    },
                }
            }
        }
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn renders_the_practice_statement_and_contact_cta() {
        let out = html();
        assert!(
            out.contains("Litigation and flat-fee transactional work"),
            "the practice statement: {out}"
        );
        assert!(out.contains("Every fee is quoted per engagement."), "lead");
        assert!(out.contains(r#"href="/contact""#), "CTA links to /contact");
        assert!(out.contains("Contact us"), "CTA label");
    }

    /// The photograph carries no text. The wordmark used to sit on it, which
    /// said the firm's name a third time — the header mark and the browser tab
    /// already do — and cost the picture its middle. The page's one `<h1>` is
    /// therefore the practice statement, which is the first thing on the page
    /// that tells a reader something they did not already know.
    #[test]
    fn the_page_h1_is_the_practice_statement_and_the_photograph_carries_no_text() {
        let out = html();
        assert_eq!(out.matches("<h1").count(), 1, "one h1: {out}");
        assert!(
            out.contains(r#"<h1 class="home-statement__heading""#),
            "the h1 is the statement: {out}"
        );
        for gone in ["home-hero__wordmark", "home-hero__scrim"] {
            assert!(!out.contains(gone), "{gone} is gone: {out}");
        }
        let hero = out.find("home-hero__picture").expect("the photograph");
        let statement = out.find("home-statement").expect("the statement");
        assert!(hero < statement, "the photograph leads the page: {out}");
    }

    #[test]
    fn the_hero_photograph_renders_responsively_with_a_real_description() {
        let out = html();
        // A `<picture>`, not a bare `<img>`: the hero is the page's largest
        // paint, and a phone must not download the 1200px variant.
        assert!(
            out.contains("<picture"),
            "the hero negotiates formats: {out}"
        );
        assert!(
            out.contains(r#"type="image/avif""#),
            "AVIF is offered first: {out}"
        );
        assert!(
            out.contains("berkeley-bay-400w.avif 400w"),
            "the candidates are keyed by width: {out}"
        );
        assert!(
            out.contains(r#"src="/public/img/berkeley-bay/berkeley-bay-1200w.jpg""#),
            "the <img> fallback is the JPEG every browser reads: {out}"
        );
        assert!(
            out.contains(r#"alt="The San Francisco Bay seen from the Berkeley hills.""#),
            "the photograph is described, not hidden behind an empty alt: {out}"
        );
    }

    #[test]
    fn a_deployment_with_no_published_hero_opens_on_the_statement() {
        // The bytes live in a bucket, not in git, so an unpublished hero is a
        // real state rather than a bug — and it must degrade to the statement on
        // the brand surface, never to a broken image.
        fn app() -> Element {
            rsx! {
                HomePage {
                    chrome: PublicChrome::default(),
                    content: HomeContent {
                        ..HomeContent::default()
                    },
                }
            }
        }
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        let out = dioxus_ssr::render(&dom);
        assert!(!out.contains("<picture"), "no empty picture: {out}");
        assert!(!out.contains("home-hero__scrim"), "no scrim: {out}");
        assert!(
            out.contains("home-statement__heading"),
            "the statement still leads: {out}"
        );
    }

    #[test]
    fn renders_the_litigation_header_with_its_areas_and_no_record_strip() {
        let out = html();
        assert!(out.contains("The practice"), "eyebrow: {out}");
        assert!(out.contains(">Litigation<"), "the practice name");
        assert!(out.contains("Trade secrets"), "an area chip: {out}");
        // The record strip is gone, and the header must not leave an empty
        // bordered `<dl>` where it used to rule off the areas below it.
        assert!(
            !out.contains("litigation__stats"),
            "no record strip renders: {out}"
        );
    }

    #[test]
    fn litigation_prose_emphasises_the_phrases_the_firm_sets_in_bold() {
        let out = html();
        assert!(
            out.contains("<strong>Plaintiff and Defense</strong>"),
            "the emphasised phrase is bold: {out}"
        );
        assert!(
            !out.contains("<strong>We are comfortable"),
            "the plain run stays plain: {out}"
        );
    }

    #[test]
    fn renders_the_two_practice_cards_under_the_litigation_header() {
        let out = html();
        // SSR escapes the ampersand in a practice name, so assert the escaped
        // spelling rather than the source one.
        assert!(
            out.contains("Fractional general counsel &#38; transactional"),
            "the transactional practice: {out}"
        );
        assert!(
            out.contains("AI &#38; regulatory counseling"),
            "the regulatory practice: {out}"
        );
        assert!(
            out.contains("Financings"),
            "a transactional area chip: {out}"
        );
        assert!(
            out.contains("National security"),
            "a regulatory area chip: {out}"
        );
        assert!(
            out.contains("<strong>fractional outside general counsel</strong>"),
            "practice prose emphasises the phrases the firm sets in bold: {out}"
        );
    }

    #[test]
    fn the_three_practices_are_three_cards_the_litigation_one_first() {
        let out = html();
        assert_eq!(
            out.matches("neon-card").count(),
            3,
            "one card per practice: {out}"
        );
        let litigation = out.find("litigation__heading").expect("litigation card");
        let practices = out.find("practice-grid").expect("the practice grid");
        assert!(
            litigation < practices,
            "litigation leads and the other two sit under it: {out}"
        );
    }

    #[test]
    fn each_practice_card_labels_itself_by_its_own_heading() {
        let out = html();
        for id in ["practice-heading-0", "practice-heading-1"] {
            assert!(
                out.contains(&format!(r#"aria-labelledby="{id}""#)),
                "{id} labels its card: {out}"
            );
            assert!(out.contains(&format!(r#"id="{id}""#)), "{id} exists: {out}");
        }
    }

    #[test]
    fn the_practice_grid_stays_out_of_the_markup_when_there_are_no_cards() {
        fn app() -> Element {
            rsx! {
                HomePage {
                    chrome: PublicChrome::default(),
                    content: HomeContent::default(),
                }
            }
        }
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        let out = dioxus_ssr::render(&dom);
        assert!(!out.contains("practice-grid"), "no empty grid: {out}");
    }

    #[test]
    fn the_statement_carries_no_glow_behind_it() {
        // The wash bled past the hero photograph's edge into the page margin,
        // which reads as a rendering fault. The photograph is the page's
        // decoration now; this pins that the glow does not come back with the
        // next copy edit.
        let out = html();
        assert!(
            !out.contains("firm-glow"),
            "no glow on the home page: {out}"
        );
    }

    #[test]
    fn wraps_the_page_in_the_public_shell_chrome() {
        let out = html();
        assert!(out.contains("site-header"), "header chrome: {out}");
        assert!(out.contains("site-footer__legal"), "footer chrome");
    }
}
