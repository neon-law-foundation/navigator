//! Admin clause editor — add, edit, reorder, and remove the custom
//! paragraphs spliced into a single notation's assembled document before
//! it is sent. Per-matter prose without forking the shared template.

use maud::{html, Markup};
use uuid::Uuid;

use crate::PageLayout;

/// One clause row for display.
pub struct ClauseRow<'a> {
    pub id: Uuid,
    pub position: i32,
    pub body: &'a str,
}

/// The clause-editor page for one notation.
pub struct ClausesPage<'a> {
    pub notation_id: Uuid,
    /// The bound template's title, for the page chrome.
    pub flow_label: &'a str,
    pub clauses: &'a [ClauseRow<'a>],
    pub csrf_token: &'a str,
}

#[must_use]
pub fn clauses_page(view: &ClausesPage<'_>) -> Markup {
    let base = format!("/portal/admin/notations/{}/clauses", view.notation_id);
    let page_title = format!("{} — custom clauses — Admin", view.flow_label);
    let body = html! {
        section.admin {
            div.container {
                nav."mb-3" {
                    a href=(format!("/portal/admin/notations/{}/step", view.notation_id)) {
                        "← Back to the intake walk"
                    }
                }
                header."mb-4" {
                    h1."mb-1" { "Custom clauses" }
                    p."text-body-secondary mb-0" {
                        "Paragraphs added here are spliced into this matter's "
                        (view.flow_label)
                        " at its custom-clauses marker, in order. Any clause sends the "
                        "document back through attorney review before it can go out for "
                        "signature."
                    }
                }

                @if view.clauses.is_empty() {
                    p."text-body-secondary" { "No custom clauses yet." }
                } @else {
                    @for (i, clause) in view.clauses.iter().enumerate() {
                        div."card p-3 mb-3" {
                            form method="post" action=(format!("{base}/{}/edit", clause.id)) {
                                input type="hidden" name="_csrf" value=(view.csrf_token);
                                label."form-label" { "Clause " (i + 1) }
                                textarea."form-control mb-2" name="body" rows="3" required {
                                    (clause.body)
                                }
                                button."btn btn-primary btn-sm" type="submit" { "Save" }
                            }
                            div."d-flex gap-2 mt-2" {
                                (move_form(&base, clause.id, "up", "Move up", view.csrf_token))
                                (move_form(&base, clause.id, "down", "Move down", view.csrf_token))
                                form method="post" action=(format!("{base}/{}/delete", clause.id)) {
                                    input type="hidden" name="_csrf" value=(view.csrf_token);
                                    button."btn btn-outline-danger btn-sm" type="submit" {
                                        "Delete"
                                    }
                                }
                            }
                        }
                    }
                }

                div."card p-3" {
                    form method="post" action=(&base) {
                        input type="hidden" name="_csrf" value=(view.csrf_token);
                        label."form-label" { "Add a clause" }
                        textarea."form-control mb-2" name="body" rows="3" required
                            placeholder="A custom paragraph for this matter only…" {}
                        button."btn btn-primary" type="submit" { "Add clause" }
                    }
                }
            }
        }
    };
    PageLayout::new(&page_title)
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

fn move_form(base: &str, id: Uuid, direction: &str, label: &str, csrf: &str) -> Markup {
    html! {
        form method="post" action=(format!("{base}/{id}/move")) {
            input type="hidden" name="_csrf" value=(csrf);
            input type="hidden" name="direction" value=(direction);
            button."btn btn-outline-secondary btn-sm" type="submit" { (label) }
        }
    }
}
