//! Nebula landing page (`/foundation/nebula`), the per-material
//! overview (`/foundation/nebula/:category/:slug`),
//! and the one-step-at-a-time classroom flow
//! (`/foundation/nebula/:category/:slug/step/:n`).
//!
//! There is one workshop on the public surface today — "Using the
//! Neon Law Navigator to Rapidly Solve Legal Outcomes." Earlier work
//! ("Claude Code + Twelve Zodiac Lawyers") is not on the public
//! surface. See the AIDA + engineer council review on 2026-05-26 for
//! the rationale.
//!
//! The classroom flow exists for distracted learners: each `##`
//! section is its own URL, so a lawyer who steps away mid-class
//! returns to exactly the step they bookmarked rather than a wall of
//! prose. The council review on 2026-05-29 chose URL-addressable
//! steps over a JS-only carousel for that come-back property.

use maud::{html, Markup, PreEscaped};

use crate::assets::{self, Priority};
use crate::brand::FOUNDATION_BRAND;
use crate::{AuthState, Locale, PageLayout};

/// One learning item on the top-level `/foundation/nebula` overview.
/// The copy is you-voiced: `audience` lets the reader self-select, and
/// `benefit` leads with what they walk out with — never a guaranteed
/// outcome (this surface is public attorney advertising).
pub struct MaterialCard<'a> {
    /// Absolute path to the Nebula material page, e.g.
    /// `/foundation/nebula/workshops/use-the-navigator`.
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

/// One event on the top-level `/foundation/nebula` overview.
pub struct EventCard<'a> {
    pub href: &'a str,
    pub title: &'a str,
    pub time: &'a str,
    pub place: &'a str,
    pub description: &'a str,
}

/// One show-and-tell on the paginated event index.
pub struct EventListItem<'a> {
    pub detail_href: &'a str,
    pub calendar_href: &'a str,
    pub title: &'a str,
    pub time: &'a str,
    pub place: &'a str,
    pub description: &'a str,
    pub invite_link: &'a str,
    pub image_url: Option<&'a str>,
    pub image_alt: &'a str,
}

pub struct EventPager<'a> {
    pub previous_href: Option<&'a str>,
    pub next_href: Option<&'a str>,
    pub current_page: usize,
    pub total_pages: usize,
}

pub struct ShowTellIndex<'a> {
    pub upcoming: &'a [EventListItem<'a>],
    pub past: &'a [EventListItem<'a>],
    pub upcoming_pager: EventPager<'a>,
    pub past_pager: EventPager<'a>,
}

/// A Nebula show-and-tell detail page.
pub struct ShowTellDetail<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub time: &'a str,
    pub place: &'a str,
    pub external_event_provider: &'a str,
    pub external_event_url: &'a str,
    pub ics_url: &'a str,
    pub body_html: &'a str,
    pub video_url: Option<&'a str>,
    pub recap_url: Option<&'a str>,
}

/// The workshop hub: orientation lede, a numbered table of contents
/// linking to each step, a "start" button, and the copy-as-markdown
/// affordance. This is the bookmarkable page a returning learner lands
/// on.
pub struct MaterialOverview<'a> {
    /// Route prefix for this material's overview and step links, e.g.
    /// `/foundation/nebula/workshops`.
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

/// A single slide in the classroom flow — the slide face on top, the
/// presenter notes beneath, like a Keynote slide with its speaker notes.
pub struct WorkshopStep<'a> {
    /// Route prefix for this material, e.g.
    /// `/foundation/nebula/workshops`.
    pub base: &'a str,
    pub slug: &'a str,
    pub workshop_title: &'a str,
    pub title: &'a str,
    /// Rendered HTML for the slide face (includes its own `<h2>`).
    pub body_html: &'a str,
    /// Rendered HTML for the presenter notes shown beneath the slide.
    /// Empty only for legacy/unsplit sections.
    pub notes_html: &'a str,
    /// 1-based position.
    pub number: usize,
    pub total: usize,
    /// Every step, for the jump-to-step dropdown.
    pub steps: &'a [StepSummary<'a>],
}

