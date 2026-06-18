//! Workshop landing page (`/foundation/workshops/navigator`), the
//! per-material overview (`/foundation/workshops/navigator/:slug`),
//! and the one-step-at-a-time classroom flow
//! (`/foundation/workshops/navigator/:slug/step/:n`).
//!
//! There is one workshop on the public surface today — "Using the
//! Navigator to Rapidly Solve Legal Outcomes." Earlier work
//! ("Claude Code + Twelve Zodiac Lawyers") lives under `prompts/`
//! and is not on the public surface. See the AIDA + engineer council
//! review on 2026-05-26 for the rationale.
//!
//! The classroom flow exists for distracted learners: each `##`
//! section is its own URL, so a lawyer who steps away mid-class
//! returns to exactly the step they bookmarked rather than a wall of
//! prose. The council review on 2026-05-29 chose URL-addressable
//! steps over a JS-only carousel for that come-back property.

use maud::{html, Markup, PreEscaped};

use crate::assets::{self, Priority};
use crate::brand::FOUNDATION_BRAND;
use crate::{AuthState, PageLayout};

/// Route prefix for the workshop *index* and the material links it
/// renders. The overview and step views take their own `base` field
/// instead, so the same stepped-content chrome backs every workshop —
/// including talks like "Rust in Peace" that fold in as workshops.
const WORKSHOP_BASE: &str = "/foundation/workshops/navigator";

pub struct MaterialSummary<'a> {
    pub slug: &'a str,
    pub title: &'a str,
    pub description: &'a str,
}

/// One entry in a workshop's table of contents / progress dropdown.
pub struct StepSummary<'a> {
    /// 1-based step number.
    pub number: usize,
    pub title: &'a str,
}

/// The workshop hub: orientation lede, a numbered table of contents
/// linking to each step, a "start" button, and the copy-as-markdown
/// affordance. This is the bookmarkable page a returning learner lands
/// on.
pub struct MaterialOverview<'a> {
    /// Route prefix for this material's overview and step links, e.g.
    /// `/foundation/workshops/navigator`.
    pub base: &'a str,
    pub slug: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    /// Rendered HTML for the pre-heading orientation lede.
    pub intro_html: &'a str,
    /// Full rendered body, used only when the material has no `##`
    /// sections to step through.
    pub body_html: &'a str,
    pub steps: &'a [StepSummary<'a>],
    /// URL of this material's raw-Markdown twin (the `.md` sibling).
    /// The copy button fetches it; the page links to it; the chrome
    /// advertises it as `rel="alternate"`. One source, three uses.
    pub md_href: &'a str,
}

/// A single step in the classroom flow.
pub struct WorkshopStep<'a> {
    /// Route prefix for this material, e.g.
    /// `/foundation/workshops/navigator`.
    pub base: &'a str,
    pub slug: &'a str,
    pub workshop_title: &'a str,
    pub title: &'a str,
    /// Rendered HTML for this one section (includes its own `<h2>`).
    pub body_html: &'a str,
    /// 1-based position.
    pub number: usize,
    pub total: usize,
    /// Every step, for the jump-to-step dropdown.
    pub steps: &'a [StepSummary<'a>],
}

const PAGE_TITLE: &str = "Using the Navigator to Rapidly Solve Legal Outcomes";
const PAGE_LEDE: &str =
    "A single hands-on workshop for attorneys. By the end you will have built one \
     deed-of-sale notation for a sample real-estate-purchase matter — \
     `{{client_name}}` placeholder, notarization step, three-minute demo — using \
     Gemini's \"Add AIDA\" connector. No command-line install, no software to \
     manage; the connector lives inside the Gemini workspace your firm already \
     uses.";

