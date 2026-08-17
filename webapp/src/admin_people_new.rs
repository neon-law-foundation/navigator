//! The "add person" create form as a Dioxus component (#641 Phase 3, admin
//! cluster) — served on two surfaces: the admin console (`/admin/people/new`,
//! Owner/Admin-only, role unlocked) and the lawyer mirror (`/lawyer/people/new`,
//! role locked for a non-admin-tier caller).
//!
//! The successor to the `admin_people_new` / `people_new` GET renders. It
//! reads the injected CSRF token and any `?error=` flash, and renders the shared
//! [`crate::components::FormCard`] as a native `POST` to the surface's create
//! route — a native form route that wraps the person create command (the
//! form posted to the REST `/app/api/people` over HTMX; the Dioxus form uses a plain
//! form, no JavaScript). On a rejected create the handler redirects back here
//! with `?error=`, surfaced above the form. A non-admin caller's role select is
//! disabled (and the command coerces a submitted role to `client`), so the
//! Lawyer surface can't create an elevated account.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{Choice, Field, FormCard};
use crate::people::ViewerRole;

/// The create form's `?error=` flash (set by the create handler's
/// redirect-on-failure).
#[derive(Deserialize, Default)]
pub struct PeopleNewQuery {
    #[serde(default)]
    pub error: Option<String>,
}

/// The rendered "add person" form: the session CSRF token, an optional error
/// flash, the viewer's tier, whether the role select is locked, and the
/// surface's create/list paths.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct PeopleNewView {
    pub csrf_token: String,
    pub error: Option<String>,
    pub role: ViewerRole,
    /// The role select is disabled — a non-admin-tier caller can only create a
    /// `client`, so the form must not invite a role the command would drop.
    pub role_locked: bool,
    /// The route the native create form posts to (`/admin/people` on the admin
    /// console, `/lawyer/people` on the lawyer mirror).
    pub create_path: String,
    /// The list path Cancel returns to.
    pub list_path: String,
    /// The surface segment for the document title (`Admin | People` /
    /// `Lawyer | People`).
    pub surface_title: String,
    /// The deploy's firm name, for the document title. Resolved from the
    /// request-scoped branding rather than written into the copy, so a
    /// white-label deploy's tab reads its own name.
    #[serde(default)]
    pub firm_name: String,
}

/// Read the injected CSRF token and the `?error=` flash — the shared prelude for
/// both surfaces' server functions.
#[cfg(feature = "server")]
async fn people_new_context() -> (String, Option<String>) {
    let csrf_token = dioxus_fullstack_core::FullstackContext::extract::<
        axum::Extension<crate::csrf::CsrfToken>,
        _,
    >()
    .await
    .map(|axum::Extension(token)| token.0)
    .unwrap_or_default();
    let error = dioxus_fullstack_core::FullstackContext::extract::<
        axum::extract::Query<PeopleNewQuery>,
        _,
    >()
    .await
    .ok()
    .and_then(|axum::extract::Query(q)| q.error);
    (csrf_token, error)
}

/// Load the **admin console** "add person" form (`/admin/people/new`): refuse
/// non-admin-tier callers; the role select is unlocked for Owner and Admin.
#[server]
pub async fn get_admin_people_new_form() -> Result<PeopleNewView, ServerFnError> {
    let role = crate::admin_listing::require_admin().await?;
    let (csrf_token, error) = people_new_context().await;
    Ok(PeopleNewView {
        firm_name: crate::app_chrome::firm_name_from_context().await,
        csrf_token,
        error,
        role,
        role_locked: !role.is_admin_tier(),
        create_path: "/admin/people".to_string(),
        list_path: "/admin/people".to_string(),
        surface_title: "Admin | People".to_string(),
    })
}

/// Load the **lawyer mirror** "add person" form (`/lawyer/people/new`): refuse
/// non-lawyer; the role select is locked for a non-admin-tier caller (the create
/// command coerces their submitted role to `client`).
#[server]
pub async fn get_lawyer_people_new_form() -> Result<PeopleNewView, ServerFnError> {
    let role = crate::admin_listing::require_lawyer().await?;
    let (csrf_token, error) = people_new_context().await;
    Ok(PeopleNewView {
        firm_name: crate::app_chrome::firm_name_from_context().await,
        csrf_token,
        error,
        role,
        role_locked: !role.is_admin_tier(),
        create_path: "/lawyer/people".to_string(),
        list_path: "/lawyer/people".to_string(),
        surface_title: "Lawyer | People".to_string(),
    })
}

/// The admin console "add person" form. Resolves the admin server function and
/// renders through the shared [`render_people_new`].
#[component]
pub fn AdminPeopleNew() -> Element {
    let resource = use_server_future(get_admin_people_new_form)?;
    render_people_new(&resource)
}

/// The lawyer mirror "add person" form (`/lawyer/people/new`) — the same page with
/// a locked role select for a non-admin caller.
#[component]
pub fn LawyerPeopleNew() -> Element {
    let resource = use_server_future(get_lawyer_people_new_form)?;
    render_people_new(&resource)
}

/// Render the resolved "add person" form for either surface: a native `POST` to
/// the surface's create route carrying the CSRF token, with the name / email /
/// role controls (the role select disabled when the caller cannot set roles).
fn render_people_new(resource: &Resource<Result<PeopleNewView, ServerFnError>>) -> Element {
    let view = match &*resource.read() {
        Some(Ok(view)) => view.clone(),
        Some(Err(_)) => {
            return rsx! {
                main { id: "people-new", p { "Failed to load the form." } }
            }
        }
        None => {
            return rsx! {
                main { id: "people-new", p { "Loading…" } }
            }
        }
    };

    let role = view.role;
    let title = format!("{} | {} | Add person", view.firm_name, view.surface_title);
    let mut role_options = Vec::new();
    if role == ViewerRole::Owner {
        role_options.push(Choice::new("owner", "Owner"));
    }
    role_options.extend([
        Choice::new("admin", "Admin"),
        Choice::new("lawyer", "Lawyer (lawyer)"),
        Choice::new("clerk", "Clerk (non-lawyer)"),
        Choice::new("client", "Client"),
    ]);
    let mut role_field = Field::select("Role", "role", role_options, Some("client".to_string()));
    if view.role_locked {
        role_field = role_field
            .disabled()
            .help("Only an Owner or Admin can set a person's role; this creates a client.");
    }
    let fields = vec![
        Field::text("Name", "name", "").required(),
        Field::input("Email", "email", "", "email").required(),
        role_field,
    ];

    rsx! {
        document::Title { "{title}" }
        document::Stylesheet { href: crate::components::THEME_STYLESHEET_HREF }
        nav { class: "lawyer-nav",
            a { class: "nav-link", href: "/app/projects", "Projects" }
            a { class: "nav-link", href: "/auth/logout", "Sign out" }
        }
        main { id: "people-new", class: "nav-theme",
            if let Some(error) = view.error.as_ref() {
                p { class: "nav-form-error", role: "alert", "{error}" }
            }
            FormCard {
                title: "Add person".to_string(),
                action: "{view.create_path}",
                submit_label: "Create person".to_string(),
                csrf_token: Some(view.csrf_token.clone()),
                fields,
            }
            p { a { href: "{view.list_path}", "← Cancel" } }
        }
    }
}
