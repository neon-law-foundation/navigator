//! `/{category}/{slug}/step/{n}` — the classroom face of a
//! Nebula deck, migrated to Dioxus SSR (#956 Phase 4).
//!
//! One `##` section per URL, so a lawyer who steps away mid-class returns to
//! exactly the step they bookmarked rather than a wall of prose. The slide face
//! sits on top and the presenter notes beneath it, like a Keynote slide with
//! its speaker notes.
//!
//! Two first-party scripts do all the work this page's markup only hints at,
//! and a Dioxus page loads only what it names:
//!
//! * `nebula-display.js` reads `data-nebula-step` and activates the
//!   `data-nebula-nav` anchors on ArrowLeft/ArrowRight. Without it the arrow
//!   keys stop moving the deck — and nothing but a real browser notices.
//! * `workshop-progress.js` reads `data-workshop-progress="step"` and marks
//!   this slide seen in `localStorage` (no server call, no telemetry). The
//!   light table reads the same keys to count progress and unlock the
//!   certificate, so dropping it here silently breaks a page over there.
//!
//! The jump-to-section menu is a native `<details>` disclosure. Its
//! predecessor was a Bootstrap dropdown (`data-bs-toggle="dropdown"`), which
//! cannot cross onto a Bootstrap-free page; `<details>` needs no JavaScript,
//! works before hydration, and is keyboard-accessible by default.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{PublicShell, SiteHeader, SiteNavLink, NEBULA_STYLESHEET_HREF};
use crate::nebula_slides::WORKSHOP_PROGRESS_SCRIPT_HREF;
use crate::public_chrome::{PublicChrome, PublicFooter};

/// The first-party script that turns ArrowLeft/ArrowRight into deck moves.
pub const NEBULA_DISPLAY_SCRIPT_HREF: &str = "/public/js/nebula-display.js";

/// One entry in the jump-to-section menu.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct StepMenuEntry {
    /// 1-based position across the whole material.
    pub number: usize,
    pub title: String,
    pub href: String,
    /// Whether this entry is the step being rendered.
    pub current: bool,
}

/// One chapter's worth of menu entries.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct StepMenuChapter {
    pub number: usize,
    pub title: String,
    pub entries: Vec<StepMenuEntry>,
}

/// Everything one classroom step renders.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct StepContent {
    pub workshop_title: String,
    /// The material slug, the key `workshop-progress.js` stores progress under.
    pub slug: String,
    /// Back to the material hub.
    pub material_href: String,
    /// This section's title.
    pub title: String,
    /// Rendered HTML for the slide face (carries its own heading).
    pub body_html: String,
    /// Rendered HTML for the presenter notes. Empty renders no notes block.
    pub notes_html: String,
    /// 1-based position across the whole material.
    pub number: usize,
    pub total: usize,
    pub chapter_number: usize,
    pub chapter_title: String,
    pub chapter_total: usize,
    /// Completion percentage for the progress bar, resolved server-side.
    pub percent: usize,
    /// The previous slide, when there is one. `None` at the first slide, where
    /// the rail offers the overview instead and `ArrowLeft` falls through.
    pub prev_href: Option<String>,
    /// The next slide, when there is one. `None` at the last slide, where the
    /// deck offers Finish instead and `ArrowRight` falls through.
    pub next_href: Option<String>,
    /// The light-table grid of every slide.
    pub slides_href: String,
    /// This slide's full-screen display face, for a second monitor.
    pub display_href: String,
    pub chapters: Vec<StepMenuChapter>,
}

/// The [`StepContent`] the portal pre-layer injects.
#[derive(Clone, Default)]
pub struct InjectedStep(pub StepContent);

/// Everything the page renders.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct NebulaStepView {
    pub chrome: PublicChrome,
    pub content: StepContent,
}

/// Resolve the Foundation chrome and this step's content.
#[server]
pub async fn nebula_step_view() -> Result<NebulaStepView, ServerFnError> {
    let content =
        dioxus_fullstack_core::FullstackContext::extract::<axum::Extension<InjectedStep>, _>()
            .await
            .map(|axum::Extension(c)| c.0)
            .unwrap_or_default();
    Ok(NebulaStepView {
        chrome: crate::public_chrome::foundation_public_chrome_from_context().await,
        content,
    })
}

