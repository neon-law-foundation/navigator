//! Admin coupons page — the reusable-discount listing plus the "mint a
//! coupon" form.
//!
//! Xero has no coupon object, so a coupon is a Neon Law Navigator concept: it holds
//! the *intent* of a standing discount. Applying one to a subscription
//! resolves it to a `billing::LineDiscount` and snapshots that onto the
//! subscription, so editing or expiring the coupon later never re-prices
//! an existing client.

use maud::{html, Markup};

use crate::components::form::{Choice, Field, FormCard};
use crate::pages::admin::subscriptions::{fmt_discount, ProductOption};
use crate::PageLayout;

/// One coupon row, as the listing needs it. Owns its strings.
pub struct CouponRow {
    pub code: String,
    /// Pre-formatted discount (`99%`, `$5.00 off`).
    pub discount: String,
    /// Product scope (`nexus`) or `any`.
    pub scope: String,
    /// Redemptions used vs. cap (`3 / 5`, or `3` when uncapped).
    pub redemptions: String,
    /// Expiry date or `never`.
    pub expires: String,
    pub active: bool,
}

impl CouponRow {
    /// Build a display row from the raw coupon columns.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        code: String,
        discount_percent: Option<i32>,
        discount_amount_cents: Option<i64>,
        product_code: Option<String>,
        redeemed_count: i32,
        max_redemptions: Option<i32>,
        expires_at: Option<String>,
        active: bool,
    ) -> Self {
        let redemptions = match max_redemptions {
            Some(max) => format!("{redeemed_count} / {max}"),
            None => redeemed_count.to_string(),
        };
        let expires = expires_at.unwrap_or_else(|| "never".to_string());
        Self {
            code,
            discount: fmt_discount(discount_percent, discount_amount_cents),
            scope: product_code.unwrap_or_else(|| "any".to_string()),
            redemptions,
            expires,
            active,
        }
    }
}

/// The coupons admin page: the mint form, then the table of every coupon.
#[must_use]
pub fn page(
    rows: &[CouponRow],
    products: &[ProductOption],
    csrf_token: &str,
    error: Option<&str>,
) -> Markup {
    let mut scope_choices = vec![Choice::new("", "Any product")];
    for p in products {
        scope_choices.push(Choice::new(&p.code, &p.name));
    }
    let fields = vec![
        Field::text("Code", "code", "")
            .required()
            .help("The redeemable code staff hand a client, e.g. FRIEND99."),
        Field::number("Discount percent", "discount_percent", "")
            .help("Whole percent off list (0–100). Set this OR a flat amount, not both."),
        Field::number("Discount amount (cents)", "discount_amount_cents", "")
            .help("Flat amount off list, in cents."),
        Field::select("Product scope", "product_code", scope_choices, None)
            .help("Restrict the coupon to one product, or leave as Any."),
        Field::input("Expires", "expires_at", "", "date")
            .help("Optional. The coupon is rejected on or after this date (UTC)."),
        Field::number("Max redemptions", "max_redemptions", "")
            .help("Optional cap on how many subscriptions may apply this coupon."),
    ];
    let form = FormCard::new("Mint a coupon", "/portal/admin/coupons", "Create")
        .fields(fields)
        .csrf(csrf_token)
        .error(error)
        .section_heading()
        .render();

    let body = html! {
        section.admin {
            h1."mb-2" { "Coupons" }
            p."text-body-secondary"."mb-4" {
                "Reusable named discounts. A coupon is resolved and "
                em { "snapshotted" }
                " onto a subscription when applied, so later editing or expiring it never changes "
                "an existing client's invoice."
            }
            div."mb-5" { (form) }
            div."table-responsive" {
                table."table"."align-middle" {
                    thead { tr {
                        th { "Code" }
                        th { "Discount" }
                        th { "Scope" }
                        th { "Redemptions" }
                        th { "Expires" }
                        th { "Active" }
                    } }
                    tbody {
                        @if rows.is_empty() {
                            tr { td colspan="6"."text-body-secondary" { "No coupons yet." } }
                        }
                        @for row in rows {
                            tr {
                                td { code { (row.code) } }
                                td { (row.discount) }
                                td { (row.scope) }
                                td { (row.redemptions) }
                                td { (row.expires) }
                                td {
                                    @if row.active {
                                        span."badge"."text-bg-success" { "active" }
                                    } @else {
                                        span."badge"."text-bg-secondary" { "inactive" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    PageLayout::new("Coupons")
        .with_description("Reusable discount coupons and the mint-a-coupon form.")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{page, CouponRow};
    use crate::pages::admin::subscriptions::ProductOption;

    #[test]
    fn coupon_row_formats_redemptions_and_scope() {
        let capped = CouponRow::new(
            "FRIEND99".into(),
            Some(99),
            None,
            Some("nexus".into()),
            2,
            Some(5),
            None,
            true,
        );
        assert_eq!(capped.discount, "99%");
        assert_eq!(capped.scope, "nexus");
        assert_eq!(capped.redemptions, "2 / 5");
        assert_eq!(capped.expires, "never");

        let uncapped = CouponRow::new("ANY".into(), None, Some(500), None, 7, None, None, true);
        assert_eq!(uncapped.scope, "any");
        assert_eq!(uncapped.redemptions, "7");
        assert_eq!(uncapped.discount, "$5.00 off");
    }

    #[test]
    fn page_renders_form_and_rows() {
        let products = vec![ProductOption {
            code: "nexus".into(),
            name: "Neon Law Nexus".into(),
        }];
        let rows = vec![CouponRow::new(
            "FRIEND99".into(),
            Some(99),
            None,
            Some("nexus".into()),
            0,
            None,
            None,
            true,
        )];
        let html = page(&rows, &products, "tok-2", None).into_string();
        assert!(html.contains("FRIEND99"));
        assert!(html.contains("action=\"/portal/admin/coupons\""));
        assert!(html.contains("value=\"tok-2\""));
        assert!(html.contains("99%"));
    }
}
