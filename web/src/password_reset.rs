//! Self-service password reset for email/password (non-Google) users.
//!
//! Passwords live in **GCP Identity Platform**, not Navigator, so a reset
//! is: mint our own single-use, expiring token (the
//! [`store::email_tokens`] table), email the link through the SendGrid
//! seam, and on confirm write the new password into Identity Platform via
//! the admin door ([`crate::idp_admin`]). The flow never enumerates
//! accounts — a request for an unknown, Google-only, or unregistered email
//! returns the same neutral "check your inbox" page as a real one; only
//! the side effect (an email) differs.
//!
//! Four routes, mounted by [`crate::oauth::routes`] only when the
//! email/password door is configured:
//!
//! - `GET  /auth/password/reset`     — the "enter your email" form
//! - `POST /auth/password/reset`     — mint + email a link (neutral reply)
//! - `GET  /auth/password/reset/new` — the "choose a new password" form
//! - `POST /auth/password/reset/new` — set the new password, then sign in

use axum::extract::{Form, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use chrono::{Duration, Utc};
use sea_orm::{EntityTrait, QueryFilter};
use serde::Deserialize;
use store::entity::email_token::PURPOSE_PASSWORD_RESET;
use tower_cookies::{Cookie, Cookies};

use crate::oauth::{constant_time_eq, expired_cookie, AuthState};
use crate::session::random_token_32;

/// Signed double-submit CSRF cookie for the account-recovery forms (the
/// reset request and the email-confirm resend). Distinct from the sign-in
/// `LOGIN_CSRF` cookie so an open reset form never clobbers an in-flight
/// sign-in.
pub(crate) const ACCOUNT_CSRF_COOKIE_NAME: &str = "navigator_account_csrf";

/// How long a reset / confirm link is good for.
pub(crate) const TOKEN_TTL_MINUTES: i64 = 30;
/// Don't mint a second link for the same person inside this window — a
/// flood of "reset" submits can't spray a mailbox.
pub(crate) const THROTTLE_SECS: i64 = 60;
/// Minimum new-password length. Identity Platform enforces its own floor
/// too; this is the friendly client-side check.
const MIN_PASSWORD_LEN: usize = 8;

/// Generic CSRF-failure reply, mirroring the sign-in door.
const CSRF_ERROR: (StatusCode, &str) = (StatusCode::BAD_REQUEST, "invalid or missing CSRF token");

/// Build the password-reset sub-router (state applied by the caller).
pub fn routes() -> Router<AuthState> {
    Router::new()
        .route(
            "/auth/password/reset",
            get(request_form).post(request_submit),
        )
        .route("/auth/password/reset/new", get(new_form).post(new_submit))
}

// ── CSRF helpers (shared with `email_confirm`) ──────────────────────────

/// Mint a fresh CSRF token, drop it as a signed cookie, and return the
/// plaintext to embed as the form's hidden field (double-submit).
pub(crate) fn mint_csrf(s: &AuthState, cookies: &Cookies) -> String {
    let csrf = random_token_32();
    let signed = s.sessions.encode_signed_bytes(csrf.as_bytes());
    cookies.add(account_csrf_cookie(signed, s.secure_cookies));
    csrf
}

/// Verify the double-submit CSRF token: the value in the signed,
/// HttpOnly cookie must match the hidden form field.
pub(crate) fn verify_csrf(s: &AuthState, cookies: &Cookies, form_token: &str) -> bool {
    cookies
        .get(ACCOUNT_CSRF_COOKIE_NAME)
        .and_then(|c| s.sessions.decode_signed_bytes(c.value()))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .is_some_and(|tok| {
            !tok.is_empty() && constant_time_eq(tok.as_bytes(), form_token.as_bytes())
        })
}

pub(crate) fn account_csrf_cookie(value: String, secure: bool) -> Cookie<'static> {
    let mut c = Cookie::new(ACCOUNT_CSRF_COOKIE_NAME, value);
    c.set_http_only(true);
    c.set_secure(secure);
    c.set_same_site(tower_cookies::cookie::SameSite::Lax);
    c.set_path("/");
    c.set_max_age(tower_cookies::cookie::time::Duration::minutes(
        TOKEN_TTL_MINUTES,
    ));
    c
}

// ── Request a reset ─────────────────────────────────────────────────────

async fn request_form(State(s): State<AuthState>, cookies: Cookies) -> Response {
    let csrf = mint_csrf(&s, &cookies);
    views::password_reset_request_page(&csrf, None).into_response()
}

#[derive(Deserialize)]
struct RequestForm {
    email: String,
    #[serde(default)]
    csrf_token: String,
}

