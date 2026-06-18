//! Staff admin surface for recurring billing — subscriptions and coupons.
//!
//! Two collections under `/portal/admin`, each a GET listing page + a POST
//! create handler, and each branching to JSON on `?format=json` so the
//! `navigator` CLI drives the same routes the browser does (bearer auth,
//! no cookie, so the CSRF layer waves it through — see [`crate::csrf`]).
//!
//! The load-bearing rules live here, not in the view:
//!
//! - A subscription is created `pending` unless `active` is ticked, so a
//!   retainer-gated engagement is invisible to the recurring-billing
//!   workflow until [`crate::esignature_webhook`] activates it on
//!   signature.
//! - A discount may come from an inline percent/amount **or** a coupon
//!   code (not both); either way it is validated *below* the product's
//!   list price via [`billing::LineDiscount::validate`] and snapshotted
//!   onto the subscription. Xero owns the resulting invoices; Navigator
//!   owns this standing intent.

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use store::entity::product;
use store::entity::subscription;
use store::Db;
use views::pages::admin::{coupons as coupon_views, subscriptions as sub_views};

use crate::admin::is_staff_tier;
use crate::session::SessionData;

/// `?format=json` selects the CLI's machine branch over the HTML page.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FormatQuery {
    #[serde(default)]
    pub format: Option<String>,
}

impl FormatQuery {
    fn wants_json(&self) -> bool {
        self.format.as_deref() == Some("json")
    }
}

fn token(session: Option<&SessionData>) -> &str {
    session.map_or("", |s| s.csrf_token.as_str())
}

