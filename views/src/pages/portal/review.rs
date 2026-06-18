//! `GET /portal/projects/:id/review/:doc_id` — the comment-only client
//! review surface (Northstar Phase A).
//!
//! The document is rendered read-only; the `<northstar-review>` custom
//! element (`/public/js/northstar-review.js`) upgrades it in the browser
//! to support text selection, a comment sidebar, and range highlights.
//! Everything degrades to a plain, readable document with no JavaScript.
//!
//! `body_html` is the attorney-reviewed draft, sanitized when the
//! generation workflow stored it, so it is rendered verbatim with
//! [`PreEscaped`]. The comment thread is handed to the element as a
//! JSON `data-` attribute (maud HTML-escapes attribute values, so a
//! comment body containing markup can't break out).

use maud::{html, Markup, PreEscaped};
use uuid::Uuid;

use crate::PageLayout;

/// Self-contained styling for the review surface: the document/sidebar
/// two-column layout and the comment-range highlight colour. Kept inline
/// so the feature needs no new vendored stylesheet.
const REVIEW_CSS: &str = "\
.northstar-review-page northstar-review{display:grid;grid-template-columns:minmax(0,1fr) 20rem;\
gap:1.5rem;align-items:start}\
.northstar-review-page .nr-document{line-height:1.7}\
.northstar-review-page .nr-sidebar{position:sticky;top:1rem}\
.northstar-review-page .nr-document ::selection{background:#fde68a}\
::highlight(nr-comment){background-color:#fde68a}\
@media (max-width:48rem){.northstar-review-page northstar-review{grid-template-columns:1fr}}";

pub struct ReviewPage<'a> {
    pub project_id: Uuid,
    pub doc_id: Uuid,
    pub title: &'a str,
    pub kind: &'a str,
    pub status: &'a str,
    /// Attorney-reviewed, sanitized draft body as HTML.
    pub body_html: &'a str,
    /// The comment thread, serialized as JSON for the viewer element.
    pub comments_json: &'a str,
    pub csrf_token: &'a str,
}

#[must_use]
pub fn render(p: &ReviewPage<'_>) -> Markup {
    let comments_url = format!(
        "/portal/projects/{}/review/{}/comments",
        p.project_id, p.doc_id
    );
    let body = html! {
        style { (PreEscaped(REVIEW_CSS)) }
        section."portal northstar-review-page" {
            nav."mb-3" {
                a href=(format!("/portal/projects/{}", p.project_id)) { "← Back to your matter" }
            }
            header."mb-4" {
                h1."mb-1" { (p.title) }
                p."mb-2" {
                    span."badge text-bg-secondary text-uppercase me-2" { (p.kind) }
                    span."badge text-bg-light text-uppercase" { (p.status) }
                }
                p."text-body-secondary mb-0" {
                    "Read your document below. Select any text to leave a comment — "
                    "you can't edit the document here, only comment. Nothing is final "
                    "until you've had your say."
                }
            }
            northstar-review
                data-create-url=(comments_url)
                data-comments=(p.comments_json)
                data-csrf=(p.csrf_token)
                data-doc-id=(p.doc_id.to_string())
            {
                article."nr-document card p-4" {
                    (PreEscaped(p.body_html))
                }
                aside."nr-sidebar" {
                    noscript {
                        p."text-body-secondary" {
                            "Enable JavaScript to add comments. You can still read the "
                            "document above."
                        }
                    }
                }
            }
        }
    };
    PageLayout::new(p.title)
        .with_description("Review your document and leave comments.")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render, ReviewPage};
    use uuid::Uuid;

    fn page() -> Markup {
        render(&ReviewPage {
            project_id: Uuid::nil(),
            doc_id: Uuid::nil(),
            title: "Last Will and Testament",
            kind: "will",
            status: "pending_review",
            body_html: "<h2>Article I</h2><p>I, Libra, declare this my will.</p>",
            comments_json: "[]",
            csrf_token: "tok123",
        })
    }

    use maud::Markup;

    #[test]
    fn renders_title_document_body_and_review_element() {
        let html = page().into_string();
        assert!(html.contains("Last Will and Testament"));
        // The attorney-reviewed body is rendered verbatim (PreEscaped).
        assert!(html.contains("<h2>Article I</h2>"));
        assert!(html.contains("I, Libra, declare this my will."));
        // The custom element + its data contract are present.
        assert!(html.contains("<northstar-review"));
        assert!(html.contains("data-create-url=\"/portal/projects/"));
        assert!(html.contains("data-csrf=\"tok123\""));
    }

    #[test]
    fn back_link_points_at_the_matter() {
        let html = page().into_string();
        assert!(html.contains("href=\"/portal/projects/00000000-0000-0000-0000-000000000000\""));
    }
}
