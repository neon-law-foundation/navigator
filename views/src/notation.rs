//! Notation renderer — fills in a template body with a context map.
//!
//! A template body is plain text with `{{var_name}}` placeholders.
//! The renderer substitutes each placeholder with the matching
//! context value (or leaves it untouched if the key is missing —
//! callers asserting on completeness can grep the rendered output)
//! and emits a maud [`Markup`] block ready to embed in a page.
//!
//! Unfilled placeholders are *not* an error; rendering a partly-
//! filled notation is a valid intermediate state. Tests use the
//! "no `{{` left in the output" assertion to detect missing keys.

use std::collections::BTreeMap;

use maud::{html, Markup};

/// Render `body` with `context` substituted into every `{{key}}`
/// placeholder. The result is wrapped in an `<article class="notation">`
/// container with one `<p>` per paragraph (blank-line-separated).
#[must_use]
pub fn render_filled_in(body: &str, context: &BTreeMap<String, String>) -> Markup {
    let mut filled = body.to_string();
    for (k, v) in context {
        filled = filled.replace(&format!("{{{{{k}}}}}"), v);
    }
    let paragraphs: Vec<&str> = filled
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    html! {
        article.notation {
            @for p in &paragraphs {
                p { (collapse_whitespace(p)) }
            }
        }
    }
}

/// Replace any run of whitespace (spaces, newlines, tabs) with a
/// single space so a multi-line paragraph in the template renders
/// as one flowing paragraph in HTML.
fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::render_filled_in;

    fn ctx(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn substitutes_single_placeholder() {
        let html = render_filled_in("Hello {{client_name}}.", &ctx(&[("client_name", "Libra")]))
            .into_string();
        assert!(html.contains("Hello Libra."), "got: {html}");
        assert!(!html.contains("{{"));
    }

    #[test]
    fn substitutes_multiple_placeholders_across_paragraphs() {
        let body = "\
I, {{client_name}}, hire the firm for {{product_description}}.

The retainer covers the project {{project_name}}.";
        let html = render_filled_in(
            body,
            &ctx(&[
                ("client_name", "Libra"),
                ("product_description", "estate planning"),
                ("project_name", "Estate planning — Libra"),
            ]),
        )
        .into_string();
        assert!(html.contains("I, Libra, hire the firm for estate planning."));
        assert!(html.contains("The retainer covers the project Estate planning — Libra."));
        // Two paragraphs → two <p> tags.
        let p_count = html.matches("<p>").count();
        assert_eq!(p_count, 2, "two paragraphs expected, html: {html}");
    }

    #[test]
    fn collapses_intra_paragraph_newlines_into_single_spaces() {
        let body = "I,\n{{client_name}},\nhire the firm.";
        let html = render_filled_in(body, &ctx(&[("client_name", "Libra")])).into_string();
        assert!(html.contains("I, Libra, hire the firm."));
    }

    #[test]
    fn leaves_unfilled_placeholders_in_output_for_grep() {
        let html = render_filled_in("Hi {{missing}}.", &ctx(&[])).into_string();
        assert!(html.contains("{{missing}}"));
    }

    #[test]
    fn empty_body_renders_empty_article() {
        let html = render_filled_in("", &ctx(&[])).into_string();
        assert!(html.contains("<article class=\"notation\">"));
        assert!(!html.contains("<p>"));
    }

    #[test]
    fn wraps_in_notation_article_class() {
        let html = render_filled_in("body", &ctx(&[])).into_string();
        assert!(html.contains("<article class=\"notation\">"), "got: {html}");
    }
}
