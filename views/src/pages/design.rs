//! `/design` — the firm's living design system.
//!
//! A public, no-login reference surface that renders the shared Bootstrap
//! building blocks against the brand palette so a contributor can see what
//! the components look like (and which class names to reach for) in one
//! place. Every block here is the *real* component the rest of the app
//! uses — the [`Card`](crate::components::Card) and
//! [`Toast`](crate::components::Toast) builders, the
//! [`pricing_section`](crate::components::pricing_section), the
//! [`FormCard`](crate::components::FormCard), and the navbar baked into
//! [`PageLayout`](crate::PageLayout) — so the gallery can never drift from
//! production.
//!
//! The primary color is the brand cyan (Tailwind cyan-500 `#06b6d4`),
//! remapped onto Bootstrap's `primary` in `web/public/css/brand.css`; this
//! page is where that mapping is shown off.

use maud::{html, Markup};

use crate::components::{
    code_block, pricing_section, syntax_highlight_assets, Card, Field, FormCard, PricingCard,
    Toast, ToastTone,
};
use crate::{AuthState, PageLayout};

/// One code sample on the gallery, copied verbatim from a real workspace
/// source file. `code` is an exact substring of `source`; the
/// `snippets_are_exact_copies_of_cited_sources` test reads each `source`
/// from the workspace and fails the build if a snippet drifts — the same
/// grounding the "Rust in Peace" talk uses for its slides.
struct CodeSnippet {
    /// Workspace-relative path to the file this snippet is copied from.
    source: &'static str,
    /// What the snippet demonstrates.
    caption: &'static str,
    /// The code, verbatim from `source`.
    code: &'static str,
}

/// The gallery's grounded snippets — real component source a contributor
/// can copy-paste, each guarded by the drift test below.
const SNIPPETS: &[CodeSnippet] = &[
    CodeSnippet {
        source: "views/src/components/card.rs",
        caption: "The Card component",
        code: "pub struct Card {
    header: Option<Markup>,
    body: Markup,
    footer: Option<Markup>,
    emphasis: Emphasis,
    full_height: bool,
    center_body: bool,
    shadow: bool,
}",
    },
    CodeSnippet {
        source: "views/src/components/card.rs",
        caption: "The cyan anchor treatment — one builder call",
        code: "    pub fn highlighted(mut self) -> Self {
        self.emphasis = Emphasis::Highlighted;
        self
    }",
    },
    CodeSnippet {
        source: "views/src/components/toast.rs",
        caption: "Toast tones — Primary is the brand cyan",
        code: "pub enum ToastTone {
    /// Red — errors and \"you must sign in\" gates.
    Danger,
    /// Green — confirmations.
    Success,
    /// Brand cyan — neutral notices.
    Primary,
    /// Amber — non-blocking warnings.
    Warning,
}",
    },
    CodeSnippet {
        source: "views/src/components/toast.rs",
        caption: "Pin a toast to the top-right overlay",
        code: "pub fn toast_overlay(toasts: &Markup) -> Markup {
    html! {
        div.\"toast-container\".\"position-fixed\".\"top-0\".\"end-0\".\"p-3\" { (toasts) }
    }
}",
    },
];

/// Render the design system gallery.
#[must_use]
pub fn render(auth: AuthState) -> Markup {
    let body = html! {
        section."mb-5" id="design" {
            h1."mb-2" { "Design system" }
            p."lead"."text-body-secondary" {
                "The Bootstrap building blocks Neon Law's surfaces share, painted "
                "with the brand "
                span."text-primary"."fw-semibold" { "cyan" }
                " primary (Tailwind cyan-500 "
                code { "#06b6d4" }
                "). Each block below is the same component the live app renders."
            }
        }

        (palette_section())
        (buttons_section())
        (cards_section())
        (pricing_cards_section())
        (toasts_section())
        (navbar_section(auth))
        (forms_section())
        (code_section())
        // Highlight.js (vendored) for the code samples above — the same
        // highlighter the talk slides use. Loaded once, at the end.
        (syntax_highlight_assets())
    };

    PageLayout::new("Design system")
        .with_description(
            "Neon Law's design system — the shared Bootstrap cards, toasts, navbar, \
             and brand cyan palette.",
        )
        .with_auth(auth)
        .render(&body)
}

/// A labeled `<section>` with a heading, used to frame each component group.
fn group(title: &str, blurb: &str, inner: &Markup) -> Markup {
    html! {
        section."mb-5" {
            h2."h3"."border-bottom"."pb-2"."mb-3" { (title) }
            p."text-body-secondary" { (blurb) }
            (inner)
        }
    }
}

