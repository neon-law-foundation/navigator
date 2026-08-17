//! `/show-and-tell` — the paginated archive of Nebula
//! gatherings, migrated to Dioxus SSR (#956 Phase 4).
//!
//! The
//! page carries **two independent pagers** — upcoming and past — so each page
//! link has to preserve the other list's position. That is why both pagers
//! render through [`crate::components::Pagination`] with its `page_param` set:
//! one writes `?upcoming_page=`, the other `?past_page=`, and each carries the
//! other as `extra_query`.
//!
//! Resolved per request: "upcoming" and "past" are relative to today, and the
//! page numbers come from the query string.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{
    NebulaHero, Pagination, PublicShell, SiteHeader, SiteNavLink, NEBULA_STYLESHEET_HREF,
};
use crate::public_chrome::{PublicChrome, PublicFooter};

/// This page's own path. The single definition of the archive URL: the site
/// nav links to it, a gathering's detail page links back to it, and every pager
/// link here is built from it.
///
/// Under `/foundation` because the gatherings are the Foundation's, and one
/// binary now serves both faces: the firm holds the site root, so every
/// Foundation surface is reachable by its own prefix.
pub const SHOW_TELL_INDEX_PATH: &str = "/foundation/show-and-tell";

/// One gathering's own page, beneath the index.
///
/// Named here rather than written as a literal at the mount, because the
/// retired `/show-and-tell/{slug}` URL is a `301` in the same binary now. Two
/// spellings of this path meant the redirect and the page both claimed it, and
/// Axum rejects an overlapping route at composition time — a panic at boot, not
/// a `404` in production, but a whole-site outage either way.
pub const SHOW_TELL_DETAIL_PATH: &str = "/foundation/show-and-tell/{slug}";

/// The query parameter each list pages on.
pub const UPCOMING_PAGE_PARAM: &str = "upcoming_page";
pub const PAST_PAGE_PARAM: &str = "past_page";

/// One gathering's card in either list.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ShowTellCard {
    pub detail_href: String,
    pub title: String,
    /// The formatted local date/time range, pre-rendered portal-side.
    pub time: String,
    pub description: String,
    pub image_url: Option<String>,
    pub image_alt: String,
}

/// One list plus where it sits in its own pagination.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ShowTellList {
    pub cards: Vec<ShowTellCard>,
    pub current_page: u32,
    pub total_pages: u32,
}

/// The index's resolved content, built per request by the portal pre-layer.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ShowTellIndexContent {
    pub upcoming: ShowTellList,
    pub past: ShowTellList,
}

/// The [`ShowTellIndexContent`] the portal pre-layer injects.
#[derive(Clone, Default)]
pub struct InjectedShowTellIndex(pub ShowTellIndexContent);

/// Everything the page renders.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ShowTellIndexView {
    pub chrome: PublicChrome,
    pub content: ShowTellIndexContent,
}

const INDEX_TITLE: &str = "Show-and-tell events";
const INDEX_DESCRIPTION: &str =
    "Upcoming and past Nebula show-and-tell events from the Neon Law Foundation.";
const INDEX_LEDE: &str =
    "Practical Nebula gatherings for lawyers and legal professionals building with AI, workflows, \
     and Neon Law Navigator.";

/// Resolve the Foundation chrome and the per-request paginated lists.
#[server]
pub async fn show_tell_index_view() -> Result<ShowTellIndexView, ServerFnError> {
    let content = dioxus_fullstack_core::FullstackContext::extract::<
        axum::Extension<InjectedShowTellIndex>,
        _,
    >()
    .await
    .map(|axum::Extension(c)| c.0)
    .unwrap_or_default();
    Ok(ShowTellIndexView {
        chrome: crate::public_chrome::foundation_public_chrome_from_context().await,
        content,
    })
}

/// The page's route entry.
#[component]
pub fn ShowTellIndexEntry() -> Element {
    let resource = use_server_future(show_tell_index_view)?;
    let view = match &*resource.read() {
        Some(Ok(view)) => view.clone(),
        _ => return rsx! {},
    };
    rsx! {
        ShowTellIndexPage { chrome: view.chrome, content: view.content }
    }
}

/// The pure index page.
#[component]
pub fn ShowTellIndexPage(chrome: PublicChrome, content: ShowTellIndexContent) -> Element {
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
    // Each pager carries the *other* list's page so a click never resets it.
    let upcoming_extra = vec![(
        PAST_PAGE_PARAM.to_string(),
        content.past.current_page.to_string(),
    )];
    let past_extra = vec![(
        UPCOMING_PAGE_PARAM.to_string(),
        content.upcoming.current_page.to_string(),
    )];
    rsx! {
        document::Title { "{chrome.brand_name} | Nebula show-and-tell events" }
        document::Meta { name: "description", content: INDEX_DESCRIPTION }
        document::Stylesheet { href: NEBULA_STYLESHEET_HREF }
        PublicShell { header, footer,
            NebulaHero {
                eyebrow: chrome.foundation_name.clone(),
                title: INDEX_TITLE.to_string(),
                lede: INDEX_LEDE.to_string(),
            }
            section { class: "show-tell-section",
                h2 { "Upcoming" }
                p { class: "show-tell-section__note", "Today forward, nearest first." }
                if content.upcoming.cards.is_empty() {
                    p { class: "nebula-empty", "No upcoming show-and-tells are scheduled yet." }
                } else {
                    ShowTellGrid { cards: content.upcoming.cards.clone() }
                    Pagination {
                        current: content.upcoming.current_page,
                        total: content.upcoming.total_pages,
                        base_path: SHOW_TELL_INDEX_PATH.to_string(),
                        page_param: UPCOMING_PAGE_PARAM.to_string(),
                        extra_query: upcoming_extra,
                    }
                }
            }
            section { class: "show-tell-section",
                h2 { "Past" }
                p { class: "show-tell-section__note", "Earlier gatherings, newest first." }
                if content.past.cards.is_empty() {
                    p { class: "nebula-empty", "No past show-and-tells yet." }
                } else {
                    ShowTellGrid { cards: content.past.cards.clone() }
                    Pagination {
                        current: content.past.current_page,
                        total: content.past.total_pages,
                        base_path: SHOW_TELL_INDEX_PATH.to_string(),
                        page_param: PAST_PAGE_PARAM.to_string(),
                        extra_query: past_extra,
                    }
                }
            }
        }
    }
}

