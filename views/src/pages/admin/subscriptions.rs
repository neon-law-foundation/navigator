//! Admin subscriptions page — the recurring-engagement listing plus the
//! "open a subscription" form.
//!
//! A subscription ties a billed party to a `recurring` product (Nexus,
//! Nautilus); the recurring-billing workflow raises one Xero invoice per
//! period for every `active` one. A subscription opened against a project
//! starts `pending` and activates when that project's retainer is signed —
//! so a client is never billed before the engagement agreement is executed.

use maud::{html, Markup};

use crate::components::form::{Choice, Field, FormCard};
use crate::PageLayout;

/// One product the create form can open a subscription against (the active
/// recurring catalog).
pub struct ProductOption {
    pub code: String,
    pub name: String,
}

/// One subscription row, as the listing needs it. Owns its strings so the
/// handler can map straight off the model without lifetime threading.
pub struct SubscriptionRow {
    pub id: String,
    pub product_code: String,
    pub contact_name: String,
    pub contact_email: String,
    pub status: String,
    /// Pre-formatted discount (`99%`, `$5.00 off`, or `—`).
    pub discount: String,
    /// Last invoiced period (`2026-06`) or `—` when never billed.
    pub last_invoiced_period: String,
    /// Linked project id or `—`.
    pub project: String,
}

/// Format a subscription/coupon discount for display: a percent, a flat
/// dollar amount off, or an em dash when billed at list.
#[must_use]
pub fn fmt_discount(percent: Option<i32>, amount_cents: Option<i64>) -> String {
    match (percent, amount_cents) {
        (Some(pct), _) => format!("{pct}%"),
        (_, Some(cents)) => format!("{} off", dollars(cents)),
        _ => "—".to_string(),
    }
}

/// Render cents as a `$N.NN` string (display only).
#[must_use]
pub fn dollars(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}${}.{:02}", abs / 100, abs % 100)
}

/// The subscriptions admin page: the create form, then the table of every
/// subscription. `error` renders an alert above the form on a failed POST.
#[must_use]
pub fn page(
    rows: &[SubscriptionRow],
    products: &[ProductOption],
    csrf_token: &str,
    error: Option<&str>,
) -> Markup {
    let mut product_choices = vec![Choice::new("", "Choose a product…")];
    for p in products {
        product_choices.push(Choice::new(&p.code, &p.name));
    }
    let fields = vec![
        Field::select("Product", "product_code", product_choices, None).required(),
        Field::text("Billing contact name", "contact_name", "").required(),
        Field::email("Billing contact email", "contact_email", "").required(),
        Field::text("Coupon code", "coupon", "").help(
            "Optional. A reusable code (e.g. FRIEND99); overrides the discount fields below.",
        ),
        Field::number("Discount percent", "discount_percent", "")
            .help("Optional whole percent off list (0–100). Use this OR a flat amount, not both."),
        Field::number("Discount amount (cents)", "discount_amount_cents", "")
            .help("Optional flat amount off list, in cents."),
        Field::text("Project id", "project_id", "").help(
            "Optional. Link a project so the subscription activates when its retainer is signed.",
        ),
        Field::text("Entity id", "entity_id", "").help("Optional billed-organisation link."),
        Field::text("Person id", "person_id", "").help("Optional billed-individual link."),
        Field::checkbox(
            "Bill immediately (skip the retainer gate)",
            "active",
            "true",
            false,
        ),
    ];
    let form = FormCard::new(
        "Open a subscription",
        "/portal/admin/subscriptions",
        "Create",
    )
    .fields(fields)
    .csrf(csrf_token)
    .error(error)
    .section_heading()
    .render();

    let body = html! {
        section.admin {
            h1."mb-2" { "Subscriptions" }
            p."text-body-secondary"."mb-4" {
                "Recurring engagements billed one Xero invoice per period. A subscription tied to "
                "a project stays "
                code { "pending" }
                " until that project's retainer is signed, then activates on the next billing run."
            }
            div."mb-5" { (form) }
            div."table-responsive" {
                table."table"."align-middle" {
                    thead { tr {
                        th { "Product" }
                        th { "Contact" }
                        th { "Status" }
                        th { "Discount" }
                        th { "Last billed" }
                        th { "Project" }
                    } }
                    tbody {
                        @if rows.is_empty() {
                            tr { td colspan="6"."text-body-secondary" { "No subscriptions yet." } }
                        }
                        @for row in rows {
                            tr {
                                td { code { (row.product_code) } }
                                td {
                                    div { (row.contact_name) }
                                    div."text-body-secondary"."small" { (row.contact_email) }
                                }
                                td { span."badge"."text-bg-secondary" { (row.status) } }
                                td { (row.discount) }
                                td { (row.last_invoiced_period) }
                                td."small"."text-body-secondary" { (row.project) }
                            }
                        }
                    }
                }
            }
        }
    };
    PageLayout::new("Subscriptions")
        .with_description("Recurring-engagement subscriptions and the open-a-subscription form.")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{dollars, fmt_discount, page, ProductOption, SubscriptionRow};

    #[test]
    fn fmt_discount_covers_each_shape() {
        assert_eq!(fmt_discount(Some(99), None), "99%");
        assert_eq!(fmt_discount(None, Some(500)), "$5.00 off");
        assert_eq!(fmt_discount(None, None), "—");
    }

    #[test]
    fn dollars_renders_two_places() {
        assert_eq!(dollars(2_222), "$22.22");
        assert_eq!(dollars(0), "$0.00");
    }

    #[test]
    fn page_lists_rows_and_renders_the_form() {
        let products = vec![ProductOption {
            code: "nexus".into(),
            name: "Neon Law Nexus".into(),
        }];
        let rows = vec![SubscriptionRow {
            id: "abc".into(),
            product_code: "nexus".into(),
            contact_name: "ALPS Consulting".into(),
            contact_email: "ami@alps.example".into(),
            status: "pending".into(),
            discount: "99%".into(),
            last_invoiced_period: "—".into(),
            project: "—".into(),
        }];
        let html = page(&rows, &products, "tok-1", None).into_string();
        assert!(html.contains("ALPS Consulting"));
        assert!(html.contains("99%"));
        assert!(html.contains("pending"));
        // The create form posts CSRF-protected to the collection route.
        assert!(html.contains("action=\"/portal/admin/subscriptions\""));
        assert!(html.contains("value=\"tok-1\""));
        assert!(html.contains("Neon Law Nexus"));
    }

    #[test]
    fn page_surfaces_an_error_banner() {
        let html = page(&[], &[], "t", Some("product must be recurring")).into_string();
        assert!(html.contains("product must be recurring"));
    }
}