fn palette_section() -> Markup {
    // The brand family from brand.css: cyan-500 primary plus the link
    // hover / active / subtle shades. (Solid buttons can't carry white
    // text on cyan-500 at WCAG AA, so `.btn-primary` keeps the cyan-500
    // fill but uses a dark cyan ink and BRIGHTENS toward cyan-400/300 on
    // hover/active — see the `.btn-primary` block in brand.css.)
    let swatches = [
        ("Primary (cyan-500)", "#06b6d4", "bg-primary"),
        ("Link hover (cyan-600)", "#0891b2", ""),
        ("Link active (cyan-700)", "#0e7490", ""),
        ("Subtle bg", "#cffafe", "bg-primary-bg-subtle"),
    ];
    group(
        "Color",
        "One cyan everywhere — \"primary\", \"the blue\", and \"the cyan\" all resolve to it.",
        &html! {
            div."row"."row-cols-2"."row-cols-md-4"."g-3" {
                @for (label, hex, util) in swatches {
                    div."col" {
                        (Card::new(html! {
                            div."rounded"."mb-2" style=(format!(
                                "height:4rem;background:{hex};"
                            )) {}
                            div."fw-semibold" { (label) }
                            code."small"."text-body-secondary" { (hex) }
                            @if !util.is_empty() {
                                div { code."small" { "." (util) } }
                            }
                        })
                        .full_height()
                        .render())
                    }
                }
            }
        },
    )
}

fn buttons_section() -> Markup {
    group(
        "Buttons",
        "Cyan solid for the primary action, outline for the secondary.",
        &html! {
            div."d-flex"."flex-wrap"."gap-2"."align-items-center" {
                button."btn"."btn-primary" type="button" { "Primary" }
                button."btn"."btn-outline-primary" type="button" { "Outline" }
                button."btn"."btn-secondary" type="button" { "Secondary" }
                button."btn"."btn-primary" disabled type="button" { "Disabled" }
                a."btn"."btn-link" href="#design" { "Link" }
            }
        },
    )
}

fn cards_section() -> Markup {
    group(
        "Cards",
        "The shared Card component — plain, highlighted (cyan anchor), and with a footer.",
        &html! {
            div."row"."row-cols-1"."row-cols-md-3"."g-4" {
                div."col" {
                    (Card::new(html! {
                        h3."card-title"."h5" { "Plain card" }
                        p."card-text"."text-body-secondary" {
                            "A shadowed container for any content."
                        }
                    })
                    .full_height()
                    .render())
                }
                div."col" {
                    (Card::new(html! {
                        h3."card-title"."h5" { "Highlighted" }
                        p."card-text"."text-body-secondary" {
                            "The cyan anchor treatment — border plus header band."
                        }
                    })
                    .header(html! { "Recommended" })
                    .highlighted()
                    .full_height()
                    .render())
                }
                div."col" {
                    (Card::new(html! {
                        h3."card-title"."h5" { "With a footer" }
                        p."card-text"."text-body-secondary" {
                            "Secondary actions and fine print live in the footer."
                        }
                    })
                    .footer(html! {
                        a."btn"."btn-sm"."btn-outline-primary" href="#design" { "Action" }
                    })
                    .full_height()
                    .render())
                }
            }
        },
    )
}

fn pricing_cards_section() -> Markup {
    // The flat-fee card keeps its own component; show both one-time and
    // recurring fees so the gallery covers the shared pricing treatment.
    let cards = [
        PricingCard {
            title: "Northstar",
            price: "$3,333",
            cadence: None,
            blurb: "Launch a Delaware C-corp with clean founder paperwork.",
            features: Vec::new(),
            cta_label: "Get started",
            cta_href: "#design",
            featured_label: Some("$3,333, once"),
        },
        PricingCard {
            title: "Nexus",
            price: "$2,222",
            cadence: Some("/month"),
            blurb: "Flat monthly counsel for operating the company.",
            features: vec!["Unlimited projects", "Priority support"],
            cta_label: "Get started",
            cta_href: "#design",
            featured_label: Some("$2,222 a month, all in"),
        },
    ];
    group(
        "Pricing cards",
        "Flat-fee cards use the cyan band and a solid CTA.",
        &pricing_section(&cards, 3),
    )
}

fn toasts_section() -> Markup {
    // Rendered inline (not in the fixed overlay) so all four tones are
    // visible at once on the page.
    group(
        "Toasts",
        "Server-rendered with the static .show class; the brand cyan is the Primary tone.",
        &html! {
            div."d-flex"."flex-column"."gap-2"."align-items-start" {
                (Toast::danger("Sign in to continue.").render())
                (Toast::primary("Your draft was saved.").render())
                (Toast::success("Matter opened.").render())
                (Toast::new("Heads up — review pending.", ToastTone::Warning).render())
            }
        },
    )
}