/// A vertical list of horizontal gathering cards.
#[component]
fn ShowTellGrid(cards: Vec<ShowTellCard>) -> Element {
    rsx! {
        div { class: "show-tell-grid",
            for card in cards.iter() {
                article { class: "show-tell-card",
                    if let Some(image_url) = card.image_url.clone() {
                        a { class: "show-tell-card-media", href: "{card.detail_href}",
                            img {
                                src: "{image_url}",
                                alt: "{card.image_alt}",
                                loading: "lazy",
                                decoding: "async",
                            }
                        }
                    }
                    div { class: "show-tell-card-body",
                        p { class: "nebula-eyebrow", "{card.time}" }
                        h3 {
                            a { href: "{card.detail_href}", "{card.title}" }
                        }
                        p { "{card.description}" }
                        div {
                            a { class: "nav-btn nav-btn--primary", href: "{card.detail_href}", "View event" }
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

    fn card(title: &str, image: Option<&str>) -> ShowTellCard {
        ShowTellCard {
            detail_href: format!("/show-and-tell/{}", title.to_lowercase()),
            title: title.to_string(),
            time: "March 3, 2026, 5:00–6:30 PM PST".to_string(),
            description: "What we showed.".to_string(),
            image_url: image.map(str::to_string),
            image_alt: format!("{title} cover"),
        }
    }

    fn populated() -> ShowTellIndexContent {
        ShowTellIndexContent {
            upcoming: ShowTellList {
                cards: vec![card("March", Some("/public/img/e/march.avif"))],
                current_page: 2,
                total_pages: 3,
            },
            past: ShowTellList {
                cards: vec![card("January", None)],
                current_page: 1,
                total_pages: 4,
            },
        }
    }

    fn html() -> String {
        fn app() -> Element {
            rsx! {
                ShowTellIndexPage { chrome: PublicChrome::default(), content: populated() }
            }
        }
        ssr(app)
    }

    #[test]
    fn lists_upcoming_and_past_gatherings_under_their_own_headings() {
        let out = html();
        assert!(out.contains(">Upcoming<"), "upcoming heading: {out}");
        assert!(out.contains(">Past<"), "past heading: {out}");
        assert!(out.contains("March"), "upcoming card: {out}");
        assert!(out.contains("January"), "past card: {out}");
    }

    #[test]
    fn each_pager_preserves_the_other_lists_page() {
        // The two lists page independently. If a pager dropped the other's
        // parameter, clicking "Next" on Upcoming would silently reset Past to
        // page 1 — a state loss no single-pager test would notice.
        let out = html();
        // Upcoming is on page 2 of 3, so its Next goes to 3 and carries past=1.
        assert!(
            out.contains("upcoming_page=3") && out.contains("past_page=1"),
            "the upcoming pager carries the past page: {out}"
        );
        // Past is on page 1 of 4, so its Next goes to 2 and carries upcoming=2.
        assert!(
            out.contains("past_page=2") && out.contains("upcoming_page=2"),
            "the past pager carries the upcoming page: {out}"
        );
    }

    #[test]
    fn a_card_without_an_image_renders_no_media_link() {
        // The body then spans the full card width (`:only-child` in the CSS).
        let out = html();
        // Exactly one card has an image, so exactly one media link renders.
        assert_eq!(
            out.matches("show-tell-card-media").count(),
            1,
            "only the imaged card gets a media link: {out}"
        );
        assert!(
            out.contains(r#"src="/public/img/e/march.avif""#),
            "the imaged card renders its cover: {out}"
        );
    }

    #[test]
    fn card_images_are_lazy_and_carry_alt_text() {
        let out = html();
        assert!(out.contains(r#"loading="lazy""#), "lazy loading: {out}");
        assert!(out.contains(r#"alt="March cover""#), "alt text: {out}");
    }

    #[test]
    fn empty_lists_say_so_instead_of_rendering_a_pager() {
        fn app() -> Element {
            rsx! {
                ShowTellIndexPage {
                    chrome: PublicChrome::default(),
                    content: ShowTellIndexContent::default(),
                }
            }
        }
        let out = ssr(app);
        assert!(
            out.contains("No upcoming show-and-tells are scheduled yet."),
            "empty upcoming copy: {out}"
        );
        assert!(
            out.contains("No past show-and-tells yet."),
            "empty past copy: {out}"
        );
        assert!(
            !out.contains("nav-pagination"),
            "no pager for an empty list: {out}"
        );
    }

    #[test]
    fn the_page_leads_with_the_animated_nebula_hero() {
        let out = html();
        assert!(out.contains("nebula-hero__core"), "hero scene: {out}");
        assert_eq!(out.matches("<h1").count(), 1, "exactly one h1: {out}");
    }
}
