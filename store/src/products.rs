//! Product-catalog reads.
//!
//! The [`product`](crate::entity::product) table is the single source of
//! truth for each product's list price. This module is the read seam the
//! rest of the workspace uses so no caller hand-rolls a price `match`
//! again (the drift this catalog exists to kill).
//!
//! The load-bearing read is [`matter_close_fee_cents`]: given the
//! *originating* template `code` of a matter (e.g. `onboarding__estate`),
//! it returns the flat fee the firm bills when that matter closes — but
//! only for products whose `billing_kind` is
//! [`matter_close_flat`](crate::entity::product::BILLING_KIND_MATTER_CLOSE_FLAT).
//! Nautilus (recurring) and 1337 (hourly) deliberately raise no
//! matter-close fee, so they resolve to `None`.

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::entity::product;
use crate::Db;

/// Resolve the flat matter-close fee, in cents, for a matter whose
/// *originating* template has `template_code`. Returns `Some` only when a
/// product (a) names this code in `matter_close_template_code`, (b) is
/// `billing_kind = matter_close_flat`, and (c) is `active`. `None`
/// otherwise — including every non-matter-close product and the closing
/// letter itself.
///
/// This replaces the old hand-written `flat_fee_cents` `match`; the cents
/// it returns are byte-identical to that match for estate/nest/nexus.
pub async fn matter_close_fee_cents(
    db: &Db,
    template_code: &str,
) -> Result<Option<i64>, sea_orm::DbErr> {
    let row = product::Entity::find()
        .filter(product::Column::MatterCloseTemplateCode.eq(template_code))
        .filter(product::Column::BillingKind.eq(product::BILLING_KIND_MATTER_CLOSE_FLAT))
        .filter(product::Column::Active.eq(true))
        .one(db)
        .await?;
    Ok(row.map(|p| p.list_price_cents))
}

/// Fetch a product by its stable `code` (`northstar`, `nest`, `nexus`,
/// `nautilus`, `nook`, `litigation`). `None` when no such product exists.
pub async fn by_code(db: &Db, code: &str) -> Result<Option<product::Model>, sea_orm::DbErr> {
    product::Entity::find()
        .filter(product::Column::Code.eq(code))
        .one(db)
        .await
}

/// The generic retainer template a matter falls back to when its product
/// has no dedicated retainer (or no product is named). This is the
/// original one-size engagement agreement.
pub const DEFAULT_RETAINER_TEMPLATE_CODE: &str = "onboarding__retainer";

/// Resolve the retainer template `code` a matter under `product_code`
/// should open with — the product's own `retainer_template_code` when
/// set, else the generic [`DEFAULT_RETAINER_TEMPLATE_CODE`]. An unknown
/// product code also falls back to the generic retainer. This is the
/// "each product opens its own retainer" mapping, read from data rather
/// than hard-coded in the matter-open handler.
pub async fn retainer_template_code_for(
    db: &Db,
    product_code: &str,
) -> Result<String, sea_orm::DbErr> {
    let resolved = by_code(db, product_code)
        .await?
        .and_then(|p| p.retainer_template_code)
        .unwrap_or_else(|| DEFAULT_RETAINER_TEMPLATE_CODE.to_string());
    Ok(resolved)
}

/// Every `active` product, ordered by `display_name` for a deterministic
/// catalog. Backs the public `/services` page — the price a prospect sees
/// is the same `list_price_cents` Xero invoices, so the two can never
/// drift.
pub async fn list_active(db: &Db) -> Result<Vec<product::Model>, sea_orm::DbErr> {
    product::Entity::find()
        .filter(product::Column::Active.eq(true))
        .order_by_asc(product::Column::DisplayName)
        .all(db)
        .await
}

/// Every `active` product billed `recurring` (Nexus, Nautilus), ordered
/// by `display_name`. The recurring-billing workflow drives entirely off
/// this set — there is no hard-coded product list — so flipping a row's
/// `billing_kind` to `recurring` is all it takes to start billing it.
pub async fn recurring(db: &Db) -> Result<Vec<product::Model>, sea_orm::DbErr> {
    product::Entity::find()
        .filter(product::Column::Active.eq(true))
        .filter(product::Column::BillingKind.eq(product::BILLING_KIND_RECURRING))
        .order_by_asc(product::Column::DisplayName)
        .all(db)
        .await
}

/// Render a product's list price in cents as a display string with a
/// leading `$` and thousands separators (`4_400` → `"$44"`,
/// `222_200` → `"$2,222"`). Whole-dollar amounts drop the cents; a
/// fractional amount keeps two places (`111_105` → `"$1,111.05"`). The
/// single canonical formatter so the `/services` view and the catalog
/// tests agree byte-for-byte on what a price looks like.
#[must_use]
pub fn format_price(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let dollars = abs / 100;
    let rem = abs % 100;
    // Group the integer dollars into comma-separated thousands.
    let digits = dollars.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    let bytes = digits.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(*b as char);
    }
    if rem == 0 {
        format!("{sign}${grouped}")
    } else {
        format!("{sign}${grouped}.{rem:02}")
    }
}

