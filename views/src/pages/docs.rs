//! `/docs/:slug` — workspace documentation rendered on the public site
//! from the single-source-of-truth `docs/` tree (see
//! [`web::docs`](../../../web/src/docs/mod.rs)).
//!
//! Rendered under the Foundation brand, beside `/foundation/mission` —
//! these are workspace/reference docs, not an offer of representation.
//! The firm-footer disclaimer renders site-wide, as on every page.

use maud::{html, Markup, PreEscaped};

use crate::brand::FOUNDATION_BRAND;
use crate::{AuthState, PageLayout};

/// Inputs for one rendered doc page. Borrowed from the `web` crate's
/// owned [`web::docs::Doc`] for the duration of the render.
pub struct DocContent<'a> {
    pub title: &'a str,
    pub body_html: &'a str,
}

#[must_use]
pub fn render(content: &DocContent<'_>, auth: AuthState) -> Markup {
    let body = html! {
        article.docs-article {
            (PreEscaped(content.body_html))
        }
    };
    PageLayout::new(content.title)
        .with_description("Navigator workspace documentation.")
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render, DocContent};
    use crate::brand::FOUNDATION_BRAND;

    fn doc() -> DocContent<'static> {
        DocContent {
            title: "Glossary",
            body_html: "<h2 id=\"council\">Council</h2><p>A group.</p>",
        }
    }

    #[test]
    fn renders_under_foundation_brand_with_title() {
        let html = render(&doc(), crate::AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!(
            "<title>{} | Glossary</title>",
            FOUNDATION_BRAND.site_name
        )));
    }

    #[test]
    fn embeds_body_html_verbatim() {
        let html = render(&doc(), crate::AuthState::Anonymous).into_string();
        assert!(html.contains("<h2 id=\"council\">Council</h2>"));
    }
}
