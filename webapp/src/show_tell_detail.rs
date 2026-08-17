//! `/show-and-tell/{slug}` — one Nebula gathering, migrated
//! to Dioxus SSR (#956 Phase 4).
//!
//! Luma owns
//! everything about actually attending, so the page shows the event picture and
//! its description, then hands off with a single outbound link.
//!
//! Fixed per URL rather than per request, but the router resolves it from the
//! path segment, so the content arrives as a per-request injection like the
//! index's.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{
    ExternalLink, PublicShell, SiteHeader, SiteNavLink, NEBULA_STYLESHEET_HREF,
};
use crate::public_chrome::{PublicChrome, PublicFooter};
use crate::show_tell_index::SHOW_TELL_INDEX_PATH;

/// One gathering's resolved content.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ShowTellDetailContent {
    pub title: String,
    pub description: String,
    /// The formatted local date/time range, pre-rendered portal-side.
    pub time: String,
    /// The Luma event URL. Luma owns attendance; present on every event.
    pub luma_url: Option<String>,
    pub image_url: Option<String>,
    pub image_alt: String,
    /// The event write-up (already-sanitized HTML).
    pub body_html: String,
}

/// The [`ShowTellDetailContent`] the portal pre-layer injects.
#[derive(Clone, Default)]
pub struct InjectedShowTellDetail(pub ShowTellDetailContent);

/// Everything the page renders.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ShowTellDetailView {
    pub chrome: PublicChrome,
    pub content: ShowTellDetailContent,
}

/// Resolve the Foundation chrome and this gathering's content.
#[server]
pub async fn show_tell_detail_view() -> Result<ShowTellDetailView, ServerFnError> {
    let content = dioxus_fullstack_core::FullstackContext::extract::<
        axum::Extension<InjectedShowTellDetail>,
        _,
    >()
    .await
    .map(|axum::Extension(c)| c.0)
    .unwrap_or_default();
    Ok(ShowTellDetailView {
        chrome: crate::public_chrome::foundation_public_chrome_from_context().await,
        content,
    })
}

/// The page's route entry.
#[component]
pub fn ShowTellDetailEntry() -> Element {
    let resource = use_server_future(show_tell_detail_view)?;
    let view = match &*resource.read() {
        Some(Ok(view)) => view.clone(),
        _ => return rsx! {},
    };
    rsx! {
        ShowTellDetailPage { chrome: view.chrome, content: view.content }
    }
}

/// The pure detail page.
#[component]
pub fn ShowTellDetailPage(chrome: PublicChrome, content: ShowTellDetailContent) -> Element {
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
        document::Title { "{chrome.brand_name} | {content.title}" }
        document::Meta { name: "description", content: "{content.description}" }
        // The event picture is sized by `.show-tell-detail__image`; the
        // page got `img-fluid rounded` from the Bootstrap the layout linked.
        document::Stylesheet { href: NEBULA_STYLESHEET_HREF }
        PublicShell { header, footer,
            article { class: "show-tell-detail",
                p {
                    a { href: SHOW_TELL_INDEX_PATH, "Back to show-and-tell events" }
                }
                h1 { "{content.title}" }
                p { class: "show-tell-detail__date", "{content.time}" }
                if let Some(image_url) = content.image_url.clone() {
                    p {
                        img {
                            class: "show-tell-detail__image",
                            src: "{image_url}",
                            alt: "{content.image_alt}",
                            loading: "lazy",
                            decoding: "async",
                        }
                    }
                }
                div { dangerous_inner_html: "{content.body_html}" }
                if let Some(luma_url) = content.luma_url.clone() {
                    p {
                        ExternalLink {
                            href: luma_url,
                            class: "nav-btn nav-btn--primary".to_string(),
                            "Check it out on Luma"
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

    fn populated() -> ShowTellDetailContent {
        ShowTellDetailContent {
            title: "March show-and-tell".to_string(),
            description: "What we showed in March.".to_string(),
            time: "March 3, 2026, 5:00–6:30 PM PST".to_string(),
            luma_url: Some("https://lu.ma/example".to_string()),
            image_url: Some("/public/img/e/march.avif".to_string()),
            image_alt: "March cover".to_string(),
            body_html: "<p>We walked through the intake walk.</p>".to_string(),
        }
    }

    fn html() -> String {
        fn app() -> Element {
            rsx! {
                ShowTellDetailPage { chrome: PublicChrome::default(), content: populated() }
            }
        }
        ssr(app)
    }

    #[test]
    fn renders_the_title_time_and_write_up() {
        let out = html();
        assert!(out.contains("March show-and-tell"), "title: {out}");
        assert!(out.contains("5:00–6:30 PM PST"), "formatted time: {out}");
        assert!(
            out.contains("We walked through the intake walk."),
            "body html: {out}"
        );
        assert_eq!(out.matches("<h1").count(), 1, "exactly one h1: {out}");
    }

    #[test]
    fn the_reading_measure_is_class_driven_not_an_inline_cap() {
        // The page borrowed the blog's `.blog-post` class *and* carried an
        // inline `max-width: 65ch`. The class owns the measure now, so the rule
        // travels with the stylesheet instead of the markup.
        let out = html();
        assert!(
            out.contains(r#"class="show-tell-detail""#),
            "the article carries its own measure class: {out}"
        );
        assert!(
            !out.contains("max-width: 65ch"),
            "measure should be class-driven, not an inline cap: {out}"
        );
    }

    #[test]
    fn links_back_to_the_archive() {
        let out = html();
        assert!(
            out.contains(r#"href="/foundation/show-and-tell""#)
                && out.contains("Back to show-and-tell events"),
            "back link: {out}"
        );
    }

    #[test]
    fn the_luma_handoff_is_external_safe() {
        // Luma owns attendance, so this is the page's one outbound link. The
        // version emitted `rel="noopener"` alone; routing it through
        // `ExternalLink` gets the full OWASP pair and the leaves-the-site glyph.
        let out = html();
        assert!(
            out.contains(r#"href="https://lu.ma/example""#),
            "luma href: {out}"
        );
        assert!(
            out.contains(r#"rel="noopener noreferrer""#),
            "both rel tokens, not noopener alone: {out}"
        );
        assert!(out.contains(r#"target="_blank""#), "opens off-site: {out}");
    }

    #[test]
    fn an_event_without_a_picture_or_a_luma_link_renders_neither() {
        fn app() -> Element {
            let content = ShowTellDetailContent {
                title: "Quiet one".to_string(),
                time: "Later".to_string(),
                body_html: "<p>Body.</p>".to_string(),
                ..ShowTellDetailContent::default()
            };
            rsx! { ShowTellDetailPage { chrome: PublicChrome::default(), content } }
        }
        let out = ssr(app);
        // Scope this to the page's own picture — the shell header always
        // renders a logo `<img>`, so a bare `<img` check would never fail.
        assert!(
            !out.contains("show-tell-detail__image"),
            "no picture: {out}"
        );
        assert!(!out.contains("lu.ma"), "no luma handoff: {out}");
        assert!(out.contains("Body."), "the write-up still renders: {out}");
    }

    #[test]
    fn the_picture_is_lazy_and_carries_alt_text() {
        let out = html();
        assert!(out.contains(r#"loading="lazy""#), "lazy: {out}");
        assert!(out.contains(r#"alt="March cover""#), "alt text: {out}");
    }
}