/// One slide thumbnail in the light-table grid.
pub struct SlideThumb<'a> {
    /// 1-based slide number.
    pub number: usize,
    pub title: &'a str,
    /// Rendered HTML of the slide face, shown shrunk into the thumbnail.
    pub body_html: &'a str,
}

/// The light-table view of a whole workshop: every slide as a thumbnail
/// in a grid, a client-side progress count, and — once every slide has
/// been viewed — a form to receive a completion certificate by email.
pub struct LightTable<'a> {
    pub base: &'a str,
    pub slug: &'a str,
    pub workshop_title: &'a str,
    pub slides: &'a [SlideThumb<'a>],
    /// Double-submit CSRF token for the certificate request form.
    pub csrf_token: &'a str,
}

// The top-level overview speaks to the reader, not about the firm.
// Nebula is the Foundation's sharing surface: the Neon Law Navigator is what
// we build; Nebula is how we show the work and help others learn it.
const LANDING_TITLE: &str = "Nebula";
const LANDING_LEDE: &str =
    "Nebula is where the Foundation shares what it is learning: workshops, show-and-tells, \
     and presentations for lawyers and legal professionals who want to build with Neon Law Navigator.";
const LANDING_MORE: &str =
    "More workshops, show-and-tells, and presentations land here as we run them.";
const LANDING_TITLE_ES: &str = "Nebula";
const LANDING_LEDE_ES: &str =
    "Nebula es donde la Fundación comparte lo que está aprendiendo: talleres, muestras \
     prácticas y presentaciones para abogados y profesionales legales que quieren construir \
     con Neon Law Navigator.";
const LANDING_MORE_ES: &str =
    "Aquí publicaremos más talleres, muestras prácticas y presentaciones a medida que las hagamos.";

