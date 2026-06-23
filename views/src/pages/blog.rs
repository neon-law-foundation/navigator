//! `/blog` — the firm blog index, and `/blog/:slug` — one post.
//!
//! Rendered under the firm brand. Posts are authored as dated markdown
//! files (see [`web::blog`](../../../web/src/blog.rs)); these functions
//! only render the loaded view models, so the crate stays free of the
//! content loader and of `chrono` (the handler formats the date to a
//! string before calling in).

use maud::{html, Markup, PreEscaped};

use crate::brand::FIRM_BRAND;
use crate::{AuthState, PageLayout};

/// One post as it appears on the index — headline, date, blurb, and a
/// link to the full post. Borrowed for the duration of the render.
pub struct PostSummary<'a> {
    pub slug: &'a str,
    /// Pre-formatted publish date (e.g. `"June 19, 2026"`).
    pub date: &'a str,
    pub title: &'a str,
    pub description: &'a str,
}

/// The full content of one post.
pub struct PostContent<'a> {
    /// Pre-formatted publish date (e.g. `"June 19, 2026"`).
    pub date: &'a str,
    pub title: &'a str,
    /// Rendered HTML body (NOT raw markdown).
    pub body_html: &'a str,
}

#[must_use]
pub fn render_index(posts: &[PostSummary<'_>], auth: AuthState) -> Markup {
    let body = html! {
        article {
            h1 { "Blog" }
            p { "Notes from " (FIRM_BRAND.site_name) "." }
            @if posts.is_empty() {
                section.empty-state {
                    p { "No posts yet. Check back soon." }
                }
            } @else {
                @for post in posts {
                    article.blog-post-summary {
                        h2 { a href=(format!("/blog/{}", post.slug)) { (post.title) } }
                        p.blog-date { small { (post.date) } }
                        @if !post.description.is_empty() {
                            p { (post.description) }
                        }
                        p { a href=(format!("/blog/{}", post.slug)) { "Read more →" } }
                    }
                }
            }
        }
    };
    PageLayout::new("Blog")
        .with_description("Occasional writing from the firm.")
        .with_auth(auth)
        .render(&body)
}

#[must_use]
pub fn render_post(post: &PostContent<'_>, auth: AuthState) -> Markup {
    // A post reads as a letter, like `/foundation/mission`, so we cap its
    // measure at the same ~65 characters and center it: comfortable prose
    // measure is 45–75 characters per line, and 65ch keeps the column
    // readable on a phone without sprawling across a wide desktop. `ch`
    // tracks the body font, so the cap holds as the type scales.
    let body = html! {
        article.blog-post style="max-width: 65ch; margin-inline: auto;" {
            p { a href="/blog" { "← All posts" } }
            h1 { (post.title) }
            p.blog-date { small { (post.date) } }
            (PreEscaped(post.body_html))
        }
    };
    PageLayout::new(post.title)
        .with_description(if post.title.is_empty() {
            "A post from the firm blog."
        } else {
            post.title
        })
        .with_auth(auth)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render_index, render_post, PostContent, PostSummary};
    use crate::brand::FIRM_BRAND;

    #[test]
    fn index_renders_heading_under_firm_brand() {
        let html = render_index(&[], crate::AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!("<title>{} | Blog</title>", FIRM_BRAND.site_name)));
    }

    #[test]
    fn index_renders_empty_state_with_no_posts() {
        let html = render_index(&[], crate::AuthState::Anonymous).into_string();
        assert!(html.contains("No posts yet"));
    }

    #[test]
    fn index_links_each_post_by_slug() {
        let posts = vec![PostSummary {
            // The `web` loader hands the view kebab-case slugs.
            slug: "thanks-apple",
            date: "June 19, 2026",
            title: "Thanks, Apple",
            description: "A short note of thanks.",
        }];
        let html = render_index(&posts, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("href=\"/blog/thanks-apple\""));
        assert!(html.contains("Thanks, Apple"));
        assert!(html.contains("June 19, 2026"));
        assert!(html.contains("A short note of thanks."));
        assert!(!html.contains("No posts yet"));
    }

    #[test]
    fn post_renders_title_date_and_body() {
        let post = PostContent {
            date: "June 19, 2026",
            title: "Thanks, Apple",
            body_html: "<p>We want to say thank you.</p>",
        };
        let html = render_post(&post, crate::AuthState::Anonymous).into_string();
        assert!(html.contains(&format!(
            "<title>{} | Thanks, Apple</title>",
            FIRM_BRAND.site_name
        )));
        assert!(html.contains("June 19, 2026"));
        assert!(html.contains("We want to say thank you."));
        // Back-link to the index.
        assert!(html.contains("href=\"/blog\""));
    }

    #[test]
    fn post_is_capped_at_the_same_readable_measure_as_the_mission_letter() {
        // A post is constrained to a ~65-character measure and centered so
        // it reads as a letter, matching `/foundation/mission`.
        let post = PostContent {
            date: "June 19, 2026",
            title: "Thanks, Apple",
            body_html: "<p>body</p>",
        };
        let html = render_post(&post, crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains("class=\"blog-post\""),
            "post body should carry the blog-post class, got: {html}"
        );
        assert!(
            html.contains("max-width: 65ch"),
            "post should be capped at a 65ch measure like the mission letter, got: {html}"
        );
    }
}
