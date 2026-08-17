//! Every retired URL the site still answers, kept alive as permanent
//! redirects.
//!
//! Two consolidations left backlinks behind, and this module is where both
//! land. The Foundation's pages served at the site root while it had a host of
//! its own, so `/mission`, `/notations`, `/transparency…`, `/show-and-tell…`,
//! and the three audience pages now `301` beneath `/foundation`. Older still,
//! `/foundation/nebula…` gathered three material catalogs that have since
//! stood on their own.
//!
//! Every entry here is a `301` or a redirect handler. The pages themselves are
//! Dioxus routers in [`crate::pages`] and [`crate::firm_pages`]; a site's
//! public surface is both halves together.
//!
//! One host serves everything now, so every destination is relative. While the
//! firm and the Foundation were separate deployments a redirect that crossed
//! between them had to be absolute; that seam is gone, and with it the whole
//! class of bug where a relative hop landed on the other host's `404`.

use portal::hosting::PublicRouter as Router;
use portal::{dioxus_app, AppState, EventIndex};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;

/// A `GET` route answering one retired path with a permanent redirect to
/// `destination`.
fn moved(from: &str, destination: &'static str) -> Router<AppState> {
    Router::new().route(
        from,
        get(move || async move { axum::response::Redirect::permanent(destination) }),
    )
}

/// Build the retired-path table: the Foundation's former root URLs, the
/// retired Nebula surface, and the legacy event URLs.
///
/// The `presentations` certificate `POST` is the one write on this surface and
/// is deliberately absent: it stays the application's, merged in by
/// [`crate::public_routes`] from `portal::nebula_presentation_command_routes`,
/// because who may claim a certificate is an authorization question rather
/// than a brand's.
pub fn retired_path_routes() -> Router<AppState> {
    foundation_root_redirects()
        .merge(retired_nebula_redirects())
        .merge(legacy_event_redirects())
}

/// The Foundation's former root URLs, each `301`ing to its `/foundation`
/// replacement.
///
/// These were live pages on `neonlaw.org` for as long as the Foundation had a
/// host of its own, so they are the most-linked retired URLs on the site.
/// `/foundation` itself is deliberately absent: it is a real page now, not a
/// redirect, which is the whole point of the consolidation.
fn foundation_root_redirects() -> Router<AppState> {
    moved("/mission", dioxus_app::FOUNDATION_MISSION_PATH)
        .merge(moved("/education", dioxus_app::FOUNDATION_EDUCATION_PATH))
        .merge(moved("/legal-aid", dioxus_app::FOUNDATION_LEGAL_AID_PATH))
        .merge(moved("/attorneys", dioxus_app::FOUNDATION_ATTORNEYS_PATH))
        .merge(moved("/notations", dioxus_app::NOTATIONS_PATH))
        .merge(moved("/transparency", dioxus_app::TRANSPARENCY_PATH))
        // The minutes prefix is registered before the bare `{slug}` so a
        // quarter key can never be read as a governance slug.
        .merge(Router::new().route(
            "/transparency/minutes/{slug}",
            get(|AxumPath(slug): AxumPath<String>| async move {
                axum::response::Redirect::permanent(&format!(
                    "/foundation/transparency/minutes/{slug}"
                ))
            }),
        ))
        .merge(Router::new().route(
            "/transparency/{slug}",
            get(|AxumPath(slug): AxumPath<String>| async move {
                axum::response::Redirect::permanent(&format!("/foundation/transparency/{slug}"))
            }),
        ))
        .merge(moved(
            "/show-and-tell",
            webapp::show_tell_index::SHOW_TELL_INDEX_PATH,
        ))
        .merge(Router::new().route(
            "/show-and-tell/{slug}",
            get(|AxumPath(slug): AxumPath<String>| async move {
                axum::response::Redirect::permanent(&format!("/foundation/show-and-tell/{slug}"))
            }),
        ))
}

/// The retired `/foundation/nebula…` surface and the two catalogs that left it.
///
/// Nebula's landing gathered three catalogs that now stand on their own, so its
/// backlinks land on the Foundation's home. The category was always the segment
/// that mattered, so it becomes the root and everything below a material rides
/// along on the wildcard.
fn retired_nebula_redirects() -> Router<AppState> {
    moved("/foundation/nebula", dioxus_app::MISSION_PATH)
        .merge(moved(
            "/foundation/workshops",
            dioxus_app::WORKSHOP_INDEX_PATH,
        ))
        .merge(moved(
            "/foundation/workshops/navigator",
            "/workshops/use-the-navigator",
        ))
        .merge(moved(
            "/foundation/nebula/show-and-tell",
            webapp::show_tell_index::SHOW_TELL_INDEX_PATH,
        ))
        .merge(Router::new().route(
            "/foundation/nebula/show-and-tell/{slug}",
            get(|AxumPath(slug): AxumPath<String>| async move {
                axum::response::Redirect::permanent(&format!("/foundation/show-and-tell/{slug}"))
            }),
        ))
        .merge(Router::new().route(
            "/foundation/nebula/{category}/{slug}",
            get(
                |AxumPath((category, slug)): AxumPath<(String, String)>| async move {
                    axum::response::Redirect::permanent(&relocated_material(&category, &slug, ""))
                },
            ),
        ))
        .merge(Router::new().route(
            "/foundation/nebula/{category}/{slug}/{*rest}",
            get(
                |AxumPath((category, slug, rest)): AxumPath<(String, String, String)>| async move {
                    axum::response::Redirect::permanent(&relocated_material(
                        &category,
                        &slug,
                        &format!("/{rest}"),
                    ))
                },
            ),
        ))
}

fn legacy_event_redirects() -> Router<AppState> {
    Router::new()
        .route("/events", get(legacy_events_redirect))
        .route("/events/{slug}", get(legacy_event_redirect))
}

/// Where a retired `/foundation/nebula/{category}/{slug}{rest}` URL now lives.
///
/// `show-and-tell` is the Foundation's and keeps its prefix; `presentations`
/// and `workshops` are the firm's and sit at the site root. Every destination
/// is relative — one host serves all three.
fn relocated_material(category: &str, slug: &str, rest: &str) -> String {
    if category == "show-and-tell" {
        return format!("/foundation/show-and-tell/{slug}{rest}");
    }
    format!("/{category}/{slug}{rest}")
}

async fn legacy_events_redirect() -> impl IntoResponse {
    axum::response::Redirect::permanent(webapp::show_tell_index::SHOW_TELL_INDEX_PATH)
}

async fn legacy_event_redirect(
    State(events): State<EventIndex>,
    AxumPath(slug): AxumPath<String>,
) -> impl IntoResponse {
    match legacy_event_destination(&events, &slug) {
        Some(destination) => axum::response::Redirect::permanent(&destination).into_response(),
        None => (StatusCode::NOT_FOUND, webapp::error_pages::not_found()).into_response(),
    }
}

fn legacy_event_destination(events: &EventIndex, slug: &str) -> Option<String> {
    events
        .get_public(slug)
        .or_else(|| events.get(slug))
        .map(|event| {
            format!(
                "{}/{}",
                webapp::show_tell_index::SHOW_TELL_INDEX_PATH,
                event.public_slug
            )
        })
}
