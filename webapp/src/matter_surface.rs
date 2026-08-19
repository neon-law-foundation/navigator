//! The one matter surface: `/app/projects` and `/app/projects/{code}`.
//!
//! These two components are *dispatchers*, not pages. Each resolves who the
//! caller is to this matter and then renders the page for that relationship.
//! The lens is a fact about the person, never about the URL they typed — a path
//! is chosen by the requester, so it can never be an authorization input.
//!
//! The matter page has five renderings, keyed on
//! [`store::access::MatterViewer`] rather than on the tier alone, because the
//! accountable participant on each side is a different reader from an ordinary
//! one:
//!
//! | Viewer | Page |
//! | --- | --- |
//! | `Client` | service, invoice, agreements, documents |
//! | `ClientDri` | the client page plus the controls only the accountable client may fire |
//! | `Clerk` | name, status, supervising lawyer — nothing else |
//! | `Lawyer` | the firm workbench |
//! | `LawyerDri` | the workbench plus the matter-level accountability actions |
//!
//! **The Clerk branch is the load-bearing one.** A Clerk is a supervised
//! non-lawyer, and `docs/access-model.md` is explicit that they never reach
//! documents, legal work, or a write. Falling a Clerk through to the client
//! page would hand them the client\'s document list, so the branch is a real
//! third rendering rather than a narrowed client view.
//!
//! Rendering each page unchanged is also what keeps module toggle-blindness
//! structural: the client branch emits exactly the markup the client page has
//! always emitted, so a module the firm has not enabled has no row for any
//! query to return and no section that renders empty.
//!
//! A viewer that cannot be resolved renders nothing. The access decision is
//! made in the same call (`matter_viewer` returns `None`), so there is no
//! window in which a page renders for a caller who was never admitted.

use dioxus::prelude::*;

use crate::people::ViewerRole;

/// The caller\'s relationship to the matter in the request path, in a
/// wasm-safe shape. Mirrors `store::access::MatterViewer`; the store type does
/// not cross to the client because it is built behind the `server` feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MatterViewerKind {
    None,
    Client,
    ClientDri,
    Clerk,
    Lawyer,
    LawyerDri,
}

/// Commit the `404` for a caller who is nobody to this matter, so the status
/// matches the page rather than a `200` carrying a not-found body.
#[cfg(feature = "server")]
fn commit_not_found() {
    dioxus_fullstack_core::FullstackContext::commit_http_status(
        axum::http::StatusCode::NOT_FOUND,
        None,
    );
}

/// Resolve the caller\'s relationship to the matter named in the request path.
#[server]
pub async fn matter_viewer_kind() -> Result<MatterViewerKind, ServerFnError> {
    use store::access::MatterViewer;

    let role = dioxus_fullstack_core::FullstackContext::extract::<axum::Extension<ViewerRole>, _>()
        .await
        .map(|axum::Extension(role)| role)
        .unwrap_or_default();
    let person_id = dioxus_fullstack_core::FullstackContext::extract::<
        axum::Extension<crate::portal_project_list::PersonId>,
        _,
    >()
    .await
    .ok()
    .and_then(|axum::Extension(pid)| pid.0)
    .and_then(|raw| raw.parse::<uuid::Uuid>().ok());
    let Ok(axum::extract::Path(code)) =
        dioxus_fullstack_core::FullstackContext::extract::<axum::extract::Path<String>, _>().await
    else {
        commit_not_found();
        return Ok(MatterViewerKind::None);
    };

    let store_role = match role {
        ViewerRole::Owner => store::persons::Role::Owner,
        ViewerRole::Admin => store::persons::Role::Admin,
        ViewerRole::Lawyer => store::persons::Role::Lawyer,
        ViewerRole::Clerk => store::persons::Role::Clerk,
        ViewerRole::Client => store::persons::Role::Client,
    };
    let surreal = consume_context::<store::surreal::SurrealDb>();
    let Some(project) = store::projects::find_by_code(&surreal, &code)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
    else {
        commit_not_found();
        return Ok(MatterViewerKind::None);
    };
    let viewer = store::access::matter_viewer(&surreal, person_id, store_role, project.id)
        .await
        .map_err(|e| ServerFnError::new(e.clone()))?;
    Ok(match viewer {
        None => {
            commit_not_found();
            MatterViewerKind::None
        }
        Some(MatterViewer::Client) => MatterViewerKind::Client,
        Some(MatterViewer::ClientDri) => MatterViewerKind::ClientDri,
        Some(MatterViewer::Clerk) => MatterViewerKind::Clerk,
        Some(MatterViewer::Lawyer) => MatterViewerKind::Lawyer,
        Some(MatterViewer::LawyerDri) => MatterViewerKind::LawyerDri,
    })
}

