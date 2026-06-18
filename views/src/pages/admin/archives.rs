//! `/portal/admin/archives/run` — result page for the manual
//! "Run nightly export now" trigger.
//!
//! The button POSTs to the handler, which fires the same `Archives`
//! Restate workflow the nightly `CronJob` runs and renders one of these
//! outcomes. The diagnostic email arrives out-of-band as the
//! workflow's final durable step, so the success page tells the
//! operator to watch their inbox rather than polling a status row.

use maud::{html, Markup};

use crate::PageLayout;

/// Success: the workflow invocation was accepted by Restate.
#[must_use]
pub fn triggered(run_key: &str, notify_email: &str) -> Markup {
    let body = html! {
        section.admin { div.container {
            header.page-header {
                h1 { "Nightly export triggered" }
            }
            p {
                "The "
                code { "Archives" }
                " workflow was started with run key "
                strong { (run_key) }
                "."
            }
            p {
                "Its final step emails the diagnostic report to "
                strong { (notify_email) }
                ". Watch that inbox — the snapshot of every table runs "
                "first, so the email lands a short while after this page."
            }
            p {
                a class="btn btn-secondary" href="/portal/admin" { "Back to admin" }
            }
        } }
    };
    PageLayout::new("Nightly export triggered")
        .with_description("Manual Archives export trigger result.")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// Failure: either the deploy has no Restate broker configured (503)
/// or the ingress rejected the call. `detail` carries the specific
/// reason for the operator.
#[must_use]
pub fn failed(title: &str, detail: &str) -> Markup {
    let body = html! {
        section.admin { div.container {
            header.page-header {
                h1 { "Nightly export not triggered" }
            }
            p { strong { (title) } }
            p { code { (detail) } }
            p {
                a class="btn btn-secondary" href="/portal/admin" { "Back to admin" }
            }
        } }
    };
    PageLayout::new("Nightly export not triggered")
        .with_description("Manual Archives export trigger result.")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{failed, triggered};

    #[test]
    fn triggered_names_the_run_key_and_recipient() {
        let html = triggered("manual-abc123", "nick@neonlaw.com").into_string();
        assert!(html.contains("manual-abc123"));
        assert!(html.contains("nick@neonlaw.com"));
        assert!(html.contains("Archives"));
    }

    #[test]
    fn failed_renders_title_and_detail() {
        let html =
            failed("Restate broker not configured", "RESTATE_BROKER_URL unset").into_string();
        assert!(html.contains("Restate broker not configured"));
        assert!(html.contains("RESTATE_BROKER_URL unset"));
    }
}
