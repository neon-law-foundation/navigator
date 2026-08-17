//! Bridge from `web`'s auth layer to `mcp`'s [`mcp::Principal`].
//!
//! Both `require_google_oauth` (production) and `require_auth`
//! (KIND HS256/JWKS) leave a verified [`crate::auth::AuthClaims`]
//! in request extensions. The MCP dispatcher reads
//! `Option<Extension<mcp::Principal>>` — this middleware translates
//! one into the other so the tools see a typed, trusted email
//! without knowing about JWT internals.
//!
//! The translation is conservative: we only insert a `Principal`
//! when `require_google_oauth` is enforced. In that path the
//! claims' `sub` field is set to the OAuth-verified email
//! (`google_oauth.rs` does the assignment). In the HS256/JWKS path
//! the `sub` is whatever the IdP put there — often a user id, not
//! an email — so we don't pretend it's trusted email.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::auth::AuthClaims;
use crate::google_oauth::GoogleOauthConfig;

/// Axum middleware. Run on the `/mcp` route AFTER
/// `require_google_oauth` + `require_auth` so any `AuthClaims`
/// have already been populated.
pub async fn inject_principal(
    axum::extract::State(google_oauth): axum::extract::State<GoogleOauthConfig>,
    mut req: Request,
    next: Next,
) -> Response {
    if google_oauth.is_enforced() {
        if let Some(claims) = req.extensions().get::<AuthClaims>().cloned() {
            req.extensions_mut().insert(mcp::Principal::new(claims.sub));
        }
    }
    next.run(req).await
}
