//! HTTP redirect service deployed to Cloud Run for navigator
//! hostnames that don't host an app.
//!
//! Two hosts today:
//!
//! - `neonlaw.com` → `https://www.neonlaw.com{path_and_query}`
//!   (naked-to-www canonicalization, path-preserving).
//! - `chat.neonlaw.com` → fixed Gemini Enterprise landing URL
//!   (regardless of path, matching the legacy third-party
//!   redirector's behavior).
//!
//! Status code is 308 (`PERMANENT_REDIRECT`) to mirror the
//! workspace convention spelled out in
//! `k8s/overlays/gke/ingress/frontend-config.yaml` — clients
//! re-issue with the original method, which matters for any POST
//! traffic that ever lands on these hosts.
//!
//! The dispatch table lives in [`redirect_target`] — a pure
//! function over `(host, uri)` so it's trivially unit-testable.
//! The axum wrapper in [`router`] turns `None` into 404.

use axum::http::{StatusCode, Uri};
use axum::response::Redirect;
use axum::routing::any;
use axum::Router;
use axum_extra::extract::Host;

const CHAT_TARGET: &str = "https://vertexaisearch.cloud.google.com/us/home/cid/1bf2ea37-8d10-473b-bd4d-f80428be4345?hl=en_US";

pub fn router() -> Router {
    Router::new().fallback(any(handler))
}

async fn handler(Host(host): Host, uri: Uri) -> Result<Redirect, StatusCode> {
    redirect_target(&host, &uri)
        .map(|t| Redirect::permanent(&t))
        .ok_or(StatusCode::NOT_FOUND)
}

/// Compute the redirect destination for a request, or `None` if
/// the host is one we don't own a rule for (handler turns that
/// into 404).
#[must_use]
pub fn redirect_target(host: &str, uri: &Uri) -> Option<String> {
    let bare = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    match bare.as_str() {
        "neonlaw.com" => {
            let pq = uri.path_and_query().map_or("/", |p| p.as_str());
            Some(format!("https://www.neonlaw.com{pq}"))
        }
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

    fn u(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn naked_root_redirects_to_www() {
        assert_eq!(
            redirect_target("neonlaw.com", &u("/")).unwrap(),
            "https://www.neonlaw.com/"
        );
    }

    #[test]
    fn naked_preserves_path_and_query() {
        assert_eq!(
            redirect_target("neonlaw.com", &u("/contact?ref=footer")).unwrap(),
            "https://www.neonlaw.com/contact?ref=footer"
        );
    }

    #[test]
    fn chat_uses_fixed_target_regardless_of_path() {
        assert_eq!(
            redirect_target("chat.neonlaw.com", &u("/")).unwrap(),
            CHAT_TARGET
        );
        assert_eq!(
            redirect_target("chat.neonlaw.com", &u("/anything?q=1")).unwrap(),
            CHAT_TARGET
        );
    }

    #[test]
    fn host_port_suffix_is_stripped() {
        assert_eq!(
            redirect_target("neonlaw.com:443", &u("/")).unwrap(),
            "https://www.neonlaw.com/"
        );
    }

    #[test]
    fn host_case_is_normalized() {
        assert_eq!(
            redirect_target("Neonlaw.COM", &u("/")).unwrap(),
            "https://www.neonlaw.com/"
        );
        assert_eq!(
            redirect_target("CHAT.NeonLaw.com", &u("/")).unwrap(),
            CHAT_TARGET
        );
    }

    #[test]
    fn unknown_host_returns_none() {
        assert!(redirect_target("example.com", &u("/")).is_none());
        // www.neonlaw.com is intentionally NOT handled by this
        // service — it's served by whatever stack owns the real
        // marketing site.
        assert!(redirect_target("www.neonlaw.com", &u("/")).is_none());
    }

    #[tokio::test]
    async fn router_serves_naked_redirect_end_to_end() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/foo?a=1")
                    .header("host", "neonlaw.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(
            response.headers().get("location").unwrap(),
            "https://www.neonlaw.com/foo?a=1"
        );
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