/// An empty form field posts as `""`; treat that as absent.
fn opt(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

/// Parse an optional UUID form field. `Ok(None)` for empty; `Err` for a
/// present-but-malformed value.
fn opt_uuid(s: &str, field: &str) -> Result<Option<Uuid>, String> {
    match opt(s) {
        None => Ok(None),
        Some(v) => Uuid::parse_str(v)
            .map(Some)
            .map_err(|_| format!("{field} is not a valid id")),
    }
}

/// Parse an optional integer form field.
fn opt_i64(s: &str, field: &str) -> Result<Option<i64>, String> {
    match opt(s) {
        None => Ok(None),
        Some(v) => v
            .parse::<i64>()
            .map(Some)
            .map_err(|_| format!("{field} must be a whole number")),
    }
}

/// The active recurring catalog as the create-form product dropdown.
async fn product_options(db: &Db) -> Result<Vec<sub_views::ProductOption>, sea_orm::DbErr> {
    Ok(store::products::recurring(db)
        .await?
        .into_iter()
        .map(|p| sub_views::ProductOption {
            code: p.code,
            name: p.display_name,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

/// `GET /portal/admin/subscriptions` — the listing page, or the JSON array
/// of subscriptions for the CLI.
pub async fn subscriptions_index(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    Query(q): Query<FormatQuery>,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let subs = match store::subscriptions::list_all(&db).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "admin: list subscriptions failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response();
        }
    };
    if q.wants_json() {
        return axum::Json(subs).into_response();
    }
    render_subscriptions_page(&db, token(session.as_deref()), None).await
}

/// Form body for opening a subscription. Optional numeric/uuid fields
/// arrive as possibly-empty strings (HTML forms post `""`, not absence).
#[derive(Debug, Deserialize)]
pub struct SubscriptionInput {
    pub product_code: String,
    pub contact_name: String,
    pub contact_email: String,
    #[serde(default)]
    pub coupon: String,
    #[serde(default)]
    pub discount_percent: String,
    #[serde(default)]
    pub discount_amount_cents: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub entity_id: String,
    #[serde(default)]
    pub person_id: String,
    /// Checkbox: posts `true` when ticked, absent otherwise.
    #[serde(default)]
    pub active: String,
}

/// `POST /portal/admin/subscriptions` — open a subscription. Validates the
/// product is an active recurring one and the discount stays below list,
/// snapshots the discount, and starts `pending` unless `active` is ticked.
pub async fn subscriptions_create(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    Query(q): Query<FormatQuery>,
    Form(input): Form<SubscriptionInput>,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match open_subscription(&db, input).await {
        Ok(sub) => {
            if q.wants_json() {
                return (StatusCode::CREATED, axum::Json(sub)).into_response();
            }
            Redirect::to("/portal/admin/subscriptions").into_response()
        }
        Err(msg) => {
            if q.wants_json() {
                return (
                    StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({ "error": msg })),
                )
                    .into_response();
            }
            (
                StatusCode::BAD_REQUEST,
                render_subscriptions_page(&db, token(session.as_deref()), Some(&msg)).await,
            )
                .into_response()
        }
    }
}

/// The validated creation core, shared by the JSON + HTML branches.
/// Returns the created model or a human message for a 400.
async fn open_subscription(
    db: &Db,
    input: SubscriptionInput,
) -> Result<subscription::Model, String> {
    let code = input.product_code.trim();
    let prod = store::products::by_code(db, code)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no product with code `{code}`"))?;
    if !prod.active || prod.billing_kind != product::BILLING_KIND_RECURRING {
        return Err(format!(
            "`{code}` is not an active recurring product — a subscription needs one"
        ));
    }
    if opt(&input.contact_name).is_none() || !input.contact_email.contains('@') {
        return Err("a billing contact name and a valid email are required".to_string());
    }

    // Resolve the discount: a coupon code, OR inline percent/amount, OR
    // none. A coupon and an inline discount together is ambiguous.
    let inline_pct = opt_i64(&input.discount_percent, "discount percent")?;
    let inline_amt = opt_i64(&input.discount_amount_cents, "discount amount")?;
    let coupon_code = opt(&input.coupon);
    if coupon_code.is_some() && (inline_pct.is_some() || inline_amt.is_some()) {
        return Err("use a coupon code OR the discount fields, not both".to_string());
    }

    let mut redeem_coupon: Option<Uuid> = None;
    let (discount_percent, discount_amount_cents) = if let Some(cc) = coupon_code {
        let coupon = store::coupons::resolve(db, cc, code, Utc::now())
            .await
            .map_err(|e| e.to_string())?;
        redeem_coupon = Some(coupon.id);
        (coupon.discount_percent, coupon.discount_amount_cents)
    } else {
        if inline_pct.is_some() && inline_amt.is_some() {
            return Err("set a discount percent OR a flat amount, not both".to_string());
        }
        (
            inline_pct.map(|p| i32::try_from(p).unwrap_or(i32::MAX)),
            inline_amt,
        )
    };

    // Below-list guardrail: a discount may only go below the catalog price.
    if let Some(d) = line_discount(discount_percent, discount_amount_cents) {
        d.validate(prod.list_price_cents)
            .map_err(|e| e.to_string())?;
    }

    let project_id = opt_uuid(&input.project_id, "project id")?;
    let entity_id = opt_uuid(&input.entity_id, "entity id")?;
    let person_id = opt_uuid(&input.person_id, "person id")?;

    let status = if input.active.trim() == "true" {
        subscription::STATUS_ACTIVE
    } else {
        subscription::STATUS_PENDING
    };

    let sub = store::subscriptions::create(
        db,
        store::subscriptions::NewSubscription {
            person_id,
            entity_id,
            project_id,
            product_code: code.to_string(),
            contact_name: input.contact_name.trim().to_string(),
            contact_email: input.contact_email.trim().to_string(),
            status: status.to_string(),
            started_at: Utc::now().to_rfc3339(),
            discount_percent,
            discount_amount_cents,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    // Burn the redemption only after the subscription is committed, so a
    // failed create never consumes a coupon use.
    if let Some(id) = redeem_coupon {
        if let Err(e) = store::coupons::mark_redeemed(db, id).await {
            tracing::error!(error = %e, coupon_id = %id, "coupon redeemed but count not advanced");
        }
    }
    Ok(sub)
}

/// Build a [`billing::LineDiscount`] from the snapshot columns, if any.
fn line_discount(percent: Option<i32>, amount_cents: Option<i64>) -> Option<billing::LineDiscount> {
    match (percent, amount_cents) {
        (Some(p), _) => Some(billing::LineDiscount::Percent(
            u32::try_from(p).unwrap_or(u32::MAX),
        )),
        (_, Some(c)) => Some(billing::LineDiscount::AmountCents(c)),
        _ => None,
    }
}

async fn render_subscriptions_page(db: &Db, csrf: &str, error: Option<&str>) -> Response {
    let products = product_options(db).await.unwrap_or_default();
    let rows = match store::subscriptions::list_all(db).await {
        Ok(subs) => subs
            .into_iter()
            .map(|s| sub_views::SubscriptionRow {
                id: s.id.to_string(),
                product_code: s.product_code,
                contact_name: s.contact_name,
                contact_email: s.contact_email,
                status: s.status,
                discount: sub_views::fmt_discount(s.discount_percent, s.discount_amount_cents),
                last_invoiced_period: s.last_invoiced_period.unwrap_or_else(|| "—".to_string()),
                project: s
                    .project_id
                    .map_or_else(|| "—".to_string(), |p| p.to_string()),
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::error!(error = %e, "admin: render subscriptions failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response();
        }
    };
    sub_views::page(&rows, &products, csrf, error).into_response()
}

// ---------------------------------------------------------------------------
// Coupons
// ---------------------------------------------------------------------------

/// `GET /portal/admin/coupons` — the listing page, or the JSON array.
pub async fn coupons_index(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    Query(q): Query<FormatQuery>,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let coupons = match store::coupons::list_all(&db).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "admin: list coupons failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response();
        }
    };
    if q.wants_json() {
        return axum::Json(coupons).into_response();
    }
    render_coupons_page(&db, token(session.as_deref()), None).await
}

/// Form body for minting a coupon.
#[derive(Debug, Deserialize)]
pub struct CouponInput {
    pub code: String,
    #[serde(default)]
    pub discount_percent: String,
    #[serde(default)]
    pub discount_amount_cents: String,
    #[serde(default)]
    pub product_code: String,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub max_redemptions: String,
}

/// `POST /portal/admin/coupons` — mint a coupon.
pub async fn coupons_create(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    Query(q): Query<FormatQuery>,
    Form(input): Form<CouponInput>,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match mint_coupon(&db, input).await {
        Ok(coupon) => {
            if q.wants_json() {
                return (StatusCode::CREATED, axum::Json(coupon)).into_response();
            }
            Redirect::to("/portal/admin/coupons").into_response()
        }
        Err(msg) => {
            if q.wants_json() {
                return (
                    StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({ "error": msg })),
                )
                    .into_response();
            }
            (
                StatusCode::BAD_REQUEST,
                render_coupons_page(&db, token(session.as_deref()), Some(&msg)).await,
            )
                .into_response()
        }
    }
}

async fn mint_coupon(db: &Db, input: CouponInput) -> Result<store::entity::coupon::Model, String> {
    let code = opt(&input.code)
        .ok_or("a coupon code is required")?
        .to_string();
    let discount_percent = opt_i64(&input.discount_percent, "discount percent")?
        .map(|p| i32::try_from(p).unwrap_or(i32::MAX));
    let discount_amount_cents = opt_i64(&input.discount_amount_cents, "discount amount")?;
    let product_code = opt(&input.product_code).map(ToString::to_string);
    let max_redemptions = opt_i64(&input.max_redemptions, "max redemptions")?
        .map(|m| i32::try_from(m).unwrap_or(i32::MAX));
    // A bare `YYYY-MM-DD` from the date input becomes end-of-day UTC.
    let expires_at = match opt(&input.expires_at) {
        None => None,
        Some(d) => Some(normalize_expiry(d)?),
    };

    store::coupons::create(
        db,
        store::coupons::NewCoupon {
            code,
            discount_percent,
            discount_amount_cents,
            product_code,
            expires_at,
            max_redemptions,
        },
    )
    .await
    .map_err(|e| match e {
        store::coupons::CouponError::Db(ref dberr) if store::is_unique_violation(dberr) => {
            "that coupon code already exists".to_string()
        }
        other => other.to_string(),
    })
}

/// Accept either a full RFC 3339 timestamp or a bare `YYYY-MM-DD` date
/// (from the HTML date input), normalizing the latter to end-of-day UTC.
fn normalize_expiry(input: &str) -> Result<String, String> {
    if chrono::DateTime::parse_from_rfc3339(input).is_ok() {
        return Ok(input.to_string());
    }
    let date = chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .map_err(|_| "expiry must be a date (YYYY-MM-DD)".to_string())?;
    let eod = date
        .and_hms_opt(23, 59, 59)
        .ok_or_else(|| "invalid expiry date".to_string())?;
    Ok(eod.and_utc().to_rfc3339())
}

async fn render_coupons_page(db: &Db, csrf: &str, error: Option<&str>) -> Response {
    let products = product_options(db).await.unwrap_or_default();
    let rows = match store::coupons::list_all(db).await {
        Ok(coupons) => coupons
            .into_iter()
            .map(|c| {
                coupon_views::CouponRow::new(
                    c.code,
                    c.discount_percent,
                    c.discount_amount_cents,
                    c.product_code,
                    c.redeemed_count,
                    c.max_redemptions,
                    c.expires_at,
                    c.active,
                )
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::error!(error = %e, "admin: render coupons failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response();
        }
    };
    coupon_views::page(&rows, &products, csrf, error).into_response()
}

#[cfg(test)]
mod tests {
    use super::normalize_expiry;

    #[test]
    fn normalize_expiry_accepts_a_bare_date_as_end_of_day_utc() {
        let got = normalize_expiry("2026-12-31").unwrap();
        assert!(got.starts_with("2026-12-31T23:59:59"));
    }

    #[test]
    fn normalize_expiry_passes_through_rfc3339() {
        let got = normalize_expiry("2026-12-31T12:00:00Z").unwrap();
        assert_eq!(got, "2026-12-31T12:00:00Z");
    }

    #[test]
    fn normalize_expiry_rejects_garbage() {
        assert!(normalize_expiry("soon").is_err());
    }
}
