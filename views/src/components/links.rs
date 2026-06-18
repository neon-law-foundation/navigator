//! `external_link` — the single front door for anchors whose `href`
//! leaves our own domains.
//!
//! Anchors used to be hand-written inline across the page views, so
//! the security + signposting story drifted: some had `target="_blank"`
//! but no `rel`, some `rel="noopener"` but no `noreferrer`, none of
//! them flagged "this leaves the site." Routing every off-site link
//! through one helper makes the next external link correct by default.
//!
//! Three things every off-site link must do (OWASP + a11y):
//!   1. open in a new tab — `target="_blank"`;
//!   2. sever the opener + referrer channel — `rel="noopener noreferrer"`
//!      (`noopener` kills `window.opener`; `noreferrer` also covers
//!      older engines and strips the `Referer` header);
//!   3. visibly say "this leaves the site" via the decorative
//!      `bi-box-arrow-up-right` glyph, `aria-hidden` so the anchor text
//!      stays the accessible label.
//!
//! Internal/relative (`/foo`), `mailto:`, and `tel:` links are **out
//! of scope** — they stay plain `a href=...` with no `target`/`rel`
//! and no arrow.

use maud::{html, Markup, PreEscaped};

/// An anchor to an off-site URL, carrying the security attributes and
/// the "opens in a new tab" arrow icon. Builder so the handful of call
/// sites that need extra `<a>` classes (footer bar-admission links) or
/// a hover `title` (the trademark mark) compose cleanly without a
/// combinatorial pile of free functions — mirrors the [`RowActions`]
/// builder idiom next door.
///
/// [`RowActions`]: crate::components::RowActions
#[derive(Debug)]
#[must_use]
pub struct ExternalLink<'a> {
    href: &'a str,
    class: Option<&'a str>,
    title: Option<&'a str>,
}

impl<'a> ExternalLink<'a> {
    /// New off-site link to `href`. No extra classes or title until
    /// the caller opts in.
    pub const fn new(href: &'a str) -> Self {
        Self {
            href,
            class: None,
            title: None,
        }
    }

    /// Set the `class` attribute on the `<a>` (e.g. `link-secondary`
    /// for the muted footer links).
    pub const fn with_class(mut self, class: &'a str) -> Self {
        self.class = Some(class);
        self
    }

    /// Set the `title` attribute (hover tooltip). Used by the
    /// trademark mark to carry the registration number.
    pub const fn with_title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Render the anchor around `body` (the visible link text/markup).
    #[must_use]
    pub fn render(&self, body: Markup) -> Markup {
        // Take `body` by value (ergonomic call sites pass `html! { … }`
        // directly), then move it into the output via `into_string` so
        // it's genuinely consumed — `(body)` alone only borrows, which
        // clippy flags as `needless_pass_by_value`. The string is
        // already escaped, hence `PreEscaped`.
        let body = PreEscaped(body.into_string());
        html! {
            a href=(self.href)
              class=[self.class]
              title=[self.title]
              target="_blank"
              rel="noopener noreferrer" {
                (body)
                // Leading space so the glyph doesn't crowd the last
                // character. The `<i class="bi ...">` font glyph reuses
                // the vendored Bootstrap Icons CSS linked in `layout.rs`
                // — same idiom as `row_actions.rs`. Decorative, so
                // `aria-hidden`: the anchor text is the label.
                " "
                i.bi."bi-box-arrow-up-right" aria-hidden="true" {}
            }
        }
    }
}

/// The common case: an off-site anchor with no extra classes.
#[must_use]
pub fn external_link(href: &str, body: Markup) -> Markup {
    ExternalLink::new(href).render(body)
}

/// An off-site anchor that also needs a `class` on the `<a>` (e.g. the
/// `link-secondary` footer links).
#[must_use]
pub fn external_link_with_class(href: &str, class: &str, body: Markup) -> Markup {
    ExternalLink::new(href).with_class(class).render(body)
}

#[cfg(test)]
mod tests {
    use super::{external_link, external_link_with_class, ExternalLink};
    use maud::html;

    #[test]
    fn external_link_opens_in_new_tab_safely() {
        let html = external_link("https://example.org/", html! { "Example" }).into_string();
        assert!(html.contains("target=\"_blank\""), "missing target: {html}");
        assert!(
            html.contains("rel=\"noopener noreferrer\""),
            "missing the OWASP rel pair: {html}",
        );
    }

    #[test]
    fn external_link_renders_the_leaves_site_arrow_icon() {
        let html = external_link("https://example.org/", html! { "Example" }).into_string();
        assert!(
            html.contains("bi-box-arrow-up-right"),
            "missing the upper-right arrow glyph: {html}",
        );
        // The glyph is decorative; the anchor text is the label.
        assert!(
            html.contains("aria-hidden=\"true\""),
            "arrow icon must be aria-hidden: {html}",
        );
    }

    #[test]
    fn external_link_carries_href_and_body() {
        let html = external_link("https://example.org/path", html! { "Read more" }).into_string();
        assert!(html.contains("href=\"https://example.org/path\""), "{html}");
        assert!(html.contains("Read more"), "{html}");
    }

    #[test]
    fn external_link_with_class_sets_the_anchor_class() {
        let html =
            external_link_with_class("https://example.org/", "link-secondary", html! { "Bar" })
                .into_string();
        assert!(html.contains("class=\"link-secondary\""), "{html}");
        assert!(html.contains("target=\"_blank\""), "{html}");
        assert!(html.contains("rel=\"noopener noreferrer\""), "{html}");
        assert!(html.contains("bi-box-arrow-up-right"), "{html}");
    }

    #[test]
    fn builder_emits_title_when_set() {
        let html = ExternalLink::new("https://example.org/")
            .with_title("Registered trademark")
            .render(html! { sup { "®" } })
            .into_string();
        assert!(html.contains("title=\"Registered trademark\""), "{html}");
        assert!(html.contains("<sup>®</sup>"), "{html}");
    }

    #[test]
    fn builder_omits_class_and_title_attributes_when_unset() {
        let html = ExternalLink::new("https://example.org/")
            .render(html! { "Plain" })
            .into_string();
        // Inspect only the opening <a ...> tag — the decorative <i>
        // glyph legitimately carries a `class`.
        let anchor = &html[..html.find('>').expect("anchor opens")];
        assert!(
            !anchor.contains("class="),
            "no class attr expected on the anchor: {anchor}",
        );
        assert!(
            !anchor.contains("title="),
            "no title attr expected on the anchor: {anchor}",
        );
    }
}
