//! Admin dashboard.
//!
//! Live counts for the headline tables plus jump-off links to every
//! administrative sub-page and JSON endpoint the binary exposes.

use maud::{html, Markup};

use crate::PageLayout;

pub struct DashboardCounts {
    pub people: u64,
    pub entities: u64,
    pub jurisdictions: u64,
    pub entity_types: u64,
}

/// CRUD admin pages — full list / new / edit / delete surfaces.
const CRUD_PAGES: &[(&str, &str)] = &[
    ("/portal/admin/people", "People"),
    ("/portal/admin/entities", "Entities"),
    ("/portal/projects", "Projects"),
    ("/portal/admin/subscriptions", "Subscriptions"),
    ("/portal/admin/coupons", "Coupons"),
];

/// Read-only listings — every remaining domain table.
///
/// `entity-types`, `templates`, and `questions` live here because
/// they're seeded by the workspace (`cli import`, `store/seeds/`)
/// rather than authored from the web UI.
const LISTING_PAGES: &[(&str, &str)] = &[
    ("/portal/admin/entity-types", "Entity types"),
    ("/portal/admin/templates", "Templates"),
    ("/portal/admin/questions", "Questions"),
    ("/portal/admin/notations", "Notations"),
    ("/portal/admin/answers", "Answers"),
    ("/portal/admin/addresses", "Addresses"),
    ("/portal/admin/mailrooms", "Mailrooms"),
    ("/portal/admin/letters", "Letters"),
    ("/portal/admin/blobs", "Blobs"),
    ("/portal/admin/documents", "Documents"),
    ("/portal/admin/person-entity-roles", "Person ↔ entity roles"),
    (
        "/portal/admin/person-project-roles",
        "Person ↔ project roles",
    ),
    (
        "/portal/admin/entity-billing-profiles",
        "Entity billing profiles",
    ),
    ("/portal/admin/invoices", "Invoices"),
    ("/portal/admin/invoice-line-items", "Invoice line items"),
    ("/portal/admin/jurisdictions", "Jurisdictions"),
    ("/portal/admin/git-repositories", "Git repositories"),
    ("/portal/admin/disclosures", "Disclosures"),
    ("/portal/admin/relationship-logs", "Relationship logs"),
];

const API_ENDPOINTS: &[(&str, &str)] = &[
    ("/openapi.json", "OpenAPI 3.1 spec"),
    ("/api/people", "JSON: /api/people"),
    ("/api/entities", "JSON: /api/entities"),
    ("/api/jurisdictions", "JSON: /api/jurisdictions"),
    ("/api/entity-types", "JSON: /api/entity-types"),
];

