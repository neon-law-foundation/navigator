//! HTTP redirect service deployed to Cloud Run for `chat.neonlaw.com`.
//!
//! `chat.neonlaw.com` → fixed Gemini Enterprise landing URL
//! (regardless of path).
//!
//! Status code is 308 (`PERMANENT_REDIRECT`) to mirror the
//! workspace convention spelled out in
//! `k8s/overlays/gke/ingress/frontend-config.yaml` — clients
//! re-issue with the original method, which matters for any POST
//! traffic that ever lands on this host.
//!
//! The dispatch table lives in [`redirect_target`] — a pure
//! function over the `Host` so it's trivially unit-testable. The
//! axum wrapper in [`router`] turns `None` into 404.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Redirect;
use axum::routing::any;
use axum::Router;

const CHAT_TARGET: &str = "https://vertexaisearch.cloud.google.com/us/home/cid/1bf2ea37-8d10-473b-bd4d-f80428be4345?hl=en_US";

pub fn router() -> Router {
    Router::new().fallback(any(handler))
}

// `axum_extra::extract::Host` is deprecated (axum#3442 — it trusts
// `X-Forwarded-Host` / `Forwarded`, a spoofing footgun); read the
// `Host` header directly. This edge redirector sits behind GKE
// ingress, so the request-line `Host` is the authority we match on.
async fn handler(headers: HeaderMap) -> Result<Redirect, StatusCode> {
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::NOT_FOUND)?;
    redirect_target(host)
        .map(|t| Redirect::permanent(&t))
        .ok_or(StatusCode::NOT_FOUND)
}

/// Compute the redirect destination for a request, or `None` if
/// the host is one we don't own a rule for (handler turns that
/// into 404).
#[must_use]
pub fn redirect_target(host: &str) -> Option<String> {
    let bare = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    match bare.as_str() {
        "chat.neonlaw.com" => Some(CHAT_TARGET.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[test]
    fn chat_uses_fixed_target_regardless_of_path() {
        assert_eq!(redirect_target("chat.neonlaw.com").unwrap(), CHAT_TARGET);
    }

    #[test]
    fn host_port_suffix_is_stripped() {
        assert_eq!(
            redirect_target("chat.neonlaw.com:443").unwrap(),
            CHAT_TARGET
        );
    }

    #[test]
    fn host_case_is_normalized() {
        assert_eq!(redirect_target("CHAT.NeonLaw.com").unwrap(), CHAT_TARGET);
    }

    #[test]
    fn unknown_host_returns_none() {
        assert!(redirect_target("example.com").is_none());
        // The apex + www of neonlaw.com are intentionally NOT handled
        // here — the apex→www redirect is a DNSimple `URL` record, and
        // www is served by the stack that owns the marketing site.
        assert!(redirect_target("neonlaw.com").is_none());
        assert!(redirect_target("www.neonlaw.com").is_none());
    }

    #[tokio::test]
    async fn router_serves_chat_redirect_end_to_end() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("host", "chat.neonlaw.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(response.headers().get("location").unwrap(), CHAT_TARGET);
    }

    #[tokio::test]
    async fn router_returns_404_for_unknown_host() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("host", "example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
