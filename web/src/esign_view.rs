//! Embedded in-portal signing page (Phase 1.2b).
//!
//! After the retainer is sent for signature the client is a **captive**
//! DocuSign recipient (see [`crate::retainer_walk::client_user_id`]) —
//! DocuSign does not email them. Instead this route asks the provider for
//! a short-lived [recipient view] URL and iframes it, so the client signs
//! inside Neon Law Navigator rather than leaving for an emailed DocuSign link.
//!
//! `GET /portal/admin/notations/:id/sign`
//!
//! The signing URL is single-use and expires in minutes, so the page is
//! rendered fresh on each request and never cached. The handler is
//! generic over the [`crate::signature::SignatureProvider`] seam, so the
//! stub returns a deterministic fake URL in dev / KIND.
//!
//! [recipient view]: https://developers.docusign.com/docs/esign-rest-api/reference/envelopes/envelopeviews/createrecipient/

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use maud::{html, Markup, DOCTYPE};
use sea_orm::EntityTrait;
use uuid::Uuid;

use crate::admin::AdminState;
use crate::retainer_walk::client_user_id;
use crate::signature::RecipientView;
use store::entity::{notation, person};

/// `GET /portal/admin/notations/:id/sign` — request an embedded signing
/// URL for the notation's captive client and render it in an iframe.
pub async fn sign_get(
    State(state): State<AdminState>,
    AxumPath(notation_id): AxumPath<Uuid>,
) -> Response {
    let Some(notation_row) = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await
        .ok()
        .flatten()
    else {
        return (StatusCode::NOT_FOUND, "notation not found").into_response();
    };

    // The envelope must already exist (the retainer walk records the id in
    // `signatures` when it parks at `sent_for_signature__pending`). No id →
    // there is nothing to sign yet.
    let Some(request_id) = store::signatures::request_id_for_notation(&state.db, notation_id)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::CONFLICT,
            "this matter has not been sent for signature yet",
        )
            .into_response();
    };

    // The captive recipient is resolved on the email/name/clientUserId
    // triple, so they must match the envelope exactly: the client's
    // Person row + the notation-derived client_user_id.
    let Some(client) = person::Entity::find_by_id(notation_row.person_id)
        .one(&state.db)
        .await
        .ok()
        .flatten()
    else {
        return (StatusCode::NOT_FOUND, "client not found").into_response();
    };

    // Where DocuSign redirects the browser once the ceremony ends. Back
    // to the matter's step page, which reflects the post-signature state.
    let return_url = format!("/portal/admin/notations/{notation_id}/step");
    let view = RecipientView {
        return_url,
        email: client.email,
        name: client.name,
        client_user_id: client_user_id(notation_id),
    };

    match state
        .signature_provider
        .create_recipient_view(&crate::signature::SignatureRequestId(request_id), &view)
        .await
    {
        Ok(signing_url) => render_signing_page(&signing_url).into_response(),
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "esign_view: recipient view failed");
            (
                StatusCode::BAD_GATEWAY,
                "could not start the signing session; please retry",
            )
                .into_response()
        }
    }
}

/// Render the embedded-signing page: a full-bleed iframe pointed at the
/// provider's single-use signing URL. Pure so it unit-tests without a DB
/// or a provider round-trip.
fn render_signing_page(signing_url: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Sign your retainer" }
                style {
                    "html,body{margin:0;height:100%}\
                     iframe{border:0;width:100%;height:100vh;display:block}"
                }
            }
            body {
                iframe
                    title="Sign your retainer"
                    src=(signing_url)
                    allow="camera; microphone" {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render_signing_page;

    #[test]
    fn signing_page_iframes_the_provider_url() {
        let url = "https://demo.docusign.net/signing/abc123";
        let markup = render_signing_page(url).into_string();
        assert!(
            markup.contains(&format!("src=\"{url}\"")),
            "the signing URL is the iframe source: {markup}"
        );
        assert!(markup.contains("<iframe"), "renders an iframe");
    }

    #[test]
    fn signing_page_escapes_a_hostile_url() {
        // maud HTML-escapes attribute values, so a crafted URL can't break
        // out of the src attribute into markup.
        let markup = render_signing_page("https://x/\"><script>alert(1)</script>").into_string();
        assert!(
            !markup.contains("<script>alert(1)</script>"),
            "attribute value must be escaped: {markup}"
        );
    }
}