#[must_use]
pub fn dashboard(counts: &DashboardCounts, csrf_token: &str) -> Markup {
    let body = html! {
        section.admin {
            h1."mb-2" { "Admin" }
            p."text-body-secondary"."mb-4" {
                "Live counts and links into every administrative sub-page and JSON endpoint."
            }

            h2."h4"."mb-3" { "Headline counts" }
            div."row"."row-cols-2"."row-cols-md-4"."g-3"."mb-4".admin-counts {
                (count_card("People",       counts.people))
                (count_card("Entities",     counts.entities))
                (count_card("Jurisdictions", counts.jurisdictions))
                (count_card("Entity types", counts.entity_types))
            }

            h2."h4"."mb-3" { "Operations" }
            div."mb-4".admin-operations {
                form method="post" action="/portal/admin/archives/run" {
                    input type="hidden" name="_csrf" value=(csrf_token);
                    button type="submit" class="btn btn-primary" {
                        "Run nightly export now"
                    }
                    span."text-body-secondary"."small"."ms-2" {
                        "Fires the Archives backup workflow; emails the diagnostic report."
                    }
                }
                div."mt-2" {
                    a href="/portal/admin/schedules" { "Cron schedules" }
                    span."text-body-secondary"."small"."ms-2" {
                        "All scheduled jobs and their cadence."
                    }
                }
            }

            h2."h4"."mb-3" { "Manage (full CRUD)" }
            div."list-group"."mb-4".admin-manage {
                @for (href, label) in CRUD_PAGES {
                    a class="list-group-item list-group-item-action" href=(href) { (label) }
                }
            }

            h2."h4"."mb-3" { "Read-only listings" }
            div."list-group"."mb-4".admin-listings {
                @for (href, label) in LISTING_PAGES {
                    a class="list-group-item list-group-item-action" href=(href) { (label) }
                }
            }

            h2."h4"."mb-3" { "JSON API" }
            div."list-group"."mb-4".admin-api {
                @for (href, label) in API_ENDPOINTS {
                    a class="list-group-item list-group-item-action" href=(href) { (label) }
                }
            }
        }
    };
    PageLayout::new("Admin")
        .with_description("Navigator administrative overview.")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// One Bootstrap card rendering "<label>: <count>". The trailing
/// `<strong>label: </strong>(count)` pattern is preserved so the
/// existing dashboard tests keep matching. Built on the shared
/// [`Card`](crate::components::Card) component (flat, full-height,
/// centered body) so the dashboard tile and every other card share one
/// chrome.
fn count_card(label: &str, value: u64) -> Markup {
    html! {
        div."col" {
            (crate::components::Card::new(html! {
                div."display-6"."mb-1" { (value) }
                div."text-body-secondary"."small" {
                    strong { (label) ": " } (value)
                }
            })
            .full_height()
            .center_body()
            .no_shadow()
            .render())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{dashboard, DashboardCounts, API_ENDPOINTS, CRUD_PAGES, LISTING_PAGES};

    fn empty_counts() -> DashboardCounts {
        DashboardCounts {
            people: 0,
            entities: 0,
            jurisdictions: 0,
            entity_types: 0,
        }
    }

    #[test]
    fn dashboard_renders_headline_counts() {
        let html = dashboard(
            &DashboardCounts {
                people: 3,
                entities: 5,
                jurisdictions: 2,
                entity_types: 1,
            },
            "",
        )
        .into_string();
        assert!(html.contains(&format!(
            "<title>{} | Admin</title>",
            crate::brand::FIRM_BRAND.site_name
        )));
        assert!(html.contains("People: </strong>3"));
        assert!(html.contains("Entities: </strong>5"));
        assert!(html.contains("Jurisdictions: </strong>2"));
        assert!(html.contains("Entity types: </strong>1"));
    }

    #[test]
    fn dashboard_lists_every_crud_admin_page() {
        let html = dashboard(&empty_counts(), "").into_string();
        for (href, label) in CRUD_PAGES {
            assert!(
                html.contains(&format!("href=\"{href}\"")),
                "missing CRUD link {href}",
            );
            assert!(html.contains(label), "missing CRUD label `{label}`");
        }
    }

    #[test]
    fn dashboard_lists_every_read_only_listing() {
        let html = dashboard(&empty_counts(), "").into_string();
        for (href, label) in LISTING_PAGES {
            assert!(
                html.contains(&format!("href=\"{href}\"")),
                "missing listing link {href}",
            );
            assert!(html.contains(label), "missing listing label `{label}`");
        }
    }

    #[test]
    fn dashboard_renders_manual_archives_trigger_button() {
        let html = dashboard(&empty_counts(), "tok-123").into_string();
        // The form posts to the manual-trigger route with the CSRF
        // token threaded into a hidden field.
        assert!(html.contains("action=\"/portal/admin/archives/run\""));
        assert!(html.contains("method=\"post\""));
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"tok-123\""));
        assert!(html.contains("Run nightly export now"));
        // And a link to the full cron-schedule reference page.
        assert!(html.contains("href=\"/portal/admin/schedules\""));
    }

    #[test]
    fn dashboard_lists_every_api_endpoint() {
        let html = dashboard(&empty_counts(), "").into_string();
        for (href, _) in API_ENDPOINTS {
            assert!(
                html.contains(&format!("href=\"{href}\"")),
                "missing API link {href}",
            );
        }
    }
}
