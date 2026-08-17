//! The impersonation banner for authenticated Dioxus pages.
//!
//! When an admin acts as a client, every page that admin sees must say so and
//! must offer the way out. The chrome has carried this since impersonation
//! landed ([`views::layout`]); this is its Dioxus counterpart, so a migrated
//! page does not silently drop the affordance.
//!
//! The view model is supplied by the server — a component never infers that a
//! session is impersonating. `portal` reads `SessionData.impersonation` and
//! hands the already-decided values down, the same shape
//! [`crate::components::NavigatorNavbar`] uses for destinations.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// Who the viewer is currently acting as, and the token the stop form needs.
///
/// `Serialize`/`Deserialize` because this crosses the server-function boundary
/// inside a page's view struct; `PartialEq` because Dioxus diffs props.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpersonationView {
    /// The impersonated person's display name.
    pub target_name: String,
    /// The impersonated person's email, shown so two people with the same name
    /// are still distinguishable.
    pub target_email: String,
    /// Per-session CSRF token for the stop form. Empty renders no hidden field,
    /// matching the banner's behavior on middleware-free test paths.
    pub csrf_token: String,
}

/// Where the stop form posts. The handler is Axum-side and unchanged.
pub const IMPERSONATION_STOP_ACTION: &str = "/app/impersonation/stop";

/// Request-extension carrier for [`ImpersonationView`].
///
/// `portal` injects this from `SessionData` (which `webapp` cannot see) and a
/// page's `#[server]` loader extracts it, the same seam
/// [`crate::portal_project_list::PersonId`] uses. A distinct newtype rather than
/// a bare `Option<ImpersonationView>` so no other injector can collide with it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Impersonating(pub Option<ImpersonationView>);

/// The banner itself. Renders nothing when `view` is `None`, so a page can
/// place it unconditionally at the top of its chrome.
///
/// `role="status"` matches the banner: it is an ambient state
/// announcement, not an alert demanding immediate action.
#[component]
pub fn ImpersonationBanner(view: Option<ImpersonationView>) -> Element {
    let Some(view) = view else {
        return rsx! {};
    };
    // Built outside `rsx!` so the literals carry no format-string punctuation:
    // the angle brackets around the email would otherwise sit adjacent to an
    // interpolation and confuse the macro's format parser.
    let target = format!("Impersonating {}", view.target_name);
    let email = format!("<{}>", view.target_email);
    rsx! {
        div { class: "impersonation-banner", role: "status",
            strong { class: "impersonation-banner__target", "{target}" }
            span { class: "impersonation-banner__email", "{email}" }
            form {
                class: "impersonation-banner__stop",
                method: "post",
                action: IMPERSONATION_STOP_ACTION,
                if !view.csrf_token.is_empty() {
                    input { r#type: "hidden", name: "_csrf", value: "{view.csrf_token}" }
                }
                button {
                    r#type: "submit",
                    class: "nav-btn nav-btn--danger",
                    "aria-label": "End impersonation",
                    "End impersonation"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssr(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    fn libra() -> ImpersonationView {
        ImpersonationView {
            target_name: "Libra Nakamoto".to_string(),
            target_email: "libra@example.com".to_string(),
            csrf_token: "SESSION_TOKEN".to_string(),
        }
    }

    #[test]
    fn banner_names_the_target_and_offers_the_way_out() {
        fn app() -> Element {
            rsx! { ImpersonationBanner { view: Some(libra()) } }
        }
        let html = ssr(app);
        // Match the text node itself rather than `class="…">Text`: SSR may wrap
        // text in hydration comments, and the angle brackets around the email
        // escape as numeric entities.
        assert!(html.contains(">Impersonating Libra Nakamoto<"), "{html}");
        assert!(html.contains("&#60;libra@example.com&#62;"), "{html}");
        assert!(
            html.contains(r#"action="/app/impersonation/stop""#),
            "the stop form must post to the handler: {html}",
        );
        assert!(html.contains(r#"method="post""#), "{html}");
        assert!(html.contains(r#"aria-label="End impersonation""#), "{html}");
        assert!(html.contains(r#"role="status""#), "{html}");
    }

    #[test]
    fn banner_threads_the_csrf_token_and_omits_the_field_when_empty() {
        fn with_token() -> Element {
            rsx! { ImpersonationBanner { view: Some(libra()) } }
        }
        fn without_token() -> Element {
            rsx! {
                ImpersonationBanner {
                    view: Some(ImpersonationView { csrf_token: String::new(), ..libra() }),
                }
            }
        }

        let html = ssr(with_token);
        assert!(html.contains(r#"name="_csrf""#), "{html}");
        assert!(html.contains("SESSION_TOKEN"), "{html}");

        let html = ssr(without_token);
        assert!(!html.contains(r#"name="_csrf""#), "{html}");
    }

    #[test]
    fn no_banner_renders_for_a_session_that_is_not_impersonating() {
        fn app() -> Element {
            rsx! { ImpersonationBanner { view: None } }
        }
        let html = ssr(app);
        assert!(!html.contains("impersonation-banner"), "{html}");
        assert!(!html.contains("Impersonating"), "{html}");
    }
}