/// The human suffix shown after the price on the catalog page
/// (`monthly` → `"/month"`, `yearly` → `"/year"`, `hourly` → `"/hour"`,
/// `once` → `" once"`, `each` → `" each"`). The one-time and per-instance
/// products carry a leading space so the price line reads `$5,555 once` or
/// `$44 each`; the slash cadences need none. Unknown cadences render no
/// suffix.
#[must_use]
pub fn cadence_suffix(cadence: &str) -> &'static str {
    match cadence {
        product::CADENCE_MONTHLY => "/month",
        product::CADENCE_YEARLY => "/year",
        product::CADENCE_HOURLY => "/hour",
        product::CADENCE_ONCE => " once",
        product::CADENCE_EACH => " each",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cadence_suffix, format_price, list_active, matter_close_fee_cents,
        retainer_template_code_for, DEFAULT_RETAINER_TEMPLATE_CODE,
    };
    use crate::entity::template;
    use crate::seed::seed_canonical;
    use crate::test_support::pg;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    async fn fs_storage() -> std::sync::Arc<dyn cloud::StorageService> {
        std::sync::Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join("navigator-products-test"))
                .await
                .unwrap(),
        )
    }

    /// The catalog must reproduce the historically-billed flat fees
    /// exactly — a cents drift here is a billing regression.
    #[tokio::test]
    async fn matter_close_fee_matches_the_historical_flat_fees() {
        let db = pg().await;
        seed_canonical(&db, &fs_storage().await).await.unwrap();
        assert_eq!(
            matter_close_fee_cents(&db, "onboarding__estate")
                .await
                .unwrap(),
            Some(333_300),
            "Northstar estate close fee"
        );
        assert_eq!(
            matter_close_fee_cents(&db, "onboarding__nest")
                .await
                .unwrap(),
            Some(111_100),
            "Nest close fee"
        );
        // Nook is a once-at-close flat fee ($9,999). The fee resolves from
        // the catalog the moment a matter closes under its originating
        // template `onboarding__realty` — even though that template (and
        // its closing/escrow workflow) is not built yet, the catalog wiring
        // is already correct, so the fee will fire the day it ships.
        assert_eq!(
            matter_close_fee_cents(&db, "onboarding__realty")
                .await
                .unwrap(),
            Some(999_900),
            "Nook brokerless-closing fee"
        );
        // The four new products open and close under their own retainer, so
        // the close fee resolves from that originating template code.
        assert_eq!(
            matter_close_fee_cents(&db, "onboarding__retainer_node")
                .await
                .unwrap(),
            Some(4_400),
            "Node on-chain attestation fee ($44 each)"
        );
        assert_eq!(
            matter_close_fee_cents(&db, "onboarding__retainer_newleaf")
                .await
                .unwrap(),
            Some(55_500),
            "Newleaf uncontested-divorce fee ($555)"
        );
        assert_eq!(
            matter_close_fee_cents(&db, "onboarding__retainer_namesake")
                .await
                .unwrap(),
            Some(77_700),
            "Namesake trademark fee ($777, one class)"
        );
        assert_eq!(
            matter_close_fee_cents(&db, "onboarding__retainer_nucleus")
                .await
                .unwrap(),
            Some(888_800),
            "Nucleus fund-formation fee ($8,888)"
        );
        // Nexus is a monthly subscription (`billing_kind: recurring`), not a
        // matter-close flat — it raises no close fee, the recurring-billing
        // workflow bills it every period instead.
        assert_eq!(
            matter_close_fee_cents(&db, "onboarding__nexus")
                .await
                .unwrap(),
            None,
            "Nexus is recurring, not a matter-close flat"
        );
    }

    /// Products with no matter-close flat fee — and unknown codes — resolve
    /// to `None`, so they never raise a close fee.
    #[tokio::test]
    async fn non_matter_close_products_raise_no_fee() {
        let db = pg().await;
        seed_canonical(&db, &fs_storage().await).await.unwrap();
        // Nautilus instruments and the closing letter are not matter-close
        // flats; an unknown code is also `None`.
        for code in [
            "nautilus__cease_communication",
            "closing__letter",
            "onboarding__retainer",
            "something_unknown",
        ] {
            assert_eq!(
                matter_close_fee_cents(&db, code).await.unwrap(),
                None,
                "{code} must raise no matter-close fee"
            );
        }
    }

    /// Each product opens its own service-specific retainer, and the
    /// mapping is wired (data), not hard-coded: the resolver returns the
    /// product's `retainer_template_code` for every catalog product, and
    /// every one of those codes resolves to a really-seeded template row.
    #[tokio::test]
    async fn each_product_opens_its_own_seeded_retainer() {
        let db = pg().await;
        seed_canonical(&db, &fs_storage().await).await.unwrap();
        for (product_code, expected_retainer) in [
            ("northstar", "onboarding__retainer_northstar"),
            ("nest", "onboarding__retainer_nest"),
            ("nexus", "onboarding__retainer_nexus"),
            ("nautilus", "onboarding__retainer_nautilus"),
            ("nook", "onboarding__retainer_nook"),
            ("litigation", "onboarding__retainer_litigation"),
            ("nerd", "onboarding__retainer_nerd"),
            ("node", "onboarding__retainer_node"),
            ("newleaf", "onboarding__retainer_newleaf"),
            ("namesake", "onboarding__retainer_namesake"),
            ("nucleus", "onboarding__retainer_nucleus"),
        ] {
            let resolved = retainer_template_code_for(&db, product_code).await.unwrap();
            assert_eq!(
                resolved, expected_retainer,
                "{product_code} must open its own retainer"
            );
            // The mapping is only "wired" if the code names a real template.
            assert!(
                template::Entity::find()
                    .filter(template::Column::Code.eq(expected_retainer))
                    .one(&db)
                    .await
                    .unwrap()
                    .is_some(),
                "retainer template `{expected_retainer}` must be seeded"
            );
        }
    }

    /// An unknown product code (or a product with no dedicated retainer)
    /// falls back to the generic engagement agreement.
    #[tokio::test]
    async fn unknown_product_falls_back_to_the_generic_retainer() {
        let db = pg().await;
        seed_canonical(&db, &fs_storage().await).await.unwrap();
        let resolved = retainer_template_code_for(&db, "no_such_product")
            .await
            .unwrap();
        assert_eq!(resolved, DEFAULT_RETAINER_TEMPLATE_CODE);
        assert_eq!(resolved, "onboarding__retainer");
    }

    #[test]
    fn format_price_groups_thousands_and_drops_whole_cents() {
        assert_eq!(format_price(4_400), "$44");
        assert_eq!(format_price(222_200), "$2,222");
        assert_eq!(format_price(333_300), "$3,333");
        assert_eq!(format_price(133_700), "$1,337");
        assert_eq!(format_price(0), "$0");
        // A fractional amount keeps two places.
        assert_eq!(format_price(111_105), "$1,111.05");
        assert_eq!(format_price(7), "$0.07");
    }

    #[test]
    fn cadence_suffix_maps_each_known_cadence() {
        assert_eq!(cadence_suffix("monthly"), "/month");
        assert_eq!(cadence_suffix("yearly"), "/year");
        assert_eq!(cadence_suffix("hourly"), "/hour");
        assert_eq!(cadence_suffix("once"), " once");
        assert_eq!(cadence_suffix("each"), " each");
        assert_eq!(cadence_suffix("???"), "");
    }

    /// The catalog lists every active product at the DB list price —
    /// Nautilus at $66/month and Nexus at $2,222/month after this change.
    #[tokio::test]
    async fn list_active_returns_the_catalog_at_db_prices() {
        let db = pg().await;
        seed_canonical(&db, &fs_storage().await).await.unwrap();
        let products = list_active(&db).await.unwrap();
        assert_eq!(products.len(), 11, "eleven active products");

        let by = |code: &str| {
            products
                .iter()
                .find(|p| p.code == code)
                .unwrap_or_else(|| panic!("{code} present"))
                .clone()
        };
        let nautilus = by("nautilus");
        assert_eq!(nautilus.list_price_cents, 6_600, "Nautilus is $66");
        assert_eq!(nautilus.cadence, "monthly");
        assert_eq!(nautilus.billing_kind, "recurring");
        assert_eq!(format_price(nautilus.list_price_cents), "$66");

        let nexus = by("nexus");
        assert_eq!(nexus.list_price_cents, 222_200, "Nexus is $2,222");
        assert_eq!(nexus.billing_kind, "recurring", "Nexus now bills monthly");
        assert_eq!(nexus.matter_close_template_code, None);
        assert_eq!(format_price(nexus.list_price_cents), "$2,222");

        // Node is the $44-each on-chain attestation: a per-instance
        // matter-close flat with the new `each` cadence suffix.
        let node = by("node");
        assert_eq!(node.list_price_cents, 4_400, "Node is $44");
        assert_eq!(node.cadence, "each");
        assert_eq!(node.billing_kind, "matter_close_flat");
        assert_eq!(format_price(node.list_price_cents), "$44");
        assert_eq!(super::cadence_suffix(&node.cadence), " each");

        // Nook repriced to $9,999 one-time.
        assert_eq!(by("nook").list_price_cents, 999_900, "Nook is $9,999");

        // Every active product carries the Xero-line requirements: a
        // currency and a mirrored item code, plus a revenue account code.
        for p in &products {
            assert!(!p.currency.is_empty(), "{} has a currency", p.code);
            assert!(
                p.xero_item_code.is_some(),
                "{} has a Xero item code",
                p.code
            );
            assert!(!p.account_code.is_empty(), "{} has an account code", p.code);
        }
    }
}
