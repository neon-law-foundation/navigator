//! `/portal` — role-aware landing.
//!
//! One entry point for every authenticated person. `Role` decides the
//! tier; participation decides the per-project scope. The fan-out:
//!
//! - [`Role::Admin`] → `303` to `/portal/admin` (the firm-wide
//!   dashboard).
//! - [`Role::Client`] with exactly one matter → `303` to
//!   `/portal/projects/:id`. The most common shape for a client (one
//!   open matter) gets them straight to the page that matters.
//! - [`Role::Staff`] / [`Role::Client`] otherwise → `200` listing the
//!   projects visible per [`crate::access::visible_projects`]. An
//!   empty list renders an empty-state message rather than a bare
//!   table.
//!
//! Anonymous callers are bounced to `/auth/login` regardless of which
//! middleware deny them first: in prod, OPA's `/portal/*` rule fires;
//! in tests with [`crate::policy::PolicyClient::passthrough`] the
//! handler does it itself. Both paths land on the same redirect.
//!
//! See [`docs/access-model.md`](../../../docs/access-model.md).

pub mod projects;

use axum::extract::{Extension, State};
use axum::middleware;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;

use store::entity::person::Role;
use store::Db;
use views::pages::portal as portal_views;

use crate::access::visible_projects;
use crate::auth::{require_auth, AuthConfig};
use crate::session::SessionData;

/// Per-router state for `/portal`. Only the database handle is needed
/// today; further dependencies (e.g. workflow runtime) join here as
/// PR 2/3 move surfaces in.
#[derive(Clone)]
pub struct PortalState {
    pub db: Db,
}

impl axum::extract::FromRef<PortalState> for Db {
    fn from_ref(s: &PortalState) -> Self {
        s.db.clone()
    }
}

/// Build the `/portal` sub-router. Mirrors the `admin::routes`
/// layer stack so `SessionData` is in request extensions, OPA gets a
/// chance to deny, and form POSTs are CSRF-checked.
pub fn routes(
    state: PortalState,
    auth: AuthConfig,
    sessions: crate::SessionStore,
    policy: crate::policy::PolicyClient,
) -> Router {
    Router::new()
        .route("/portal", get(landing))
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            sessions.clone(),
            crate::csrf::require_csrf,
        ))
        .route_layer(middleware::from_fn_with_state(
            (sessions, policy),
            crate::policy::require_policy,
        ))
        .route_layer(middleware::from_fn_with_state(auth, require_auth))
}

async fn landing(State(db): State<Db>, session: Option<Extension<SessionData>>) -> Response {
    let Some(Extension(session)) = session else {
        return Redirect::to("/auth/login?return_to=/portal").into_response();
    };
    if session.role == Role::Admin {
        return Redirect::to("/portal/admin").into_response();
    }
    let projects = visible_projects(&db, session.person_id, session.role)
        .await
        .unwrap_or_default();
    // Client with exactly one matter — skip the list, land them on
    // the matter. Staff stay on the list even at N=1 because they
    // routinely have more than one in flight; the list IS their
    // staff dashboard.
    if session.role == Role::Client {
        if let [only] = projects.as_slice() {
            return Redirect::to(&format!("/portal/projects/{}", only.id)).into_response();
        }
    }
    let rows: Vec<portal_views::project_list::ProjectRow<'_>> = projects
        .iter()
        .map(|p| portal_views::project_list::ProjectRow {
            id: p.id,
            name: &p.name,
            status: &p.status,
        })
        .collect();
    portal_views::project_list::render(&rows).into_response()
}