async fn request_submit(
    State(s): State<AuthState>,
    cookies: Cookies,
    Form(form): Form<RequestForm>,
) -> Response {
    if !verify_csrf(&s, &cookies, &form.csrf_token) {
        return CSRF_ERROR.into_response();
    }
    cookies.add(expired_cookie(ACCOUNT_CSRF_COOKIE_NAME));

    // Best-effort side effect; the reply is always the same neutral page so
    // a caller can't tell a registered address from an unknown one.
    if let Err(e) = try_send_reset(&s, form.email.trim()).await {
        // A failure here (DB, IdP, mail) is logged but never surfaced —
        // surfacing it would itself be an enumeration oracle.
        tracing::warn!(error = %e, "password-reset: request side effect failed");
    }
    views::password_reset_sent_page().into_response()
}

/// The side effect behind a reset request: if the email maps to a
/// Navigator person who is a password (non-Google) Identity Platform
/// account, mint a token and email the link. Any "no account / not a
/// password user / throttled" case is a silent no-op.
async fn try_send_reset(s: &AuthState, email: &str) -> anyhow::Result<()> {
    if email.is_empty() {
        return Ok(());
    }
    let Some(admin) = s.identity_admin.as_ref() else {
        tracing::warn!("password-reset: no Identity Platform admin config; cannot send a link");
        return Ok(());
    };
    let Some(person) = find_person_by_email(&s.db, email).await? else {
        return Ok(());
    };
    // Only password accounts are resettable — never set a password on a
    // Google-federated identity (it would silently add a password door).
    // A Google account gets a truthful "you sign in with Google" notice
    // instead of silence, so the requester isn't left wondering why no
    // link arrived. Any other case (no account, an unknown federation) is
    // a silent no-op, preserving the no-enumeration property.
    match admin.lookup_by_email(&person.email).await {
        Ok(Some(info)) if info.has_password => {}
        Ok(Some(info)) if info.is_google => {
            send_google_sign_in_email(s, &person).await;
            return Ok(());
        }
        Ok(_) => return Ok(()),
        Err(e) => return Err(e.into()),
    }

    let now = Utc::now();
    // Throttle: a live link minted in the last THROTTLE_SECS suppresses a
    // second send.
    if store::email_tokens::has_live_token_since(
        &s.db,
        person.id,
        PURPOSE_PASSWORD_RESET,
        now - Duration::seconds(THROTTLE_SECS),
        now,
    )
    .await?
    {
        return Ok(());
    }

    let plaintext = random_token_32();
    store::email_tokens::mint(
        &s.db,
        person.id,
        &person.email,
        PURPOSE_PASSWORD_RESET,
        &plaintext,
        now + Duration::minutes(TOKEN_TTL_MINUTES),
    )
    .await?;

    send_reset_email(s, &person, &plaintext).await;
    Ok(())
}

async fn send_reset_email(s: &AuthState, person: &store::entity::person::Model, plaintext: &str) {
    let base_url = workflows::email::base_url_from_env();
    let reset_url = format!(
        "{}/auth/password/reset/new?token={}",
        base_url.trim_end_matches('/'),
        plaintext,
    );
    let body = workflows::email::password_reset::render_password_reset_body(
        &person.name,
        &person.email,
        &reset_url,
    );
    let html = workflows::email::password_reset::render_password_reset_html(
        &person.name,
        &person.email,
        &reset_url,
        &base_url,
    );
    let mut msg = crate::email::OutboundEmail::new(
        person.email.clone(),
        workflows::email::password_reset::password_reset_subject(),
        body,
    );
    msg.html_body = Some(html);
    msg.template_slug = Some(PURPOSE_PASSWORD_RESET.to_string());
    msg.person_id = Some(person.id.to_string());
    if let Err(e) = s.email.send(msg).await {
        tracing::warn!(error = %e, person_id = %person.id, "password-reset: email send failed");
    }
}

/// Mail the "you sign in with Google" notice: the address has no password
/// in Identity Platform, so there's nothing to reset. Carries no token —
/// only a link back to the sign-in page where the Google button lives.
async fn send_google_sign_in_email(s: &AuthState, person: &store::entity::person::Model) {
    let base_url = workflows::email::base_url_from_env();
    let login_url = format!("{}/auth/login", base_url.trim_end_matches('/'));
    let body = workflows::email::google_sign_in::render_google_sign_in_body(
        &person.name,
        &person.email,
        &login_url,
    );
    let html = workflows::email::google_sign_in::render_google_sign_in_html(
        &person.name,
        &person.email,
        &login_url,
        &base_url,
    );
    let mut msg = crate::email::OutboundEmail::new(
        person.email.clone(),
        workflows::email::google_sign_in::google_sign_in_subject(),
        body,
    );
    msg.html_body = Some(html);
    msg.template_slug = Some("google_sign_in".to_string());
    msg.person_id = Some(person.id.to_string());
    if let Err(e) = s.email.send(msg).await {
        tracing::warn!(error = %e, person_id = %person.id, "password-reset: google-notice send failed");
    }
}