fn navbar_section(auth: AuthState) -> Markup {
    // The live navbar sits at the top of this very page (it's baked into
    // PageLayout). Render a static, self-contained replica here so the
    // class structure is documented in one place without a second sticky
    // header. Distinct id so it never collides with the live #main-nav.
    let _ = auth;
    group(
        "Navbar",
        "The same Bootstrap navbar PageLayout renders site-wide — brand mark, \
         collapsible links, and a mobile hamburger.",
        &html! {
            nav."navbar"."navbar-expand-lg"."bg-body-tertiary"."border"."rounded" {
                div."container-fluid" {
                    a."navbar-brand"."d-flex"."align-items-center"."gap-2" href="#design" {
                        strong { "Neon Law" }
                    }
                    button."navbar-toggler" type="button"
                        data-bs-toggle="collapse"
                        data-bs-target="#design-navbar-example"
                        aria-controls="design-navbar-example"
                        aria-expanded="false"
                        aria-label="Toggle navigation"
                    {
                        span."navbar-toggler-icon" {}
                    }
                    div."collapse"."navbar-collapse" id="design-navbar-example" {
                        ul."navbar-nav"."ms-auto"."mb-2"."mb-lg-0" {
                            li."nav-item" { a."nav-link"."active" href="#design" { "Home" } }
                            li."nav-item" { a."nav-link" href="#design" { "Services" } }
                            li."nav-item" { a."nav-link" href="#design" { "About" } }
                            li."nav-item" { a."nav-link" href="#design" { "Sign in" } }
                        }
                    }
                }
            }
        },
    )
}

fn code_section() -> Markup {
    group(
        "Code",
        "Real, copy-pasteable component source — each block is verified against \
         its file by the drift test, the same grounding the talk slides use.",
        &html! {
            @for snippet in SNIPPETS {
                figure."mb-4" {
                    figcaption."small"."text-body-secondary"."mb-1" {
                        (snippet.caption) " — " code { (snippet.source) }
                    }
                    (code_block(snippet.code))
                }
            }
        },
    )
}

fn forms_section() -> Markup {
    group(
        "Forms",
        "The shared FormCard — labeled fields, a cyan submit, in a centered card.",
        &FormCard::new("Contact us", "#design", "Send")
            .fields(vec![
                Field::text("Name", "name", "").required(),
                Field::email("Email", "email", "").required(),
                Field::textarea("Message", "message", "", 4),
            ])
            .render(),
    )
}

#[cfg(test)]
mod tests {
    use super::{render, SNIPPETS};
    use crate::AuthState;

    #[test]
    fn gallery_renders_the_shared_components() {
        let out = render(AuthState::Anonymous).into_string();
        // Card component.
        assert!(out.contains("class=\"card"), "has cards: {out}");
        assert!(out.contains("border-primary"), "has a highlighted card");
        // Toast component, including the brand-cyan Primary tone.
        assert!(out.contains("text-bg-danger"), "has a danger toast");
        assert!(out.contains("text-bg-primary"), "has a cyan toast");
        assert!(out.contains("toast-body"));
        // The navbar example.
        assert!(out.contains("navbar navbar-expand-lg"), "has a navbar");
        assert!(out.contains("id=\"design-navbar-example\""));
        // Code samples + the vendored highlighter that styles them.
        assert!(out.contains("class=\"language-rust\""), "has code blocks");
        assert!(out.contains("highlight.min.js"), "loads highlight.js");
        // CSP-safe external init (inline script is blocked by script-src 'self').
        assert!(
            out.contains("highlight-init.js"),
            "loads the highlight init"
        );
        // Brand framing.
        assert!(out.contains("Design system"));
        assert!(out.contains("#06b6d4"));
    }

    /// Every gallery snippet is an exact copy of the source file it cites.
    /// Reads each cited file from the workspace (not a baked copy, which
    /// would always pass) and fails when a snippet drifts — mirrors
    /// `web::presentations`'s `talk_snippets_are_exact_copies_of_cited_sources`.
    #[test]
    fn snippets_are_exact_copies_of_cited_sources() {
        let workspace_root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
        for snippet in SNIPPETS {
            let source = std::fs::read_to_string(format!("{workspace_root}/{}", snippet.source))
                .unwrap_or_else(|e| panic!("cited source {} is unreadable: {e}", snippet.source));
            assert!(
                source.contains(snippet.code),
                "design snippet drifted from {} — update the gallery to match the source",
                snippet.source
            );
        }
        assert!(
            SNIPPETS.len() >= 4,
            "expected at least 4 grounded snippets, found {}",
            SNIPPETS.len()
        );
    }

    #[test]
    fn gallery_carries_the_shared_chrome() {
        let out = render(AuthState::Anonymous).into_string();
        // PageLayout wraps it: brand title + the live navbar at the top.
        assert!(out.contains("<title>Neon Law | Design system</title>"));
        assert!(out.contains("id=\"main-nav\""));
    }
}
