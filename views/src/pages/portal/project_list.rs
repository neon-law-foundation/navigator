//! Project list rendered at `GET /portal` for staff and client tiers.
//!
//! The list itself is the product surface — see [`Leo`'s and
//! [`Cancer`]'s notes in the design council]. The row carries only
//! what the reader needs to recognise their matter and click into it.
//! Status appears as a small chip; "next step" wiring lands in PR 2.

use maud::{html, Markup};
use uuid::Uuid;

use crate::PageLayout;

/// One project as the portal list renders it. The handler shapes its
/// `SeaORM` rows into a `Vec<ProjectRow>` rather than passing entities
/// straight through, so the view does not depend on `store` and
/// stays trivial to test in isolation.
pub struct ProjectRow<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub status: &'a str,
}

/// Render the portal landing for a non-admin caller.
///
/// `rows` is the slice returned by [`web::access::visible_projects`]
/// — already scoped to the caller's `person_project_roles`. Empty →
/// the empty-state card; non-empty → a clickable list.
#[must_use]
pub fn render(rows: &[ProjectRow<'_>]) -> Markup {
    let body = if rows.is_empty() {
        empty_state()
    } else {
        list(rows)
    };
    PageLayout::new("Portal")
        .with_description("Your matters at Neon Law.")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

fn list(rows: &[ProjectRow<'_>]) -> Markup {
    html! {
        section."portal" {
            h1."mb-2" { "Your matters" }
            p."text-body-secondary"."mb-4" {
                "Open a matter to see its current state and what's next."
            }
            div."list-group".portal-projects {
                @for row in rows {
                    a class="list-group-item list-group-item-action"
                       href=(format!("/portal/projects/{}", row.id)) {
                        div."d-flex w-100 justify-content-between align-items-center" {
                            span."fw-semibold" { (row.name) }
                            span."badge text-bg-secondary text-uppercase" { (row.status) }
                        }
                    }
                }
            }
        }
    }
}

fn empty_state() -> Markup {
    html! {
        section."portal portal-empty" {
            h1."mb-2" { "Your portal is empty" }
            p."text-body-secondary" {
                "You don't have any matters open yet. When the firm assigns "
                "you to one, it will appear here."
            }
            p."text-body-secondary" {
                "Need help? Email "
                a href="mailto:support@neonlaw.org" { "support@neonlaw.org" }
                "."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{render, ProjectRow};
    use uuid::Uuid;

    #[test]
    fn empty_list_renders_the_empty_state_copy() {
        let html = render(&[]).into_string();
        assert!(html.contains("Your portal is empty"));
        assert!(html.contains("support@neonlaw.org"));
    }

    #[test]
    fn non_empty_list_renders_one_link_per_row() {
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let rows = [
            ProjectRow {
                id: a,
                name: "Atlas LLC",
                status: "open",
            },
            ProjectRow {
                id: b,
                name: "Borealis Trust",
                status: "draft",
            },
        ];
        let html = render(&rows).into_string();
        assert!(html.contains("Atlas LLC"));
        assert!(html.contains("Borealis Trust"));
        assert!(html.contains(&format!("href=\"/portal/projects/{a}\"")));
        assert!(html.contains(&format!("href=\"/portal/projects/{b}\"")));
        assert!(!html.contains("Your portal is empty"));
    }
}