/// The page's route entry.
#[component]
pub fn NebulaStepEntry() -> Element {
    let resource = use_server_future(nebula_step_view)?;
    let view = match &*resource.read() {
        Some(Ok(view)) => view.clone(),
        _ => return rsx! {},
    };
    rsx! {
        NebulaStepPage { chrome: view.chrome, content: view.content }
    }
}

/// The pure classroom step.
#[component]
pub fn NebulaStepPage(chrome: PublicChrome, content: StepContent) -> Element {
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
        document::Meta { name: "description", content: "{content.workshop_title}" }
        document::Stylesheet { href: NEBULA_STYLESHEET_HREF }
        // Without this the arrow keys stop moving the deck.
        document::Script { src: NEBULA_DISPLAY_SCRIPT_HREF, defer: true }
        // Without this the slide is never marked seen, so the light table's
        // checks and certificate gate never arrive.
        document::Script { src: WORKSHOP_PROGRESS_SCRIPT_HREF, defer: true }
        PublicShell { header, footer,
            article {
                class: "workshop-step",
                "data-workshop-progress": "step",
                "data-nebula-step": true,
                "data-workshop-slug": "{content.slug}",
                "data-slide": "{content.number}",
                StepRail { content: content.clone() }
                // The slide face: a 16:9 canvas so the deck reads like a
                // Keynote wide presentation.
                section { class: "workshop-slide",
                    div { class: "material-body", dangerous_inner_html: "{content.body_html}" }
                }
                if !content.notes_html.is_empty() {
                    aside { class: "presenter-notes", "aria-label": "Presenter notes",
                        p { class: "presenter-notes__label", "Presenter notes" }
                        div {
                            class: "presenter-notes__body",
                            dangerous_inner_html: "{content.notes_html}",
                        }
                    }
                }
                div { class: "workshop-step__links",
                    a { href: "{content.slides_href}", "View all slides" }
                    // Opens the slide-only display face in a second window, so
                    // a presenter can drag it to an external monitor and
                    // full-screen it while the notes stay here.
                    a {
                        href: "{content.display_href}",
                        target: "_blank",
                        rel: "noopener",
                        "Open display"
                    }
                }
                StepNav { content: content.clone() }
            }
        }
    }
}

/// The persistent rail: back to the hub, where you are, the jump-to-section
/// menu, and a progress bar — so orientation is never stranded behind a Next
/// button.
#[component]
fn StepRail(content: StepContent) -> Element {
    rsx! {
        nav { class: "workshop-rail", "aria-label": "Workshop progress",
            div { class: "workshop-rail__top",
                a { class: "workshop-rail__back", href: "{content.material_href}",
                    "← {content.workshop_title}"
                }
                div { class: "workshop-rail__meta",
                    div {
                        p { class: "workshop-rail__chapter", "{content.chapter_title}" }
                        p { class: "workshop-rail__position",
                            "Chapter {content.chapter_number} of {content.chapter_total} · Section {content.number} of {content.total}"
                        }
                    }
                    SectionMenu { chapters: content.chapters.clone() }
                }
            }
            div {
                class: "workshop-progress",
                role: "progressbar",
                "aria-label": "Workshop progress",
                "aria-valuenow": "{content.number}",
                "aria-valuemin": "0",
                "aria-valuemax": "{content.total}",
                div { class: "workshop-progress__bar", style: "width:{content.percent}%" }
            }
        }
    }
}