/// The top-level Nebula overview (`/foundation/nebula`).
#[must_use]
pub fn landing(
    workshop_cards: &[MaterialCard<'_>],
    presentation_cards: &[MaterialCard<'_>],
    event_cards: &[EventCard<'_>],
    auth: AuthState,
) -> Markup {
    landing_in(
        workshop_cards,
        presentation_cards,
        event_cards,
        auth,
        Locale::En,
    )
}

#[must_use]
pub fn landing_in(
    workshop_cards: &[MaterialCard<'_>],
    presentation_cards: &[MaterialCard<'_>],
    event_cards: &[EventCard<'_>],
    auth: AuthState,
    locale: Locale,
) -> Markup {
    let (title, lede, more, workshops, presentations, show_and_tell, empty) = match locale {
        Locale::En => (
            LANDING_TITLE,
            LANDING_LEDE,
            LANDING_MORE,
            "Workshops",
            "Presentations",
            "Show-and-tell",
            "Nebula materials are still loading. Email ",
        ),
        Locale::Es => (
            LANDING_TITLE_ES,
            LANDING_LEDE_ES,
            LANDING_MORE_ES,
            "Talleres",
            "Presentaciones",
            "Muestras prácticas",
            "Los materiales de Nebula todavía se están cargando. Escriba a ",
        ),
    };
    let body = html! {
        section.workshops {
            div.container {
                header.nebula-hero."position-relative"."overflow-hidden"."mb-5" {
                    div.nebula-hero-media aria-hidden="true" {
                        (assets::picture("lantana", "100vw", Priority::Eager))
                    }
                    div.nebula-hero-copy."position-relative"."py-5"."px-4"."px-lg-5" {
                        p."text-uppercase"."small"."fw-semibold"."mb-2" { "Neon Law Foundation" }
                        h1 { (title) }
                        p.lede."mb-0" { (lede) }
                    }
                }
                @if workshop_cards.is_empty() && presentation_cards.is_empty() && event_cards.is_empty() {
                    p.empty {
                        (empty)
                        a href={ "mailto:" (crate::brand::foundation_email()) } {
                            (crate::brand::foundation_email())
                        }
                        " for the runbook in the meantime."
                    }
                } @else {
                    @if !workshop_cards.is_empty() {
                        h2 { (workshops) }
                        ul.workshop-materials."list-unstyled"."mb-5" {
                            @for c in workshop_cards {
                                (material_card(c))
                            }
                        }
                    }
                    @if !event_cards.is_empty() {
                        h2 {
                            i."bi"."bi-people-fill"."me-2" aria-hidden="true" {}
                            (show_and_tell)
                        }
                        ul.workshop-materials."list-unstyled"."mb-5" {
                            @for c in event_cards {
                                li.workshop-material."mb-4" {
                                    p.workshop-audience."text-uppercase"."small"."fw-semibold"."text-body-secondary"."mb-1" {
                                        (c.time) " · " (c.place)
                                    }
                                    h3 {
                                        a href=(c.href) { (c.title) }
                                    }
                                    p { (c.description) }
                                }
                            }
                        }
                        p {
                            a href="/foundation/nebula/show-and-tell" { "View all show-and-tells" }
                        }
                    }
                    @if !presentation_cards.is_empty() {
                        h2 { (presentations) }
                        ul.workshop-materials."list-unstyled"."mb-5" {
                            @for c in presentation_cards {
                                (material_card(c))
                            }
                        }
                    }
                    p.workshops-more."fst-italic"."text-body-secondary" { (more) }
                }
            }
        }
    };
    let layout = PageLayout::new(title)
        .with_description(lede)
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .with_locale(locale)
        .with_canonical_path("/foundation/nebula");
    match assets::preload_href("lantana") {
        Some(href) => layout.with_preload_image(&href).render(&body),
        None => layout.render(&body),
    }
}

fn material_card(c: &MaterialCard<'_>) -> Markup {
    html! {
        li.workshop-material."mb-4" {
            p.workshop-audience."text-uppercase"."small"."fw-semibold"."text-body-secondary"."mb-1" {
                (c.audience)
            }
            h3 {
                a href=(c.href) { (c.title) }
            }
            p { (c.benefit) }
        }
    }
}

#[must_use]
pub fn show_tell_index(index: &ShowTellIndex<'_>, auth: AuthState) -> Markup {
    let body = html! {
        section.show-tell-index {
            header.nebula-hero."position-relative"."overflow-hidden"."mb-5" {
                div.nebula-hero-media aria-hidden="true" {
                    (assets::picture("lantana", "100vw", Priority::Eager))
                }
                div.nebula-hero-copy."position-relative"."py-5"."px-4"."px-lg-5" {
                    p."text-uppercase"."small"."fw-semibold"."mb-2" { "Neon Law Foundation" }
                    h1 { "Show-and-tell events" }
                    p.lede."mb-0" {
                        "Practical Nebula gatherings for lawyers and legal professionals building with AI, workflows, \
                         and Neon Law Navigator."
                    }
                }
            }

            div."d-flex"."align-items-end"."justify-content-between"."gap-3"."flex-wrap"."mb-3" {
                div {
                    h2."mb-1" { "Upcoming" }
                    p."text-body-secondary"."mb-0" { "Today forward, nearest first." }
                }
            }
            @if index.upcoming.is_empty() {
                p.empty { "No upcoming show-and-tells are scheduled yet." }
            } @else {
                div.show-tell-grid."mb-4" {
                    @for event in index.upcoming {
                        (event_list_card(event))
                    }
                }
                (event_pagination(&index.upcoming_pager))
            }

            div."d-flex"."align-items-end"."justify-content-between"."gap-3"."flex-wrap"."mt-5"."mb-3" {
                div {
                    h2."mb-1" { "Past" }
                    p."text-body-secondary"."mb-0" { "Earlier gatherings, newest first." }
                }
            }
            @if index.past.is_empty() {
                p.empty { "No past show-and-tells yet." }
            } @else {
                div.show-tell-grid."mb-4" {
                    @for event in index.past {
                        (event_list_card(event))
                    }
                }
                (event_pagination(&index.past_pager))
            }
        }
    };
    let layout = PageLayout::new("Nebula show-and-tell events")
        .with_description(
            "Upcoming and past Nebula show-and-tell events from the Neon Law Foundation.",
        )
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .with_canonical_path("/foundation/nebula/show-and-tell");
    match assets::preload_href("lantana") {
        Some(href) => layout.with_preload_image(&href).render(&body),
        None => layout.render(&body),
    }
}

fn event_list_card(event: &EventListItem<'_>) -> Markup {
    html! {
        article.show-tell-card {
            @if let Some(image_url) = event.image_url {
                a.show-tell-card-media href=(event.detail_href) {
                    img src=(image_url) alt=(event.image_alt) loading="lazy" decoding="async";
                }
            }
            div.show-tell-card-body {
                p.workshop-audience."text-uppercase"."small"."fw-semibold"."text-body-secondary"."mb-1" {
                    (event.time) " · " (event.place)
                }
                h3 { a href=(event.detail_href) { (event.title) } }
                p { (event.description) }
                div."d-flex"."flex-wrap"."gap-2" {
                    a.btn.btn-primary.btn-sm href=(event.invite_link) {
                        (luma_logo())
                        span { "RSVP on Luma" }
                    }
                    a.btn.btn-outline-secondary.btn-sm href=(event.calendar_href) {
                        i."bi"."bi-calendar-plus" aria-hidden="true" {}
                        span."ms-1" { "Calendar" }
                    }
                }
            }
        }
    }
}

fn event_pagination(pager: &EventPager<'_>) -> Markup {
    if pager.total_pages <= 1 {
        return html! {};
    }
    html! {
        nav."d-flex"."align-items-center"."gap-2" aria-label="Event pagination" {
            @if let Some(href) = pager.previous_href {
                a.btn.btn-outline-secondary.btn-sm href=(href) { "Previous" }
            } @else {
                span.btn.btn-outline-secondary.btn-sm.disabled aria-disabled="true" { "Previous" }
            }
            span."small"."text-body-secondary" {
                "Page " (pager.current_page) " of " (pager.total_pages)
            }
            @if let Some(href) = pager.next_href {
                a.btn.btn-outline-secondary.btn-sm href=(href) { "Next" }
            } @else {
                span.btn.btn-outline-secondary.btn-sm.disabled aria-disabled="true" { "Next" }
            }
        }
    }
}

#[must_use]
pub fn show_tell(event: &ShowTellDetail<'_>, auth: AuthState) -> Markup {
    let body = html! {
        article.blog-post style="max-width: 65ch; margin-inline: auto;" {
            p { a href="/foundation/nebula/show-and-tell" { "Back to show-and-tell events" } }
            h1 { (event.title) }
            p.blog-date { small { (event.time) " · " (event.place) } }
            p {
                a.btn.btn-primary href=(event.external_event_url) {
                    @if event.external_event_provider.eq_ignore_ascii_case("luma") {
                        (luma_logo())
                    }
                    span { "RSVP on " (provider_label(event.external_event_provider)) }
                }
                " "
                a.btn.btn-outline-secondary href=(event.ics_url) { "Add to calendar" }
            }
            (PreEscaped(event.body_html))
            @if let Some(video_url) = event.video_url {
                h2 { "Video" }
                p { a href=(video_url) { "Watch the show-and-tell" } }
            }
            @if let Some(recap_url) = event.recap_url {
                h2 { "Recap" }
                p { a href=(recap_url) { "Read the recap" } }
            }
        }
    };
    PageLayout::new(event.title)
        .with_description(event.description)
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

fn provider_label(provider: &str) -> &str {
    if provider.eq_ignore_ascii_case("luma") {
        "Luma"
    } else {
        "event page"
    }
}

fn luma_logo() -> Markup {
    html! {
        img.luma-logo src="/public/logos/luma.svg" alt="" aria-hidden="true" loading="lazy" decoding="async";
    }
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
                    @if !m.steps.is_empty() {
                        a."btn"."btn-outline-secondary" href={ (m.base) "/" (m.slug) "/slides" } {
                            i."bi"."bi-grid-3x3-gap" aria-hidden="true" {}
                            " View all slides"
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
        // `data-workshop-progress="step"` + the slug/number let
        // `workshop-progress.js` mark this slide seen in localStorage on
        // view (no server call, no telemetry). The light-table reads the
        // same keys to paint checks and unlock the certificate.
        article.workshop-step
            data-workshop-progress="step"
            data-workshop-slug=(s.slug)
            data-slide=(s.number)
        {
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
                // The slide face: a fixed-aspect card so the deck reads
                // like Keynote. A green check rides the corner once the
                // JS has marked this slide seen.
                section.workshop-slide."card"."shadow-sm"."position-relative" {
                    span.slide-seen-badge."position-absolute"."top-0"."end-0"."m-2"
                        data-slide-seen-badge hidden
                        aria-hidden="true"
                    {
                        i."bi"."bi-check-circle-fill"."text-success"."fs-4" {}
                    }
                    div."card-body"."material-body" { (PreEscaped(s.body_html)) }
                }
                (crate::components::code::syntax_highlight_assets())
                @if !s.notes_html.is_empty() {
                    aside.presenter-notes."mt-3" aria-label="Presenter notes" {
                        p.presenter-notes-label."text-uppercase"."small"."fw-semibold"."text-body-secondary"."mb-2" {
                            "Presenter notes"
                        }
                        div.presenter-notes-body { (PreEscaped(s.notes_html)) }
                    }
                }
                div."text-center"."mt-3" {
                    a."small"."text-decoration-none" href={ (s.base) "/" (s.slug) "/slides" } {
                        i."bi"."bi-grid-3x3-gap" aria-hidden="true" {}
                        " View all slides"
                    }
                }
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

/// The light-table grid: every slide as a thumbnail, a client-side
/// progress count, and a certificate form unlocked once all slides are
/// seen. Progress lives only in `localStorage` (no telemetry); the
/// `data-*` hooks drive `workshop-progress.js`.
#[must_use]
pub fn slides(t: &LightTable<'_>, auth: AuthState) -> Markup {
    let title = format!("{} — slides", t.workshop_title);
    let action = format!("{}/{}/certificate", t.base, t.slug);
    let total = t.slides.len();
    let body = html! {
        article.workshop-lighttable
            data-workshop-progress="lighttable"
            data-workshop-slug=(t.slug)
            data-total=(total)
        {
            div.container {
                nav."mb-3" aria-label="Back to workshop" {
                    a."small"."text-decoration-none" href={ (t.base) "/" (t.slug) } {
                        "← " (t.workshop_title)
                    }
                }
                header."d-flex"."justify-content-between"."align-items-center"."flex-wrap"."gap-2"."mb-3" {
                    h1."h3"."mb-0" { (t.workshop_title) }
                    span.workshop-progress-count."badge"."text-bg-secondary" data-progress-count {
                        "0 / " (total) " viewed"
                    }
                }
                p."text-body-secondary" {
                    "Open any slide to read it — each one earns a green check, kept only in this browser and "
                    "never sent anywhere. View them all to unlock your certificate."
                }
                div."row"."row-cols-2"."row-cols-md-3"."row-cols-lg-4"."g-3" {
                    @for sl in t.slides {
                        div."col" {
                            a."slide-thumb-link"."text-decoration-none"."text-reset"
                                href={ (t.base) "/" (t.slug) "/step/" (sl.number) }
                                data-slide=(sl.number)
                            {
                                div."card"."h-100"."shadow-sm"."position-relative" {
                                    span.slide-seen-badge."position-absolute"."top-0"."end-0"."m-1"
                                        data-slide-seen-badge hidden aria-hidden="true"
                                    {
                                        i."bi"."bi-check-circle-fill"."text-success"."fs-5" {}
                                    }
                                    // A shrunk peek of the slide face. Inline
                                    // styles match the existing progress-bar
                                    // pattern (CSP allows inline style).
                                    div."slide-thumb-preview"."card-body"."p-2"."overflow-hidden"
                                        aria-hidden="true"
                                        style="height:9rem;font-size:.5rem;line-height:1.2"
                                    {
                                        (PreEscaped(sl.body_html))
                                    }
                                    div."card-footer"."small"."text-truncate"."py-1" {
                                        (sl.number) ". " (sl.title)
                                    }
                                }
                            }
                        }
                    }
                }
                (crate::components::code::syntax_highlight_assets())
                // Revealed by `workshop-progress.js` only when every slide
                // has been viewed. Completion is client-trusted by design
                // (no telemetry), so this gate is a courtesy, not an
                // access control.
                section.workshop-certificate."card"."border-success"."mt-4"
                    data-cert-gate hidden
                {
                    div."card-body" {
                        h2."h4" { "You finished — claim your certificate" }
                        p {
                            "Enter your name and email and the Neon Law Foundation will send a PDF "
                            "certificate of completion."
                        }
                        form."row"."g-2"."align-items-end" method="post" action=(action) {
                            input type="hidden" name="csrf_token" value=(t.csrf_token);
                            div."col-12"."col-md-4" {
                                label."form-label"."small" for="cert-name" { "Your name" }
                                input."form-control" type="text" id="cert-name" name="name"
                                    required maxlength="120" placeholder="Jane Q. Student";
                            }
                            div."col-12"."col-md-5" {
                                label."form-label"."small" for="cert-email" { "Email" }
                                input."form-control" type="email" id="cert-email" name="email"
                                    required maxlength="254" placeholder="you@example.com";
                            }
                            div."col-12"."col-md-3"."d-grid" {
                                button."btn"."btn-success" type="submit" { "Email my certificate" }
                            }
                        }
                        p."form-text"."mt-2"."mb-0" {
                            "We use your email only to send this certificate."
                        }
                    }
                }
            }
        }
    };
    PageLayout::new(&title)
        .with_description("Every slide in the workshop, at a glance.")
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

/// Neutral confirmation after a certificate request. The same page shows
/// whether or not the address was valid, so the endpoint isn't an oracle.
#[must_use]
pub fn certificate_sent(workshop_title: &str, base: &str, auth: AuthState) -> Markup {
    let body = html! {
        article.workshop-cert-sent {
            div.container."text-center"."py-5" {
                h1 { "Check your inbox" }
                p.lead {
                    "Your certificate for " em { (workshop_title) }
                    " is on its way from the Neon Law Foundation."
                }
                p."text-body-secondary" {
                    "It can take a minute to arrive — and it's worth checking your spam folder."
                }
                a."btn"."btn-outline-secondary" href=(base) { "← Back to the workshop" }
            }
        }
    };
    PageLayout::new("Certificate on its way")
        .with_description("Your workshop completion certificate is on its way.")
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
        landing, overview, step, EventCard, MaterialCard, MaterialOverview, StepSummary,
        WorkshopStep, LANDING_TITLE,
    };
    use crate::brand::{foundation_email, FOUNDATION_BRAND};

    fn sample_cards() -> Vec<MaterialCard<'static>> {
        vec![
            MaterialCard {
                href: "/foundation/nebula/workshops/use-the-navigator",
                title: "Using the Neon Law Navigator",
                audience: "For lawyers",
                benefit: "You walk out with a deed-of-sale notation you built yourself.",
            },
            MaterialCard {
                href: "/foundation/nebula/workshops/deploy-the-navigator",
                title: "Deploy the Neon Law Navigator",
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
    fn landing_titles_the_page_nebula() {
        let html = landing(&[], &[], &[], crate::AuthState::Anonymous).into_string();
        assert!(html.contains(&format!(
            "<title>{} | Nebula</title>",
            FOUNDATION_BRAND.site_name
        )));
        // The sole <h1> is the overview title, not a single workshop's.
        assert_eq!(html.matches("<h1>").count(), 1, "exactly one <h1>: {html}");
        assert!(html.contains(&format!(">{LANDING_TITLE}</h1>")));
    }

    #[test]
    fn landing_shows_an_inviting_preloaded_hero() {
        let html = landing(&[], &[], &[], crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains("class=\"nebula-hero"),
            "Nebula overview should carry the hero shell"
        );
        assert!(
            html.contains("fetchpriority=\"high\""),
            "hero should be loaded eagerly"
        );
        assert!(
            html.contains("rel=\"preload\" as=\"image\""),
            "hero should preload its image"
        );
    }

    #[test]
    fn landing_empty_falls_back_to_real_foundation_email() {
        let html = landing(&[], &[], &[], crate::AuthState::Anonymous).into_string();
        let email = foundation_email();
        assert!(html.contains(&format!("mailto:{email}")));
    }

    #[test]
    fn landing_lists_each_workshop_with_audience_and_benefit() {
        let cards = sample_cards();
        let html = landing(&cards, &[], &[], crate::AuthState::Anonymous).into_string();
        // Each card links to its overview at the canonical per-workshop URL,
        // titled by its short name and tagged with who it's for.
        assert!(html.contains("href=\"/foundation/nebula/workshops/use-the-navigator\""));
        assert!(html.contains(
            "<h3><a href=\"/foundation/nebula/workshops/use-the-navigator\">Using the Neon Law Navigator</a></h3>"
        ));
        assert!(html.contains("For lawyers"));
        assert!(html.contains("href=\"/foundation/nebula/workshops/deploy-the-navigator\""));
        assert!(html.contains("For operators"));
        // You-voiced benefit copy renders, and the "more coming" footer
        // signals the surface grows.
        assert!(html.contains("You walk out"));
        assert!(html.contains("land here as we run them"));
    }

    #[test]
    fn landing_separates_workshops_presentations_and_events() {
        let workshops = vec![MaterialCard {
            href: "/foundation/nebula/workshops/use-the-navigator",
            title: "Using the Navigator",
            audience: "For lawyers",
            benefit: "You walk out with a notation.",
        }];
        let presentations = vec![MaterialCard {
            href: "/foundation/nebula/presentations/rust-in-peace",
            title: "Rust in Peace",
            audience: "For the hackers",
            benefit: "You walk out able to argue from real code.",
        }];
        let events = vec![EventCard {
            href: "/foundation/nebula/show-and-tell/seattle-summer-2026",
            title: "Seattle Summer 2026",
            time: "July 2, 2026, 11:00 AM-3:00 PM PT",
            place: "Private lounge",
            description: "A practical AI workflow gathering.",
        }];
        let html = landing(
            &workshops,
            &presentations,
            &events,
            crate::AuthState::Anonymous,
        )
        .into_string();
        assert!(html.contains(">Workshops</h2>"));
        assert!(html.contains(">Presentations</h2>"));
        // The show-and-tell section is labelled "Show-and-tell" (not "Events")
        // and carries the people/meeting icon.
        assert!(html.contains("Show-and-tell</h2>"));
        assert!(html.contains("bi-people-fill"));
        // Show-and-tell sits above Presentations and below Workshops.
        let workshops_at = html.find(">Workshops</h2>").expect("workshops heading");
        let show_tell_at = html
            .find("Show-and-tell</h2>")
            .expect("show-and-tell heading");
        let presentations_at = html
            .find(">Presentations</h2>")
            .expect("presentations heading");
        assert!(
            workshops_at < show_tell_at && show_tell_at < presentations_at,
            "order should be Workshops → Show-and-tell → Presentations: {html}"
        );
        assert!(html.contains("href=\"/foundation/nebula/presentations/rust-in-peace\""));
        assert!(html.contains("href=\"/foundation/nebula/show-and-tell/seattle-summer-2026\""));
        // The preview links out to the full paginated index.
        assert!(html.contains("View all show-and-tells"));
    }

    #[test]
    fn overview_carries_exactly_one_h1_and_links_each_step() {
        let steps = sample_steps();
        let m = MaterialOverview {
            base: "/foundation/nebula/workshops",
            slug: "use-the-navigator",
            title: "Runbook",
            description: "How.",
            intro_html: "<p>Orientation.</p>",
            body_html: "",
            steps: &steps,
            md_href: "/foundation/nebula/workshops/use-the-navigator.md",
        };
        let html = overview(&m, crate::AuthState::Anonymous).into_string();
        // The chrome's title is the sole <h1> — no duplicate.
        assert_eq!(html.matches("<h1>").count(), 1, "exactly one <h1>: {html}");
        assert!(html.contains(&format!(
            "<title>{} | Runbook</title>",
            FOUNDATION_BRAND.site_name
        )));
        // Start button + every step links to its step route.
        assert!(html.contains("href=\"/foundation/nebula/workshops/use-the-navigator/step/1\""));
        assert!(html.contains("href=\"/foundation/nebula/workshops/use-the-navigator/step/2\""));
        assert!(html.contains("href=\"/foundation/nebula/workshops/use-the-navigator/step/3\""));
        assert!(html.contains("Start →"));
    }

    #[test]
    fn overview_copy_button_and_link_point_at_the_md_twin() {
        let steps = sample_steps();
        let md = "/foundation/nebula/workshops/use-the-navigator.md";
        let m = MaterialOverview {
            base: "/foundation/nebula/workshops",
            slug: "use-the-navigator",
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
            base: "/foundation/nebula/workshops",
            slug: "use-the-navigator",
            workshop_title: "Runbook",
            title: "Build the template",
            body_html: "<pre><code class=\"language-rust\">fn main() {}</code></pre>",
            notes_html: "<p>Notes.</p>",
            number: 1,
            total: 3,
            steps: &steps,
        };
        let step_html = step(&s, crate::AuthState::Anonymous).into_string();
        let m = MaterialOverview {
            base: "/foundation/nebula/workshops",
            slug: "use-the-navigator",
            title: "Runbook",
            description: "How.",
            intro_html: "",
            body_html: "",
            steps: &steps,
            md_href: "/foundation/nebula/workshops/use-the-navigator.md",
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
            base: "/foundation/nebula/workshops",
            slug: "use-the-navigator",
            title: "Runbook",
            description: "How.",
            intro_html: "",
            body_html: "<h2>Only section</h2><p>x</p>",
            steps: &[],
            md_href: "/foundation/nebula/workshops/use-the-navigator.md",
        };
        let html = overview(&m, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("<h2>Only section</h2>"));
        assert!(!html.contains("Start →"));
    }

    #[test]
    fn step_renders_progress_prev_next_and_single_body() {
        let steps = sample_steps();
        let s = WorkshopStep {
            base: "/foundation/nebula/workshops",
            slug: "use-the-navigator",
            workshop_title: "Runbook",
            title: "Build the template",
            body_html: "<h2>Build the template</h2><p>do it</p>",
            notes_html: "<p>Walk the room through why.</p>",
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
        assert!(html.contains("href=\"/foundation/nebula/workshops/use-the-navigator/step/1\""));
        assert!(html.contains("href=\"/foundation/nebula/workshops/use-the-navigator/step/3\""));
        assert!(html.contains("← Previous"));
        assert!(html.contains("Next →"));
        // The step body renders; the chrome title is the only <h1>.
        assert!(html.contains("<h2>Build the template</h2>"));
        assert_eq!(html.matches("<h1>").count(), 0, "step body has no <h1>");
    }

    #[test]
    fn step_links_honor_the_provided_base() {
        // The "Rust in Peace" talk is a Nebula presentation now (`rust-in-peace`
        // slug); every generated link threads the `base` + `slug` it was
        // given, so a talk and a runbook share one chrome.
        let steps = sample_steps();
        let s = WorkshopStep {
            base: "/foundation/nebula/presentations",
            slug: "rust-in-peace",
            workshop_title: "Rust in Peace",
            title: "One language, every library",
            body_html: "<h2>One language, every library</h2>",
            notes_html: "<p>Speaker notes.</p>",
            number: 2,
            total: 3,
            steps: &steps,
        };
        let html = step(&s, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("href=\"/foundation/nebula/presentations/rust-in-peace/step/1\""));
        assert!(html.contains("href=\"/foundation/nebula/presentations/rust-in-peace/step/3\""));
        assert!(html.contains("href=\"/foundation/nebula/presentations/rust-in-peace\""));
    }

    #[test]
    fn first_step_offers_overview_not_previous() {
        let steps = sample_steps();
        let s = WorkshopStep {
            base: "/foundation/nebula/workshops",
            slug: "use-the-navigator",
            workshop_title: "Runbook",
            title: "Install",
            body_html: "<h2>Install</h2>",
            notes_html: "<p>Notes.</p>",
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
            base: "/foundation/nebula/workshops",
            slug: "use-the-navigator",
            workshop_title: "Runbook",
            title: "Notarize",
            body_html: "<h2>Notarize</h2>",
            notes_html: "<p>Notes.</p>",
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