// ── Set the new password ────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenQuery {
    #[serde(default)]
    token: String,
}

async fn new_form(
    State(s): State<AuthState>,
    cookies: Cookies,
    Query(q): Query<TokenQuery>,
) -> Response {
    let now = Utc::now();
    match store::email_tokens::validate(&s.db, &q.token, PURPOSE_PASSWORD_RESET, now).await {
        Ok(Some(_)) => {
            let csrf = mint_csrf(&s, &cookies);
            views::password_reset_new_page(&q.token, &csrf, None).into_response()
        }
        Ok(None) => views::auth_link_invalid_page().into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "password-reset: token validate failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct NewForm {
    token: String,
    password: String,
    #[serde(default)]
    confirm: String,
    #[serde(default)]
    csrf_token: String,
}

async fn new_submit(
    State(s): State<AuthState>,
    cookies: Cookies,
    Form(form): Form<NewForm>,
) -> Response {
    if !verify_csrf(&s, &cookies, &form.csrf_token) {
        return CSRF_ERROR.into_response();
    }
    cookies.add(expired_cookie(ACCOUNT_CSRF_COOKIE_NAME));

    let now = Utc::now();
    let token_row = match store::email_tokens::validate(
        &s.db,
        &form.token,
        PURPOSE_PASSWORD_RESET,
        now,
    )
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return views::auth_link_invalid_page().into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "password-reset: confirm validate failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response();
        }
    };

    // Password policy. On failure, re-render the form (with a fresh CSRF)
    // carrying the token so the user can correct it.
    if let Some(err) = password_policy_error(&form.password, &form.confirm) {
        let csrf = mint_csrf(&s, &cookies);
        return (
            StatusCode::BAD_REQUEST,
            views::password_reset_new_page(&form.token, &csrf, Some(err)),
        )
            .into_response();
    }

    let Some(admin) = s.identity_admin.as_ref() else {
        tracing::warn!("password-reset: no admin config at confirm; cannot set password");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            views::internal_error_page(),
        )
            .into_response();
    };

    // Resolve the Identity Platform user and write the new password.
    let local_id = match admin.lookup_by_email(&token_row.email).await {
        Ok(Some(info)) if info.has_password => info.local_id,
        Ok(_) => {
            // The account vanished or isn't a password user — treat the
            // link as dead rather than mint a password door.
            return views::auth_link_invalid_page().into_response();
        }
        Err(e) => {
            tracing::warn!(error = %e, "password-reset: lookup at confirm failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response();
        }
    };
    if let Err(e) = admin.set_password(&local_id, &form.password).await {
        tracing::warn!(error = %e, "password-reset: set_password failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            views::internal_error_page(),
        )
            .into_response();
    }

    // Spend the token last, so a failed IdP write leaves the link usable
    // for a retry.
    if let Err(e) = store::email_tokens::consume(&s.db, token_row.id, now).await {
        tracing::warn!(error = %e, "password-reset: token consume failed (password was set)");
    }
    tracing::info!(person_id = %token_row.person_id, "password-reset: password updated");
    Redirect::to("/auth/login?notice=password_reset").into_response()
}

/// Validate the new password, returning a warm error message or `None`.
fn password_policy_error(password: &str, confirm: &str) -> Option<&'static str> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Some("Your new password must be at least 8 characters.");
    }
    if password != confirm {
        return Some("Those passwords don't match. Please re-enter them.");
    }
    None
}

/// Case-insensitive person lookup by email — `lower(email) = lower($1)`
/// so a reset request with different casing still finds the row.
pub(crate) async fn find_person_by_email(
    db: &store::Db,
    email: &str,
) -> Result<Option<store::entity::person::Model>, sea_orm::DbErr> {
    use sea_orm::sea_query::{Expr, Func};
    use store::entity::person;
    person::Entity::find()
        .filter(Expr::expr(Func::lower(Expr::col(person::Column::Email))).eq(email.to_lowercase()))
        .one(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::{password_policy_error, MIN_PASSWORD_LEN};

    #[test]
    fn policy_rejects_short_passwords() {
        let short = "a".repeat(MIN_PASSWORD_LEN - 1);
        assert!(password_policy_error(&short, &short).is_some());
    }

    #[test]
    fn policy_rejects_mismatch() {
        assert!(password_policy_error("abcd1234", "abcd9999").is_some());
    }

    #[test]
    fn policy_accepts_matching_long_password() {
        assert!(password_policy_error("abcd1234", "abcd1234").is_none());
    }
}
