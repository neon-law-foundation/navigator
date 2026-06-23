//! `CommonMark` → HTML rendering for static prose pages.
//!
//! Static pages under [`crate::pages`] keep their bodies in
//! `views/content/<slug>.md` (loaded via `include_str!`) and pass the
//! source through [`render`]. The result is a [`maud::Markup`] the
//! page layout drops into its content slot.

use maud::{Markup, PreEscaped};
use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};

fn markdown_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts
}

/// Render a `CommonMark` string to HTML wrapped in [`maud::Markup`].
///
/// Enables tables, strikethrough, footnotes, and task lists so authored
/// markdown matches what the workspace's own linter accepts.
#[must_use]
pub fn render(src: &str) -> Markup {
    let parser = Parser::new_ext(src, markdown_options());
    let mut out = String::new();
    html::push_html(&mut out, parser);
    PreEscaped(out)
}

/// Like [`render`], but every link destination is passed through
/// `rewrite` first and every heading gets a slug `id` so in-page anchors
/// resolve. Used to serve repo-relative Markdown (a README, a doc) on the
/// web: a link written for a git reader (`docs/glossary.md#project`,
/// `notation_templates/x/y.md`) is retargeted onto its site route, and a
/// same-page anchor (`#trademarks`) lands on the matching heading.
#[must_use]
pub fn render_with_link_rewrite(src: &str, rewrite: impl Fn(&str) -> String) -> Markup {
    let events: Vec<Event> = Parser::new_ext(src, markdown_options()).collect();
    let mut out_events: Vec<Event> = Vec::with_capacity(events.len());

    for i in 0..events.len() {
        match &events[i] {
            // Stamp a slug id on headings that don't already declare one.
            Event::Start(Tag::Heading {
                level,
                id: None,
                classes,
                attrs,
            }) => {
                let text = heading_text(&events[i + 1..]);
                out_events.push(Event::Start(Tag::Heading {
                    level: *level,
                    id: Some(slugify(&text).into()),
                    classes: classes.clone(),
                    attrs: attrs.clone(),
                }));
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => out_events.push(Event::Start(Tag::Link {
                link_type: *link_type,
                dest_url: CowStr::from(rewrite(dest_url)),
                title: title.clone(),
                id: id.clone(),
            })),
            other => out_events.push(other.clone()),
        }
    }

    let mut out = String::new();
    html::push_html(&mut out, out_events.into_iter());
    PreEscaped(out)
}

/// Concatenate the text of a heading from the events following its
/// `Start(Heading)` up to the matching `End`. `Code` spans count as text.
fn heading_text(rest: &[Event]) -> String {
    let mut text = String::new();
    for ev in rest {
        match ev {
            Event::End(TagEnd::Heading(_)) => break,
            Event::Text(t) | Event::Code(t) => text.push_str(t),
            _ => {}
        }
    }
    text
}

/// GitHub-style heading slug: lowercase, drop punctuation, spaces → `-`,
/// keep existing hyphens and underscores.
#[must_use]
fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c == ' ' {
            out.push('-');
        } else if c == '-' || c == '_' {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn renders_heading_and_emphasis() {
        let html = render("# Hello\n\nA *bold* claim.").into_string();
        assert!(html.contains("<h1>Hello</h1>"));
        assert!(html.contains("<em>bold</em>"));
    }

    #[test]
    fn renders_bulleted_list() {
        let html = render("- first\n- second\n").into_string();
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>first</li>"));
    }

    #[test]
    fn link_rewrite_retargets_only_relative_links() {
        use super::render_with_link_rewrite;
        let src = "[a](docs/x.md) and [b](https://example.com)";
        let html = render_with_link_rewrite(src, |d| {
            if d.starts_with("http") {
                d.to_string()
            } else {
                format!("/site/{d}")
            }
        })
        .into_string();
        assert!(html.contains("href=\"/site/docs/x.md\""), "got: {html}");
        assert!(html.contains("href=\"https://example.com\""), "got: {html}");
    }

    #[test]
    fn link_rewrite_stamps_heading_ids_for_anchors() {
        use super::render_with_link_rewrite;
        // The in-page `#trademarks` anchor only resolves if the heading
        // carries a matching id.
        let html =
            render_with_link_rewrite("### Trademarks\n\n[x](#trademarks)", ToString::to_string)
                .into_string();
        assert!(html.contains("id=\"trademarks\""), "got: {html}");
    }
}