#[must_use]
pub fn index(materials: &[MaterialSummary<'_>], auth: AuthState) -> Markup {
    let body = html! {
        section.workshops {
            div.container {
                h1 { (PAGE_TITLE) }
                p.lede { (PAGE_LEDE) }
                div."my-4" {
                    (assets::picture("lantana", "100vw", Priority::Lazy))
                }
                @if materials.is_empty() {
                    p.empty {
                        "Workshop materials are still loading. Email "
                        a href={ "mailto:" (crate::brand::foundation_email()) } {
                            (crate::brand::foundation_email())
                        }
                        " for the runbook in the meantime."
                    }
                } @else {
                    ul.workshop-materials {
                        @for m in materials {
                            li.workshop-material {
                                h2 {
                                    a href={ (WORKSHOP_BASE) "/" (m.slug) } {
                                        (m.title)
                                    }
                                }
                                p { (m.description) }
                            }
                        }
                    }
                }
            }
        }
    };
    PageLayout::new("Workshops")
        .with_description(PAGE_LEDE)
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

#[must_use]
pub fn overview(m: &MaterialOverview<'_>, auth: AuthState) -> Markup {
    let body = html! {
        article.workshop-material-page {
            div.container {
                header.material-header {
                    h1 { (m.title) }
                    p.description.lead { (m.description) }
                }
                @if !m.intro_html.is_empty() {
                    div.material-intro { (PreEscaped(m.intro_html)) }
                }
                div."d-flex"."flex-wrap"."gap-2"."my-4" {
                    @if let Some(first) = m.steps.first() {
                        a."btn"."btn-primary" href={ (m.base) "/" (m.slug) "/step/" (first.number) } {
                            "Start →"
                        }
                    }
                    (copy_markdown_button(m.md_href))
                    a."btn"."btn-outline-secondary" href=(m.md_href) {
                        i."bi"."bi-filetype-md" aria-hidden="true" {}
                        " View as Markdown"
                    }
                }
                (crate::components::code::syntax_highlight_assets())
                @if m.steps.is_empty() {
                    div.material-body { (PreEscaped(m.body_html)) }
                } @else {
                    nav aria-label="Workshop steps" {
                        ol."list-group"."list-group-numbered" {
                            @for s in m.steps {
                                li."list-group-item" {
                                    a."text-decoration-none"
                                        href={ (m.base) "/" (m.slug) "/step/" (s.number) } {
                                        (s.title)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    PageLayout::new(m.title)
        .with_description(m.description)
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .with_alternate_markdown(m.md_href)
        .render(&body)
}

#[must_use]
pub fn step(s: &WorkshopStep<'_>, auth: AuthState) -> Markup {
    let pct = (s.number * 100).checked_div(s.total).unwrap_or(0);
    let body = html! {
        article.workshop-step {
            div.container {
                // Persistent rail: back to the hub, progress, and a
                // jump-to-step dropdown so orientation is never stranded
                // behind a Next button.
                nav.workshop-rail."mb-4" aria-label="Workshop progress" {
                    div."d-flex"."justify-content-between"."align-items-center"."gap-2"."mb-2" {
                        a."small"."text-decoration-none" href={ (s.base) "/" (s.slug) } {
                            "← " (s.workshop_title)
                        }
                        div."d-flex"."align-items-center"."gap-2" {
                            span."small"."text-body-secondary" {
                                "Step " (s.number) " of " (s.total)
                            }
                            (step_dropdown(s))
                        }
                    }
                    div."progress"
                        role="progressbar"
                        aria-label="Workshop progress"
                        aria-valuenow=(s.number)
                        aria-valuemin="0"
                        aria-valuemax=(s.total)
                    {
                        div."progress-bar" style={ "width:" (pct) "%" } {}
                    }
                }
                section.material-body { (PreEscaped(s.body_html)) }
                (crate::components::code::syntax_highlight_assets())
                nav."d-flex"."justify-content-between"."align-items-center"."gap-2"."mt-4"
                    aria-label="Step navigation"
                {
                    @if s.number > 1 {
                        a."btn"."btn-outline-secondary"
                            href={ (s.base) "/" (s.slug) "/step/" (s.number - 1) } {
                            "← Previous"
                        }
                    } @else {
                        a."btn"."btn-outline-secondary" href={ (s.base) "/" (s.slug) } {
                            "← Overview"
                        }
                    }
                    @if s.number < s.total {
                        a."btn"."btn-primary"
                            href={ (s.base) "/" (s.slug) "/step/" (s.number + 1) } {
                            "Next →"
                        }
                    } @else {
                        a."btn"."btn-success" href={ (s.base) "/" (s.slug) } {
                            "Finish ✓"
                        }
                    }
                }
            }
        }
    };
    PageLayout::new(s.title)
        .with_description(s.workshop_title)
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

/// The jump-to-step dropdown shown in the step rail.
fn step_dropdown(s: &WorkshopStep<'_>) -> Markup {
    html! {
        div.dropdown {
            button."btn"."btn-sm"."btn-outline-secondary"."dropdown-toggle"
                type="button"
                data-bs-toggle="dropdown"
                aria-expanded="false"
            {
                "Steps"
            }
            ul."dropdown-menu"."dropdown-menu-end" {
                @for entry in s.steps {
                    li {
                        a."dropdown-item"
                            .active[entry.number == s.number]
                            href={ (s.base) "/" (s.slug) "/step/" (entry.number) } {
                            (entry.number) ". " (entry.title)
                        }
                    }
                }
            }
        }
    }
}

/// Claude-docs-style "Copy as Markdown" button. Fetches the page's
/// `.md` twin and writes the body to the clipboard — there is no
/// on-page raw-markdown node to read from, so the corpus lives at one
/// canonical URL and the button, the visible link, and the
/// `rel="alternate"` head tag all point at it. Alpine is loaded
/// site-wide and the foundation pages carry no CSP, so the inline
/// handler runs.
fn copy_markdown_button(md_href: &str) -> Markup {
    // No arrow functions in the handler so the rendered attribute needs
    // no `>` entity round-trip to stay valid JS. `copied` only flips
    // once the clipboard write resolves, so a failed fetch leaves the
    // label untouched.
    let handler = format!(
        "fetch('{md_href}').then(function (r) {{ return r.text() }})\
         .then(function (t) {{ return navigator.clipboard.writeText(t) }})\
         .then(function () {{ copied = true; \
         setTimeout(function () {{ copied = false }}, 2000) }})"
    );
    html! {
        div."d-inline-block" x-data="{ copied: false }" {
            button."btn"."btn-outline-secondary"
                type="button"
                "x-on:click"=(handler)
            {
                i."bi"."bi-clipboard" aria-hidden="true" {}
                " "
                span x-text="copied ? 'Copied!' : 'Copy as Markdown'" {
                    "Copy as Markdown"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        index, overview, step, MaterialOverview, MaterialSummary, StepSummary, WorkshopStep,
        PAGE_TITLE,
    };
    use crate::brand::{foundation_email, FOUNDATION_BRAND};

    fn sample_steps() -> Vec<StepSummary<'static>> {
        vec![
            StepSummary {
                number: 1,
                title: "Install",
            },
            StepSummary {
                number: 2,
                title: "Build the template",
            },
            StepSummary {
                number: 3,
                title: "Notarize",
            },
        ]
    }

    #[test]
    fn index_titles_the_page_after_the_canonical_workshop() {
        let html = index(&[], crate::AuthState::Anonymous).into_string();
        assert!(html.contains(&format!(
            "<title>{} | Workshops</title>",
            FOUNDATION_BRAND.site_name
        )));
        assert!(
            html.contains(PAGE_TITLE),
            "expected page h1 to carry the workshop title"
        );
    }

    #[test]
    fn index_shows_an_inviting_lazy_banner() {
        let html = index(&[], crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains("lantana"),
            "workshop index should carry the banner photo"
        );
        assert!(
            html.contains("loading=\"lazy\""),
            "banner must not block the page"
        );
    }

    #[test]
    fn index_empty_falls_back_to_real_foundation_email() {
        let html = index(&[], crate::AuthState::Anonymous).into_string();
        let email = foundation_email();
        assert!(html.contains(&format!("mailto:{email}")));
    }

    #[test]
    fn index_lists_each_material_at_canonical_url() {
        let mats = [MaterialSummary {
            slug: "readme",
            title: "Runbook",
            description: "How to participate.",
        }];
        let html = index(&mats, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("href=\"/foundation/workshops/navigator/readme\""));
        assert!(html.contains(">Runbook</a>"));
    }

    #[test]
    fn overview_carries_exactly_one_h1_and_links_each_step() {
        let steps = sample_steps();
        let m = MaterialOverview {
            base: "/foundation/workshops/navigator",
            slug: "readme",
            title: "Runbook",
            description: "How.",
            intro_html: "<p>Orientation.</p>",
            body_html: "",
            steps: &steps,
            md_href: "/foundation/workshops/navigator/readme.md",
        };
        let html = overview(&m, crate::AuthState::Anonymous).into_string();
        // The chrome's title is the sole <h1> — no duplicate.
        assert_eq!(html.matches("<h1>").count(), 1, "exactly one <h1>: {html}");
        assert!(html.contains(&format!(
            "<title>{} | Runbook</title>",
            FOUNDATION_BRAND.site_name
        )));
        // Start button + every step links to its step route.
        assert!(html.contains("href=\"/foundation/workshops/navigator/readme/step/1\""));
        assert!(html.contains("href=\"/foundation/workshops/navigator/readme/step/2\""));
        assert!(html.contains("href=\"/foundation/workshops/navigator/readme/step/3\""));
        assert!(html.contains("Start →"));
    }

    #[test]
    fn overview_copy_button_and_link_point_at_the_md_twin() {
        let steps = sample_steps();
        let md = "/foundation/workshops/navigator/readme.md";
        let m = MaterialOverview {
            base: "/foundation/workshops/navigator",
            slug: "readme",
            title: "Runbook",
            description: "How.",
            intro_html: "",
            body_html: "",
            steps: &steps,
            md_href: md,
        };
        let html = overview(&m, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("Copy as Markdown"));
        // The copy button fetches the canonical `.md` twin; there is no
        // on-page raw-markdown node to read from.
        assert!(html.contains(&format!("fetch('{md}')")));
        // A visible link and a machine-readable alternate both point at
        // the same `.md` URL so humans and crawlers find the corpus.
        assert!(html.contains(&format!("href=\"{md}\"")));
        assert!(html.contains(&format!(
            "<link rel=\"alternate\" type=\"text/markdown\" href=\"{md}\">"
        )));
    }

    #[test]
    fn step_and_overview_load_syntax_highlighting() {
        let steps = sample_steps();
        let s = WorkshopStep {
            base: "/foundation/workshops/navigator",
            slug: "readme",
            workshop_title: "Runbook",
            title: "Build the template",
            body_html: "<pre><code class=\"language-rust\">fn main() {}</code></pre>",
            number: 1,
            total: 3,
            steps: &steps,
        };
        let step_html = step(&s, crate::AuthState::Anonymous).into_string();
        let m = MaterialOverview {
            base: "/foundation/workshops/navigator",
            slug: "readme",
            title: "Runbook",
            description: "How.",
            intro_html: "",
            body_html: "",
            steps: &steps,
            md_href: "/foundation/workshops/navigator/readme.md",
        };
        let overview_html = overview(&m, crate::AuthState::Anonymous).into_string();
        // Both chrome variants vendor the highlighter: stylesheet, core
        // bundle (with Gherkin appended), and the external CSP-safe init
        // file that picks up pulldown-cmark's `language-…` fence classes.
        for html in [&step_html, &overview_html] {
            assert!(html.contains("highlight-github-dark.min.css"));
            assert!(html.contains("js/highlight.min.js"));
            assert!(html.contains("js/highlight-init.js"));
        }
    }

    #[test]
    fn overview_with_no_steps_falls_back_to_full_body() {
        let m = MaterialOverview {
            base: "/foundation/workshops/navigator",
            slug: "readme",
            title: "Runbook",
            description: "How.",
            intro_html: "",
            body_html: "<h2>Only section</h2><p>x</p>",
            steps: &[],
            md_href: "/foundation/workshops/navigator/readme.md",
        };
        let html = overview(&m, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("<h2>Only section</h2>"));
        assert!(!html.contains("Start →"));
    }

    #[test]
    fn step_renders_progress_prev_next_and_single_body() {
        let steps = sample_steps();
        let s = WorkshopStep {
            base: "/foundation/workshops/navigator",
            slug: "readme",
            workshop_title: "Runbook",
            title: "Build the template",
            body_html: "<h2>Build the template</h2><p>do it</p>",
            number: 2,
            total: 3,
            steps: &steps,
        };
        let html = step(&s, crate::AuthState::Anonymous).into_string();
        // Progress: step 2 of 3.
        assert!(html.contains("Step 2 of 3"));
        assert!(html.contains("aria-valuenow=\"2\""));
        assert!(html.contains("aria-valuemax=\"3\""));
        // 2 of 3 → 66%.
        assert!(
            html.contains("width:66%"),
            "expected width:66%, got: {html}"
        );
        // Prev → step 1, Next → step 3.
        assert!(html.contains("href=\"/foundation/workshops/navigator/readme/step/1\""));
        assert!(html.contains("href=\"/foundation/workshops/navigator/readme/step/3\""));
        assert!(html.contains("← Previous"));
        assert!(html.contains("Next →"));
        // The step body renders; the chrome title is the only <h1>.
        assert!(html.contains("<h2>Build the template</h2>"));
        assert_eq!(html.matches("<h1>").count(), 0, "step body has no <h1>");
    }

    #[test]
    fn step_links_honor_the_provided_base() {
        // The "Rust in Peace" talk is a workshop now (`rust-in-peace`
        // slug); every generated link threads the `base` + `slug` it was
        // given, so a talk and a runbook share one chrome.
        let steps = sample_steps();
        let s = WorkshopStep {
            base: "/foundation/workshops/navigator",
            slug: "rust-in-peace",
            workshop_title: "Rust in Peace",
            title: "One language, every library",
            body_html: "<h2>One language, every library</h2>",
            number: 2,
            total: 3,
            steps: &steps,
        };
        let html = step(&s, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("href=\"/foundation/workshops/navigator/rust-in-peace/step/1\""));
        assert!(html.contains("href=\"/foundation/workshops/navigator/rust-in-peace/step/3\""));
        assert!(html.contains("href=\"/foundation/workshops/navigator/rust-in-peace\""));
    }

    #[test]
    fn first_step_offers_overview_not_previous() {
        let steps = sample_steps();
        let s = WorkshopStep {
            base: "/foundation/workshops/navigator",
            slug: "readme",
            workshop_title: "Runbook",
            title: "Install",
            body_html: "<h2>Install</h2>",
            number: 1,
            total: 3,
            steps: &steps,
        };
        let html = step(&s, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("← Overview"));
        assert!(!html.contains("← Previous"));
    }

    #[test]
    fn last_step_offers_finish_not_next() {
        let steps = sample_steps();
        let s = WorkshopStep {
            base: "/foundation/workshops/navigator",
            slug: "readme",
            workshop_title: "Runbook",
            title: "Notarize",
            body_html: "<h2>Notarize</h2>",
            number: 3,
            total: 3,
            steps: &steps,
        };
        let html = step(&s, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("Finish ✓"));
        assert!(!html.contains("Next →"));
        assert!(html.contains("width:100%"));
    }
}
