//! CSRF protection for `application/x-www-form-urlencoded` POSTs
//! under `/portal/*`.
//!
//! Every authenticated session carries a 32-byte url-safe random
//! `csrf_token` in its cookie. Admin forms render that token in a
//! hidden `<input name="_csrf">`. This middleware peeks at every
//! form-encoded POST/PUT/DELETE: if the session cookie is present,
//! the submitted `_csrf` field must equal the session's token
//! (constant-time compare). Missing or mismatched → 403.
//!
//! Requests without a session cookie are passed through — they
//! can't have authenticated the user, so any state change they
//! attempt fails at the auth layer instead. This keeps the dev /
//! tests path (no session) working without per-test CSRF token
//! plumbing.

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tower_cookies::Cookies;

use crate::session::{SessionStore, SESSION_COOKIE_NAME};

/// Maximum form body we'll buffer before refusing — 1 MiB. Admin
/// forms are tiny in practice; this just bounds the middleware's
/// allocations.
pub const MAX_FORM_BODY_BYTES: usize = 1024 * 1024;

/// Form field name carrying the CSRF token.
pub const CSRF_FIELD: &str = "_csrf";

pub async fn require_csrf(
    State(sessions): State<SessionStore>,
    cookies: Cookies,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Decode the session cookie up front (if present + valid) so
    // GET handlers can render the per-session CSRF token into their
    // forms via the request's `Extension<SessionData>`.
    let session = cookies
        .get(SESSION_COOKIE_NAME)
        .and_then(|c| sessions.decode(c.value()));
    if let Some(s) = session.clone() {
        req.extensions_mut().insert(s);
    }

    // Only state-changing methods are CSRF-checked.
    if !matches!(req.method(), &Method::POST | &Method::PUT | &Method::DELETE) {
        return Ok(next.run(req).await);
    }
    // Only form-encoded bodies — JSON APIs use bearer tokens, not
    // cookie auth, so they aren't browser-CSRF-vulnerable.
    let ct = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or("").trim().to_string())
        .unwrap_or_default();
    if ct != "application/x-www-form-urlencoded" {
        return Ok(next.run(req).await);
    }
    // No session cookie → not authenticated; let auth/handler decide.
    let Some(session) = session else {
        return Ok(next.run(req).await);
    };

    // Read the body so we can extract `_csrf`. We then rebuild the
    // request so the downstream `Form<T>` extractor still parses.
    let (parts, body) = req.into_parts();
    let bytes = to_bytes(body, MAX_FORM_BODY_BYTES)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let body_str = std::str::from_utf8(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let submitted = extract_csrf_field(body_str).ok_or(StatusCode::FORBIDDEN)?;

    if !constant_time_eq(submitted.as_bytes(), session.csrf_token.as_bytes()) {
        return Err(StatusCode::FORBIDDEN);
    }

    let req = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(req).await)
}

/// Pull the first `_csrf=<value>` field out of a form-encoded body.
/// Returns the raw url-safe-base64 value (no `+`, `/`, `=` to
/// percent-decode).
#[must_use]
pub fn extract_csrf_field(body: &str) -> Option<String> {
    body.split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{CSRF_FIELD}=")))
        .map(ToString::to_string)
}

/// Constant-time `==` for byte slices of any length. Returns false
/// for length mismatch without examining contents.
#[must_use]
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::{constant_time_eq, extract_csrf_field};

    #[test]
    fn extracts_csrf_field_from_form_body() {
        assert_eq!(
            extract_csrf_field("name=Libra&_csrf=ABC123&email=a%40b").as_deref(),
            Some("ABC123"),
        );
    }

    #[test]
    fn extracts_csrf_field_when_first() {
        assert_eq!(
            extract_csrf_field("_csrf=XYZ&name=Libra").as_deref(),
            Some("XYZ"),
        );
    }

    #[test]
    fn returns_none_when_csrf_field_missing() {
        assert!(extract_csrf_field("name=Libra&email=a%40b").is_none());
        assert!(extract_csrf_field("").is_none());
    }

    #[test]
    fn constant_time_eq_matches_equal_slices() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn constant_time_eq_rejects_unequal_slices() {
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"abcd", b"abc"));
    }
}
