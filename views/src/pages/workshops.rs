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

/// One workshop on the top-level `/foundation/workshops` overview.
/// The copy is you-voiced: `audience` lets the reader self-select, and
/// `benefit` leads with what they walk out with — never a guaranteed
/// outcome (this surface is public attorney advertising).
pub struct WorkshopCard<'a> {
    /// Absolute path to the workshop's overview page, e.g.
    /// `/foundation/workshops/navigator/readme`.
    pub href: &'a str,
    pub title: &'a str,
    /// Who it's for, e.g. "For lawyers".
    pub audience: &'a str,
    /// The you-voiced takeaway shown as the card body.
    pub benefit: &'a str,
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

// The top-level overview speaks to the reader, not about the firm:
// every line is what *you* get. The cards themselves are data-driven
// from the workshop manifest, so a new workshop (or a show-and-tell)
// appears here by adding a manifest entry, not by editing this view.
const LANDING_TITLE: &str = "Workshops";
const LANDING_LEDE: &str =
    "Pick the one that meets you where you sit. Each walks you, hands-on, from where you \
     are now to something you can use the same day — and you keep what you build.";
const LANDING_MORE: &str = "More workshops — and our show-and-tells — land here as we run them.";

/// The top-level workshops overview (`/foundation/workshops`): a
/// you-voiced lede and one card per workshop, each tagged with who it's
/// for and what the reader walks out with. The per-workshop overview,
/// step flow, and Markdown twin live one level down under
/// `/foundation/workshops/navigator/{slug}`.
#[must_use]
pub fn landing(cards: &[WorkshopCard<'_>], auth: AuthState) -> Markup {
    let body = html! {
        section.workshops {
            div.container {
                h1 { (LANDING_TITLE) }
                p.lede { (LANDING_LEDE) }
                div."my-4" {
                    (assets::picture("lantana", "100vw", Priority::Lazy))
                }
                @if cards.is_empty() {
                    p.empty {
                        "Workshops are still loading. Email "
                        a href={ "mailto:" (crate::brand::foundation_email()) } {
                            (crate::brand::foundation_email())
                        }
                        " for the runbook in the meantime."
                    }
                } @else {
                    ul.workshop-materials."list-unstyled" {
                        @for c in cards {
                            li.workshop-material."mb-4" {
                                p.workshop-audience."text-uppercase"."small"."fw-semibold"."text-body-secondary"."mb-1" {
                                    (c.audience)
                                }
                                h2 {
                                    a href=(c.href) { (c.title) }
                                }
                                p { (c.benefit) }
                            }
                        }
                    }
                    p.workshops-more."fst-italic"."text-body-secondary" { (LANDING_MORE) }
                }
            }
        }
    };
    PageLayout::new(LANDING_TITLE)
        .with_description(LANDING_LEDE)
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
        landing, overview, step, MaterialOverview, StepSummary, WorkshopCard, WorkshopStep,
        LANDING_TITLE,
    };
    use crate::brand::{foundation_email, FOUNDATION_BRAND};

    fn sample_cards() -> Vec<WorkshopCard<'static>> {
        vec![
            WorkshopCard {
                href: "/foundation/workshops/navigator/readme",
                title: "Using the Navigator",
                audience: "For lawyers",
                benefit: "You walk out with a deed-of-sale notation you built yourself.",
            },
            WorkshopCard {
                href: "/foundation/workshops/navigator/deploy",
                title: "Deploy the Navigator",
                audience: "For operators",
                benefit: "You walk out running the same stack a working law firm runs.",
            },
        ]
    }

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
    fn landing_titles_the_page_workshops() {
        let html = landing(&[], crate::AuthState::Anonymous).into_string();
        assert!(html.contains(&format!(
            "<title>{} | Workshops</title>",
            FOUNDATION_BRAND.site_name
        )));
        // The sole <h1> is the overview title, not a single workshop's.
        assert_eq!(html.matches("<h1>").count(), 1, "exactly one <h1>: {html}");
        assert!(html.contains(&format!(">{LANDING_TITLE}</h1>")));
    }

    #[test]
    fn landing_shows_an_inviting_lazy_banner() {
        let html = landing(&[], crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains("lantana"),
            "workshops overview should carry the banner photo"
        );
        assert!(
            html.contains("loading=\"lazy\""),
            "banner must not block the page"
        );
    }

    #[test]
    fn landing_empty_falls_back_to_real_foundation_email() {
        let html = landing(&[], crate::AuthState::Anonymous).into_string();
        let email = foundation_email();
        assert!(html.contains(&format!("mailto:{email}")));
    }

    #[test]
    fn landing_lists_each_workshop_with_audience_and_benefit() {
        let cards = sample_cards();
        let html = landing(&cards, crate::AuthState::Anonymous).into_string();
        // Each card links to its overview at the canonical per-workshop URL,
        // titled by its short name and tagged with who it's for.
        assert!(html.contains("href=\"/foundation/workshops/navigator/readme\""));
        assert!(html.contains(">Using the Navigator</a>"));
        assert!(html.contains("For lawyers"));
        assert!(html.contains("href=\"/foundation/workshops/navigator/deploy\""));
        assert!(html.contains("For operators"));
        // You-voiced benefit copy renders, and the "more coming" footer
        // signals the surface grows.
        assert!(html.contains("You walk out"));
        assert!(html.contains("land here as we run them"));
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
