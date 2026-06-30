//! A reusable Bootstrap card.
//!
//! Cards are the firm's most-repeated container — admin dashboard counts,
//! the sign-in form, the design gallery — so they share one builder here
//! instead of each site hand-rolling `div.card > div.card-body`. The
//! richer pricing-tier card keeps its own component
//! ([`crate::components::pricing`]); this is the plain card the rest of the
//! app reaches for.
//!
//! `highlighted` paints the border and (when present) the header band in
//! the brand cyan — the same anchor treatment the pricing cards use — so a
//! "this one" card reads consistently everywhere.

use maud::{html, Markup};

/// Visual emphasis of a card. `Highlighted` is the cyan anchor treatment —
/// a brand border and, when a header is present, a cyan header band.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Emphasis {
    #[default]
    Plain,
    Highlighted,
}

/// A Bootstrap card composed from a body plus optional header / footer.
///
/// Build with [`Card::new`] and the chaining setters, then [`Card::render`].
/// The body is arbitrary [`Markup`]; the chrome (shadow, full height,
/// centered body, the cyan highlight) is toggled by the builder so call
/// sites stay declarative.
#[derive(Default)]
pub struct Card {
    header: Option<Markup>,
    body: Markup,
    footer: Option<Markup>,
    emphasis: Emphasis,
    full_height: bool,
    center_body: bool,
    shadow: bool,
}

impl Card {
    /// A card wrapping `body`. Shadowed by default (the common look); call
    /// [`Card::no_shadow`] for the flat dashboard variant.
    #[must_use]
    pub fn new(body: Markup) -> Self {
        Self {
            body,
            shadow: true,
            ..Self::default()
        }
    }

    /// Add a header band above the body. When the card is
    /// [`highlighted`](Card::highlighted) the band is painted cyan.
    #[must_use]
    pub fn header(mut self, header: Markup) -> Self {
        self.header = Some(header);
        self
    }

    /// Add a footer band below the body (secondary actions, fine print).
    #[must_use]
    pub fn footer(mut self, footer: Markup) -> Self {
        self.footer = Some(footer);
        self
    }

    /// Anchor treatment — cyan border, and a cyan header band when a header
    /// is set. Mirrors the highlighted pricing card.
    #[must_use]
    pub fn highlighted(mut self) -> Self {
        self.emphasis = Emphasis::Highlighted;
        self
    }

    /// `h-100` so the card fills its grid column (equal-height rows).
    #[must_use]
    pub fn full_height(mut self) -> Self {
        self.full_height = true;
        self
    }

    /// Center the body content (the dashboard count look).
    #[must_use]
    pub fn center_body(mut self) -> Self {
        self.center_body = true;
        self
    }

    /// Drop the default `shadow-sm` (flat card).
    #[must_use]
    pub fn no_shadow(mut self) -> Self {
        self.shadow = false;
        self
    }

    #[must_use]
    pub fn render(&self) -> Markup {
        let highlighted = self.emphasis == Emphasis::Highlighted;
        let mut card_class = String::from("card");
        if self.full_height {
            card_class.push_str(" h-100");
        }
        if self.shadow {
            card_class.push_str(" shadow-sm");
        }
        if highlighted {
            card_class.push_str(" border-primary");
        }
        let body_class = if self.center_body {
            "card-body text-center"
        } else {
            "card-body"
        };
        html! {
            div class=(card_class) {
                @if let Some(header) = &self.header {
                    @if highlighted {
                        div."card-header"."bg-primary"."text-white"."fw-semibold" { (header) }
                    } @else {
                        div."card-header" { (header) }
                    }
                }
                div class=(body_class) { (self.body) }
                @if let Some(footer) = &self.footer {
                    div."card-footer" { (footer) }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Card;
    use maud::html;

    #[test]
    fn plain_card_wraps_body_in_card_and_card_body() {
        let out = Card::new(html! { p { "Hello" } }).render().into_string();
        assert!(out.contains("class=\"card shadow-sm\""));
        assert!(out.contains("class=\"card-body\""));
        assert!(out.contains("Hello"));
    }

    #[test]
    fn modifiers_compose_into_the_card_class() {
        let out = Card::new(html! { "x" })
            .full_height()
            .center_body()
            .no_shadow()
            .render()
            .into_string();
        assert!(out.contains("class=\"card h-100\""));
        assert!(out.contains("class=\"card-body text-center\""));
        assert!(!out.contains("shadow-sm"));
    }

    #[test]
    fn highlighted_card_paints_border_and_header_band_cyan() {
        let out = Card::new(html! { "body" })
            .header(html! { "Recommended" })
            .highlighted()
            .render()
            .into_string();
        assert!(out.contains("card shadow-sm border-primary"));
        assert!(out.contains("card-header bg-primary text-white fw-semibold"));
        assert!(out.contains("Recommended"));
    }

    #[test]
    fn footer_renders_a_card_footer() {
        let out = Card::new(html! { "body" })
            .footer(html! { a href="/x" { "More" } })
            .render()
            .into_string();
        assert!(out.contains("class=\"card-footer\""));
        assert!(out.contains("More"));
    }
}