/// Which of the three matter lists the caller reads.
///
/// A Clerk is not a narrower client here: their list is the *supervised* set,
/// which is its own query and its own page. Falling a Clerk through to the
/// client dashboard would show them a client\'s services and invoices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MatterListKind {
    Client,
    Clerk,
    Firm,
}

/// Which matter list this caller reads, from their tier.
#[server]
pub async fn matter_list_kind() -> Result<MatterListKind, ServerFnError> {
    let role = dioxus_fullstack_core::FullstackContext::extract::<axum::Extension<ViewerRole>, _>()
        .await
        .map(|axum::Extension(role)| role)
        .unwrap_or_default();
    Ok(match role {
        ViewerRole::Owner | ViewerRole::Admin | ViewerRole::Lawyer => MatterListKind::Firm,
        ViewerRole::Clerk => MatterListKind::Clerk,
        ViewerRole::Client => MatterListKind::Client,
    })
}

/// `/app/projects` — the matter list, through the caller\'s own lens.
#[component]
pub fn Projects() -> Element {
    let resource = use_server_future(matter_list_kind)?;
    // An unresolved tier reads as a client: the narrowest of the three, and
    // each list re-scopes its own rows anyway.
    let kind = match &*resource.read() {
        Some(Ok(kind)) => *kind,
        _ => MatterListKind::Client,
    };
    rsx! {
        match kind {
            MatterListKind::Firm => rsx! { crate::project_list::LawyerProjects {} },
            MatterListKind::Clerk => rsx! { crate::clerk::ClerkProjects {} },
            MatterListKind::Client => rsx! { crate::portal_project_list::ClientProjects {} },
        }
    }
}

/// `/app/projects/{code}` — the matter, through the caller\'s own relationship
/// to it.
#[component]
pub fn ProjectDetail() -> Element {
    let resource = use_server_future(matter_viewer_kind)?;
    // The loader answers with a viewer kind, not a view, so the not-found
    // title has nowhere to ride along; it asks the brand seam directly.
    let firm = use_server_future(crate::app_chrome::firm_name)?;
    let firm_name = match &*firm.read() {
        Some(Ok(name)) => name.clone(),
        _ => String::new(),
    };
    let kind = match &*resource.read() {
        Some(Ok(kind)) => *kind,
        // A failed resolution is not a reason to render something; it is a
        // reason to render nothing.
        _ => MatterViewerKind::None,
    };
    rsx! {
        match kind {
            MatterViewerKind::Lawyer | MatterViewerKind::LawyerDri => rsx! {
                crate::lawyer_project_detail::LawyerProjectDetail {}
            },
            MatterViewerKind::Client | MatterViewerKind::ClientDri => rsx! {
                crate::portal_project_detail::ClientProjectDetail {}
            },
            MatterViewerKind::Clerk => rsx! {
                crate::clerk::ClerkProjectDetail {}
            },
            // Nobody to this matter. The `404` status is committed by the
            // loader; a matter that does not exist and one the caller may not
            // see are deliberately the same response.
            MatterViewerKind::None => rsx! {
                main { id: "matter-not-found", class: "nav-theme",
                    document::Title { "{firm_name} | Not found" }
                    h1 { "Not found" }
                    p { "No matter is available at this address." }
                }
            },
        }
    }
}