/// The chapter-grouped jump-to-section menu.
///
/// A native `<details>` disclosure, not a scripted dropdown: it opens before
/// hydration, needs no JavaScript at all, and Enter/Space on the `<summary>`
/// is browser behavior rather than something this page has to implement.
#[component]
fn SectionMenu(chapters: Vec<StepMenuChapter>) -> Element {
    rsx! {
        details { class: "workshop-sections",
            summary { class: "workshop-sections__toggle", "Sections" }
            div { class: "workshop-sections__menu",
                for chapter in chapters.iter() {
                    p {
                        class: "workshop-sections__header",
                        "data-workshop-chapter": "{chapter.title}",
                        "{chapter.number}. {chapter.title}"
                    }
                    ul { class: "workshop-sections__list",
                        for entry in chapter.entries.iter() {
                            li {
                                a {
                                    class: if entry.current { "workshop-sections__item workshop-sections__item--current" } else { "workshop-sections__item" },
                                    href: "{entry.href}",
                                    "{entry.number}. {entry.title}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Previous / next across the deck.
///
/// Only a true slide move carries `data-nebula-nav`, so an arrow key at either
/// end of the deck finds no control and falls through untouched — the Overview
/// and Finish exits stay click-only.
#[component]
fn StepNav(content: StepContent) -> Element {
    rsx! {
        nav { class: "workshop-step__nav", "aria-label": "Step navigation",
            if let Some(prev) = content.prev_href.clone() {
                a {
                    class: "nav-btn nav-btn--secondary",
                    "data-nebula-nav": "prev",
                    href: "{prev}",
                    "← Previous"
                }
            } else {
                a { class: "nav-btn nav-btn--secondary", href: "{content.material_href}",
                    "← Overview"
                }
            }
            if let Some(next) = content.next_href.clone() {
                a {
                    class: "nav-btn nav-btn--primary",
                    "data-nebula-nav": "next",
                    href: "{next}",
                    "Next →"
                }
            } else {
                a { class: "nav-btn nav-btn--primary", href: "{content.material_href}",
                    "Finish"
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

    fn content() -> StepContent {
        StepContent {
            workshop_title: "Runbook".into(),
            slug: "use-the-navigator".into(),
            material_href: "/workshops/use-the-navigator".into(),
            title: "Build the template".into(),
            body_html: "<h3>Build the template</h3><p>do it</p>".into(),
            notes_html: "<p>Walk the room through why.</p>".into(),
            number: 2,
            total: 3,
            chapter_number: 1,
            chapter_title: "Intro".into(),
            chapter_total: 2,
            percent: 66,
            prev_href: Some("/workshops/use-the-navigator/step/1".into()),
            next_href: Some("/workshops/use-the-navigator/step/3".into()),
            slides_href: "/workshops/use-the-navigator/slides".into(),
            display_href: "/workshops/use-the-navigator/display/2".into(),
            chapters: vec![
                StepMenuChapter {
                    number: 1,
                    title: "Intro".into(),
                    entries: vec![
                        StepMenuEntry {
                            number: 1,
                            title: "Install".into(),
                            href: "/workshops/use-the-navigator/step/1".into(),
                            current: false,
                        },
                        StepMenuEntry {
                            number: 2,
                            title: "Build the template".into(),
                            href: "/workshops/use-the-navigator/step/2".into(),
                            current: true,
                        },
                    ],
                },
                StepMenuChapter {
                    number: 2,
                    title: "Wrap Up".into(),
                    entries: vec![StepMenuEntry {
                        number: 3,
                        title: "Notarize".into(),
                        href: "/workshops/use-the-navigator/step/3".into(),
                        current: false,
                    }],
                },
            ],
        }
    }

    /// The middle slide, where both arrow targets exist.
    fn html() -> String {
        fn app() -> Element {
            rsx! {
                NebulaStepPage { chrome: PublicChrome::default(), content: content() }
            }
        }
        ssr(app)
    }

    #[test]
    fn step_names_the_chapter_the_section_and_the_progress() {
        let html = html();
        assert!(
            html.contains(">Chapter 1 of 2 · Section 2 of 3<"),
            "rail position: {html}"
        );
        assert!(html.contains("aria-valuenow=\"2\""), "progress now: {html}");
        assert!(html.contains("aria-valuemax=\"3\""), "progress max: {html}");
        assert!(html.contains("width:66%"), "progress width: {html}");
    }

    #[test]
    fn step_carries_the_hooks_both_first_party_scripts_read() {
        let html = html();
        // `nebula-display.js` opts in on this attribute and navigates by
        // clicking the marked prev/next anchors.
        assert!(html.contains("data-nebula-step"), "arrow-nav root: {html}");
        assert!(
            html.contains("data-nebula-nav=\"prev\""),
            "prev target: {html}"
        );
        assert!(
            html.contains("data-nebula-nav=\"next\""),
            "next target: {html}"
        );
        // `workshop-progress.js` marks the slide seen from these.
        assert!(
            html.contains("data-workshop-progress=\"step\""),
            "progress root: {html}"
        );
        assert!(
            html.contains("data-workshop-slug=\"use-the-navigator\""),
            "progress key: {html}"
        );
        assert!(html.contains("data-slide=\"2\""), "slide number: {html}");
        assert!(
            !html.contains("data-slide-seen-badge"),
            "no seen badge: {html}"
        );
    }

    #[test]
    fn arrow_targets_are_absent_at_the_deck_ends() {
        fn first_slide() -> Element {
            rsx! {
                NebulaStepPage {
                    chrome: PublicChrome::default(),
                    content: StepContent { number: 1, prev_href: None, ..content() },
                }
            }
        }
        fn last_slide() -> Element {
            rsx! {
                NebulaStepPage {
                    chrome: PublicChrome::default(),
                    content: StepContent { number: 3, next_href: None, ..content() },
                }
            }
        }
        // First slide: the Overview exit is a plain link, so ArrowLeft finds no
        // control and falls through untouched.
        let first = ssr(first_slide);
        assert!(
            !first.contains("data-nebula-nav=\"prev\""),
            "no prev target at the first slide: {first}"
        );
        assert!(
            first.contains("data-nebula-nav=\"next\""),
            "still advances: {first}"
        );
        assert!(first.contains("← Overview"), "overview exit: {first}");
        assert!(!first.contains("← Previous"));

        let last = ssr(last_slide);
        assert!(
            !last.contains("data-nebula-nav=\"next\""),
            "no next target at the last slide: {last}"
        );
        assert!(
            last.contains("data-nebula-nav=\"prev\""),
            "still goes back: {last}"
        );
        assert!(last.contains("Finish"), "finish exit: {last}");
        assert!(!last.contains('✓'), "finish has no checkmark: {last}");
        assert!(!last.contains("Next →"));
    }

    #[test]
    fn the_section_menu_is_a_native_disclosure_grouped_by_chapter() {
        let html = html();
        // A `<details>`/`<summary>` disclosure, not a scripted dropdown: no
        // Bootstrap toggle attribute survives the migration.
        assert!(html.contains("<details"), "native disclosure: {html}");
        assert!(
            html.contains("workshop-sections__toggle"),
            "summary hook the browser test focuses: {html}"
        );
        assert!(
            !html.contains("data-bs-toggle"),
            "no Bootstrap JavaScript: {html}"
        );
        assert!(
            html.contains("data-workshop-chapter=\"Intro\""),
            "chapter header: {html}"
        );
        assert!(
            html.contains("data-workshop-chapter=\"Wrap Up\""),
            "second chapter: {html}"
        );
        // The step being read is marked, so a reader can see where they are in
        // the list.
        assert!(
            html.contains("workshop-sections__item--current"),
            "current entry: {html}"
        );
    }

    #[test]
    fn step_renders_notes_and_the_two_deck_exits() {
        let html = html();
        assert!(html.contains(">Presenter notes<"), "notes label: {html}");
        assert!(html.contains("Walk the room through why."), "notes: {html}");
        assert!(
            html.contains("href=\"/workshops/use-the-navigator/slides\""),
            "light table: {html}"
        );
        assert!(
            html.contains("href=\"/workshops/use-the-navigator/display/2\""),
            "display face for this slide: {html}"
        );
        // The slide body renders; the chrome title is the only <h1>.
        assert!(html.contains("<h3>Build the template</h3>"));
    }

    #[test]
    fn a_material_without_presenter_notes_renders_no_notes_block() {
        fn app() -> Element {
            rsx! {
                NebulaStepPage {
                    chrome: PublicChrome::default(),
                    content: StepContent { notes_html: String::new(), ..content() },
                }
            }
        }
        let html = ssr(app);
        assert!(
            !html.contains("Presenter notes"),
            "no empty notes block: {html}"
        );
    }
}
