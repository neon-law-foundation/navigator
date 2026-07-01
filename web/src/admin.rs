//! Admin routes — gated by [`auth::require_auth`].
//!
//! Each sub-page (dashboard, people, …) is a `Router` attached to
//! the same auth layer. New admin surfaces add another `.route(...)`
//! and inherit auth automatically.

use std::sync::Arc;
use uuid::Uuid;

use axum::extract::{Extension, FromRef, Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};

use crate::session::SessionData;

/// Pull the per-session CSRF token from the request's optional
/// `SessionData` extension, returning an empty string when no
/// session is attached.
pub(crate) fn csrf_token(session: Option<&SessionData>) -> &str {
    session.map_or("", |s| s.csrf_token.as_str())
}

/// Pick the appropriate response for a row-delete handler depending
/// on whether the request came from htmx or a plain form submit.
///
/// - htmx (presence of the `HX-Request: true` header) gets an empty
///   `200 OK` so `hx-swap="outerHTML"` replaces the parent `<tr>`
///   with nothing — the row vanishes in place.
/// - everything else gets the historical `303 See Other` redirect
///   back to the list index, matching browser-without-JS behavior.
fn delete_response(headers: &axum::http::HeaderMap, redirect_to: &'static str) -> Response {
    let is_htmx = headers
        .get("HX-Request")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if is_htmx {
        axum::http::StatusCode::OK.into_response()
    } else {
        Redirect::to(redirect_to).into_response()
    }
}

/// HTMX-aware response for a delete that **failed** (most often a
/// foreign-key block — the row still has dependent records). The delete
/// button swaps `closest tr` with the response on success; here we instead
/// retarget the swap to `body` / `beforeend` so a **red toast** is appended
/// and the row is **not** removed — the opposite of the old silent
/// `let _ = …` which made a failed delete look like it worked until a
/// refresh. Non-HTMX callers get a plain redirect back to the listing (where
/// the row is still present).
fn delete_error_toast(
    headers: &axum::http::HeaderMap,
    message: &str,
    redirect_to: &'static str,
) -> Response {
    let is_htmx = headers
        .get("HX-Request")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if is_htmx {
        let toast =
            views::components::toast_overlay(&views::components::Toast::danger(message).render());
        (
            axum::http::StatusCode::OK,
            [("HX-Retarget", "body"), ("HX-Reswap", "beforeend")],
            toast,
        )
            .into_response()
    } else {
        Redirect::to(redirect_to).into_response()
    }
}
use maud::Markup;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};
use serde::Deserialize;

use crate::auth::{require_auth, AuthConfig};
use crate::signature::SignatureProvider;
use store::entity::{
    address, answer, blob, disclosure, document, entity, entity_billing_profile, entity_type,
    git_repository, invoice, invoice_line_item, jurisdiction, letter, mailroom, notation, person,
    person_entity_role, person_project_role, project, question, relationship_log, sent_email,
    share_issuance, template,
};
use store::Db;
use views::pages::admin as admin_views;

/// Per-router state for the admin sub-tree. Wraps the database
/// handle plus the durable-workflow + signature-provider seams that
/// the retainer-intake flow needs. Existing handlers continue to
/// extract `State<Db>` via [`FromRef`].
#[derive(Clone)]
pub struct AdminState {
    pub db: Db,
    pub workflow_runtime: Arc<dyn workflows::StateMachineRuntime>,
    pub signature_provider: Arc<dyn SignatureProvider>,
    /// Parsed questionnaire spec from the bundled retainer
    /// template. Drives the per-step walker at
    /// `/portal/admin/notations/{id}/step`.
    pub retainer_intake_questionnaire: workflows::QuestionnaireSpec,
    /// Same `Arc` as `workflow_runtime` — kept as a separate field so
    /// the questionnaire walker reads from a name that matches the
    /// timeline it drives.
    pub questionnaire_runtime: Arc<dyn workflows::StateMachineRuntime>,
    /// Object storage seam — the retainer workflow's
    /// `document_open__retainer_pdf` step writes the rendered PDF here.
    pub storage: Arc<dyn cloud::StorageService>,
    /// Outbound email backend — same `Arc` as `AppState.email`,
    /// passed through so the admin "Send welcome" handler reaches
    /// the audited [`crate::email::LoggingEmail`] decorator.
    pub email: Arc<dyn crate::email::EmailService>,
    /// Accounting seam — raises the flat matter-close fee when the firm
    /// signs the closing letter (see
    /// [`crate::retainer_walk::raise_matter_close_fee`]). Same `Arc` as
    /// `AppState.billing_provider`.
    pub billing_provider: Arc<dyn billing::BillingProvider>,
    /// Inbound-contract deviation reviewer (same `Arc` as
    /// `AppState.contract_reviewer`). The `analysis__contract_deviations`
    /// step runs this web-side to flag a contract against the client
    /// Entity's playbook. See [`crate::contract_review_walk`].
    pub contract_reviewer: Arc<dyn crate::contract_review::ContractReviewer>,
    /// Email of the "always-admin" operator (see
    /// [`crate::oauth::AuthState::bootstrap_admin_email`]). The role
    /// editor uses this to lock the `role` field on that one row
    /// so an admin can't accidentally demote themselves from the UI.
    /// `None` disables the lock — every row's role becomes freely
    /// editable.
    pub bootstrap_admin_email: Option<String>,
}

impl FromRef<AdminState> for Db {
    fn from_ref(s: &AdminState) -> Self {
        s.db.clone()
    }
}

impl FromRef<AdminState> for Arc<dyn crate::email::EmailService> {
    fn from_ref(s: &AdminState) -> Self {
        s.email.clone()
    }
}

impl FromRef<AdminState> for Arc<dyn cloud::StorageService> {
    fn from_ref(s: &AdminState) -> Self {
        s.storage.clone()
    }
}

/// Build the admin sub-router. The caller merges it into the main
/// router. Auth is applied here via `route_layer` so a missing or
/// invalid token fails before the handler runs. The `sessions`
/// store backs the CSRF middleware that gates every form-encoded
/// state-changing request.
#[allow(clippy::too_many_lines)]
pub fn routes(
    state: AdminState,
    auth: AuthConfig,
    sessions: crate::SessionStore,
    policy: crate::policy::PolicyClient,
) -> Router {
    // PR 4: `/portal/*` is the only URL space. Firm-wide CRUD lives
    // under `/portal/admin/*` (staff/admin via OPA's `/portal/admin`
    // rule); project routes live under `/portal/projects/*` and are
    // role-aware at the handler level — clients see the lightweight
    // detail and get `404` on every write URL.
    let mut r = Router::new();
    r = register_firm_routes(r, "/portal/admin");
    r = register_project_routes(r, "/portal/projects");
    // Blank government forms — any authenticated person (OPA's
    // `/portal/forms` rule); the bytes come from the bundled `forms`
    // registry, the same canonical examples the workflows fill.
    r = r
        .route("/portal/forms", get(crate::gov_forms::index_get))
        .route("/portal/forms/{file}", get(crate::gov_forms::download_get));
    let bearer_sessions = sessions.clone();
    r.with_state(state)
        .layer(middleware::from_fn_with_state(
            sessions.clone(),
            crate::csrf::require_csrf,
        ))
        // Policy check runs after bearer-token + session decode so
        // rego rules can read `input.session.role`. A deny short-
        // circuits with 403 — the CSRF layer below it never runs.
        .route_layer(middleware::from_fn_with_state(
            (sessions, policy),
            crate::policy::require_policy,
        ))
        .route_layer(middleware::from_fn_with_state(auth, require_auth))
        // Outermost: resolve a `navigator` CLI bearer credential (the
        // same `SessionData` blob the cookie carries) into a
        // `SessionData` + `AuthClaims` extension, so the CLI drives every
        // `/portal` handler over the same path the browser does. Sits
        // outside `require_auth` so the JWT layer short-circuits on the
        // injected `AuthClaims` instead of rejecting a session blob.
        .route_layer(middleware::from_fn_with_state(
            bearer_sessions,
            crate::auth::inject_bearer_session,
        ))
}

/// Register the firm-wide CRUD routes under `{prefix}/...`. Today
/// this is called once with `/portal/admin`; the helper survives as
/// a single point of edit for the firm CRUD surface.
#[allow(clippy::too_many_lines)]
fn register_firm_routes(r: Router<AdminState>, prefix: &str) -> Router<AdminState> {
    r.route(prefix, get(dashboard))
        .route(
            &format!("{prefix}/retainers/new"),
            get(crate::retainer_walk::start_get).post(crate::retainer_walk::start_post),
        )
        .route(
            &format!("{prefix}/notations/{{id}}/step"),
            get(crate::retainer_walk::step_get).post(crate::retainer_walk::step_post),
        )
        // Hand the matter's client their self-serve intake link.
        .route(
            &format!("{prefix}/notations/{{id}}/send-intake"),
            post(crate::retainer_walk::send_intake_post),
        )
        // Attorney approves a notation parked at staff_review (it carries
        // custom content): fires `approved` so the worker renders + persists
        // the reviewed bytes, then parks at `document_open__retainer_pdf`.
        .route(
            &format!("{prefix}/notations/{{id}}/approve-send"),
            post(crate::retainer_walk::approve_send_post),
        )
        // The deliberate send half: confirms the worker's PDF landed, then
        // dispatches exactly one envelope. 409 + JSON reason when not ready.
        .route(
            &format!("{prefix}/notations/{{id}}/send"),
            post(crate::retainer_walk::send_post),
        )
        // Review/approve screen for a notation parked at staff_review —
        // where the matter-open form lands staff after opening a matter
        // with a retainer.
        .route(
            &format!("{prefix}/notations/{{id}}/review"),
            get(crate::retainer_walk::review_get),
        )
        // Northstar: the attorney releases the generated estate drafts to
        // the client — advances staff_review → client_review and flips each
        // draft to pending_review (visible on the Phase A review surface).
        .route(
            &format!("{prefix}/notations/{{id}}/release-drafts"),
            post(crate::estate::release_drafts_post),
        )
        // Per-notation custom clauses spliced into the assembled document.
        .route(
            &format!("{prefix}/notations/{{id}}/clauses"),
            get(crate::clauses::clauses_page).post(crate::clauses::clause_add),
        )
        .route(
            &format!("{prefix}/notations/{{id}}/clauses/{{cid}}/edit"),
            post(crate::clauses::clause_edit),
        )
        .route(
            &format!("{prefix}/notations/{{id}}/clauses/{{cid}}/delete"),
            post(crate::clauses::clause_delete),
        )
        .route(
            &format!("{prefix}/notations/{{id}}/clauses/{{cid}}/move"),
            post(crate::clauses::clause_move),
        )
        .route(
            &format!("{prefix}/notations/{{id}}/sign"),
            get(crate::esign_view::sign_get),
        )
        .route(
            &format!("{prefix}/projects/{{id}}/close"),
            post(crate::retainer_walk::close_matter_post),
        )
        .route(
            &format!("{prefix}/notations/{{id}}/documents/{{doc_id}}"),
            get(crate::documents::download),
        )
        // Admin-only governed expunge of a filed document — drives the
        // history-rewrite + storage-delete + audit primitive. The
        // handler 404s any non-admin session.
        .route(
            &format!("{prefix}/documents/{{doc_id}}/expunge"),
            get(crate::expunge_route::confirm).post(crate::expunge_route::run),
        )
        // Client document-deletion requests: a staff/admin queue, with
        // admin-only authorize (runs the expunge) + staff/admin deny.
        .route(
            &format!("{prefix}/expunge-requests"),
            get(crate::expunge_request_route::admin_queue),
        )
        .route(
            &format!("{prefix}/expunge-requests/{{id}}/authorize"),
            post(crate::expunge_request_route::admin_authorize),
        )
        .route(
            &format!("{prefix}/expunge-requests/{{id}}/deny"),
            post(crate::expunge_request_route::admin_deny),
        )
        .route(
            &format!("{prefix}/people"),
            get(people_index).post(people_create),
        )
        .route(&format!("{prefix}/people.csv"), get(people_csv))
        .route(&format!("{prefix}/people/new"), get(people_new))
        .route(
            &format!("{prefix}/people/{{id}}"),
            get(people_edit).post(people_update),
        )
        .route(&format!("{prefix}/people/{{id}}/edit"), get(people_edit))
        .route(
            &format!("{prefix}/people/{{id}}/delete"),
            post(people_delete),
        )
        .route(
            &format!("{prefix}/people/{{id}}/welcome"),
            post(people_send_welcome),
        )
        .route(
            &format!("{prefix}/entities"),
            get(entities_index).post(entities_create),
        )
        .route(&format!("{prefix}/entities.csv"), get(entities_csv))
        .route(&format!("{prefix}/entities/new"), get(entities_new))
        .route(
            &format!("{prefix}/entities/{{id}}"),
            get(entities_edit).post(entities_update),
        )
        .route(
            &format!("{prefix}/entities/{{id}}/edit"),
            get(entities_edit),
        )
        .route(
            &format!("{prefix}/entities/{{id}}/delete"),
            post(entities_delete),
        )
        .route(
            &format!("{prefix}/entities/{{id}}/cap-table"),
            get(entity_cap_table),
        )
        // Inbound-contract-review playbooks: a Company's negotiating
        // positions, the yardstick the deviation analysis measures a
        // third-party contract against.
        .route(
            &format!("{prefix}/playbooks"),
            get(crate::admin_playbooks::index).post(crate::admin_playbooks::create),
        )
        .route(
            &format!("{prefix}/playbooks/new"),
            get(crate::admin_playbooks::new_form),
        )
        .route(
            &format!("{prefix}/playbooks/{{id}}"),
            post(crate::admin_playbooks::update),
        )
        .route(
            &format!("{prefix}/playbooks/{{id}}/edit"),
            get(crate::admin_playbooks::edit_form),
        )
        // Attorney review screen for an inbound contract review: act on
        // each finding, edit the risk summary, then approve (assemble +
        // deliver the memo) or reject. Row-scoped to the matter in the
        // handlers.
        .route(
            &format!("{prefix}/contract-reviews/{{id}}"),
            get(crate::admin_contract_reviews::show),
        )
        .route(
            &format!("{prefix}/contract-reviews/{{id}}/findings/{{idx}}"),
            post(crate::admin_contract_reviews::save_finding),
        )
        .route(
            &format!("{prefix}/contract-reviews/{{id}}/summary"),
            post(crate::admin_contract_reviews::save_summary),
        )
        .route(
            &format!("{prefix}/contract-reviews/{{id}}/approve"),
            post(crate::admin_contract_reviews::approve),
        )
        .route(
            &format!("{prefix}/contract-reviews/{{id}}/reject"),
            post(crate::admin_contract_reviews::reject),
        )
        // Read-only listings — these tables are seeded by the
        // workspace (`cli import`, `store/seeds/`) rather than
        // authored from the web UI.
        .route(&format!("{prefix}/entity-types"), get(entity_types_index))
        .route(&format!("{prefix}/templates"), get(templates_index))
        .route(&format!("{prefix}/questions"), get(questions_index))
        // Manual "Run nightly export now" — fires the same `Archives`
        // Restate workflow as the nightly CronJob, for post-deploy
        // testing and missed-night recovery.
        .route(
            &format!("{prefix}/archives/run"),
            post(crate::archives::run),
        )
        // Cron-schedule reference: the deployed CronJobs + their
        // schedules, with an inline "Run now" for jobs that expose one.
        .route(&format!("{prefix}/schedules"), get(cron_schedules))
        // Recurring-billing admin: open a subscription (pending until its
        // retainer is signed) and mint reusable discount coupons. Both
        // branch to JSON on `?format=json` for the `navigator` CLI.
        .route(
            &format!("{prefix}/subscriptions"),
            get(crate::billing_admin::subscriptions_index)
                .post(crate::billing_admin::subscriptions_create),
        )
        .route(
            &format!("{prefix}/coupons"),
            get(crate::billing_admin::coupons_index).post(crate::billing_admin::coupons_create),
        )
        .route(&format!("{prefix}/notations"), get(notations_list))
        .route(&format!("{prefix}/answers"), get(answers_list))
        .route(&format!("{prefix}/addresses"), get(addresses_list))
        .route(&format!("{prefix}/mailrooms"), get(mailrooms_list))
        .route(&format!("{prefix}/letters"), get(letters_list))
        .route(&format!("{prefix}/letters/{{id}}"), get(letter_detail))
        .route(&format!("{prefix}/email-log"), get(email_log_index))
        .route(&format!("{prefix}/blobs"), get(blobs_list))
        .route(&format!("{prefix}/documents"), get(documents_list))
        .route(
            &format!("{prefix}/person-entity-roles"),
            get(person_entity_roles_list),
        )
        .route(
            &format!("{prefix}/person-project-roles"),
            get(person_project_roles_list),
        )
        .route(
            &format!("{prefix}/entity-billing-profiles"),
            get(entity_billing_profiles_list),
        )
        .route(&format!("{prefix}/invoices"), get(invoices_list))
        .route(
            &format!("{prefix}/invoice-line-items"),
            get(invoice_line_items_list),
        )
        .route(&format!("{prefix}/jurisdictions"), get(jurisdictions_list))
        .route(
            &format!("{prefix}/git-repositories"),
            get(git_repositories_list),
        )
        .route(&format!("{prefix}/disclosures"), get(disclosures_list))
        .route(
            &format!("{prefix}/relationship-logs"),
            get(relationship_logs_list),
        )
}

/// Register the project handlers under `{prefix}/...`. Project rows
/// are row-scoped via [`crate::access::visible_projects`] in the
/// handlers themselves; every write surface gates on `staff_tier` so
/// a client sees `404` on every URL they can't act on. The role
/// branch on `GET /{id}` is what splits the lightweight client view
/// (in [`crate::portal::projects::detail`]) from the admin-chrome
/// view (header + documents + upload — rendered by
/// [`projects_detail`]).
fn register_project_routes(r: Router<AdminState>, prefix: &str) -> Router<AdminState> {
    r.route(prefix, get(projects_index).post(projects_create_staff_only))
        .route(&format!("{prefix}.csv"), get(projects_csv))
        .route(&format!("{prefix}/new"), get(projects_new_staff_only))
        .route(
            &format!("{prefix}/{{id}}"),
            get(projects_detail_role_aware).post(projects_update_staff_only),
        )
        .route(
            &format!("{prefix}/{{id}}/edit"),
            get(projects_edit_staff_only),
        )
        .route(
            &format!("{prefix}/{{id}}/delete"),
            post(projects_delete_staff_only),
        )
        .route(
            &format!("{prefix}/{{id}}/documents/upload"),
            post(crate::project_documents::upload),
        )
        // Northstar: file a sitting's transcript into an estate matter
        // (text / file / link) — threads the reusable document-intake
        // step through the workflow's `transcript_uploaded` signal.
        .route(
            &format!("{prefix}/{{id}}/notations/{{nid}}/transcript"),
            post(crate::transcript_intake::upload),
        )
        // Inbound contract review: a Nexus client (or staff) uploads a
        // third-party contract; opens a `services__contract_review`
        // notation, files the contract, runs the playbook deviation
        // analysis web-side, and lands at `staff_review`. Row-scoped in
        // the handler.
        .route(
            &format!("{prefix}/{{id}}/contract-review"),
            post(crate::contract_review_walk::upload),
        )
        .route(
            &format!("{prefix}/{{id}}/documents/{{doc_id}}"),
            get(crate::project_documents::detail),
        )
        .route(
            &format!("{prefix}/{{id}}/documents/{{doc_id}}/download"),
            get(crate::project_documents::download),
        )
        // Client/staff "download all my documents" — a ZIP of the
        // matter's current files (repo HEAD), row-scoped in the handler.
        .route(
            &format!("{prefix}/{{id}}/documents.zip"),
            get(crate::project_export::download_all),
        )
        // Client-initiated "Delete this document" — records a pending
        // request a staff/admin later authorizes. Request-only.
        .route(
            &format!("{prefix}/{{id}}/documents/{{doc_id}}/request-deletion"),
            post(crate::expunge_request_route::client_request),
        )
        // Northstar: the client approves their estate plan, firing
        // `client_approved` (client_review → sent_for_signature__pending)
        // and flipping every released draft to `approved`. Row + status
        // gated in the handler; OPA allows the path for any authenticated
        // caller.
        .route(
            &format!("{prefix}/{{id}}/approve-plan"),
            post(crate::estate::approve_plan_post),
        )
        // Comment-only client review surface (Northstar Phase A). The
        // page is row-scoped to the matter and only renders drafts an
        // attorney has advanced past `draft`; comments POST back as
        // form-encoded (CSRF-checked) and list as JSON for the viewer.
        .route(
            &format!("{prefix}/{{id}}/review/{{doc_id}}"),
            get(crate::review::review_page),
        )
        .route(
            &format!("{prefix}/{{id}}/review/{{doc_id}}/comments"),
            get(crate::review::list_comments).post(crate::review::create_comment),
        )
        // The matter's single privileged conversation log — document
        // comments, email (both directions), and portal messages interleaved
        // in time. Row-scoped; clients never see firm-internal notes.
        .route(
            &format!("{prefix}/{{id}}/conversation"),
            get(crate::conversation::thread_page),
        )
        .route(
            &format!("{prefix}/{{id}}/conversation/messages"),
            post(crate::conversation::post_message),
        )
        // Client self-serve intake (the magic link): the demand-side
        // mirror of the admin walker. Row-scoped to the matter; the
        // client answers the client-facing questions, source `client`.
        .route(
            &format!("{prefix}/{{id}}/intake/{{notation_id}}"),
            get(crate::intake::intake_page).post(crate::intake::intake_save),
        )
}

/// Returns `true` when the caller can act on the staff/admin write
/// surface. A `Client` session is the only role explicitly denied;
/// no session at all (e.g. tests running with
/// [`crate::policy::PolicyClient::passthrough`] and no cookie) is
/// treated as staff so existing handler-level tests still reach the
/// admin chrome. In production OPA's `/portal/admin/*` rule blocks
/// unauthenticated traffic before the handler runs, and on
/// `/portal/projects/*` the row-scoping in
/// [`crate::access::visible_projects`] is the second line of defense.
pub(crate) fn is_staff_tier(session: Option<&SessionData>) -> bool {
    !matches!(
        session.map(|s| s.role),
        Some(store::entity::person::Role::Client)
    )
}

fn not_found_response() -> Response {
    (StatusCode::NOT_FOUND, views::not_found_page()).into_response()
}

async fn dashboard(State(db): State<Db>, session: Option<Extension<SessionData>>) -> Markup {
    let counts = admin_views::DashboardCounts {
        people: person::Entity::find().count(&db).await.unwrap_or(0),
        entities: entity::Entity::find().count(&db).await.unwrap_or(0),
        jurisdictions: jurisdiction::Entity::find().count(&db).await.unwrap_or(0),
        entity_types: entity_type::Entity::find().count(&db).await.unwrap_or(0),
    };
    admin_views::dashboard(&counts, csrf_token(session.as_deref()))
}

/// `GET /portal/admin/schedules` — the cron-schedule reference page.
async fn cron_schedules(session: Option<Extension<SessionData>>) -> Markup {
    admin_views::schedules::schedules(csrf_token(session.as_deref()))
}

/// JSON:API 1.1 query parameters honored by `/portal/admin/people`.
///
/// `sort` is the comma-separated key list (a leading `-` flips a
/// field to descending). `filter[name]` and `filter[email]` are
/// case-insensitive substring matches. Brackets in the key arrive
/// raw or percent-encoded; serde_urlencoded decodes both forms back
/// to the `filter[name]` rename target.
#[derive(Deserialize, Default, Debug)]
struct PeopleListQuery {
    #[serde(default)]
    sort: Option<String>,
    #[serde(rename = "filter[name]", default)]
    filter_name: Option<String>,
    #[serde(rename = "filter[email]", default)]
    filter_email: Option<String>,
}

async fn people_index(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    axum::extract::Query(q): axum::extract::Query<PeopleListQuery>,
) -> Response {
    use views::components::{SortDirection, SortSpec};
    let token = csrf_token(session.as_deref());
    let allowed: std::collections::HashSet<&str> = ["name", "email"].into_iter().collect();
    let sort = match SortSpec::parse(q.sort.as_deref()).validated(&allowed) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let filter_name = q.filter_name.as_deref().unwrap_or("");
    let filter_email = q.filter_email.as_deref().unwrap_or("");

    let mut query = person::Entity::find();
    if !filter_name.is_empty() {
        query = query.filter(person::Column::Name.contains(filter_name));
    }
    if !filter_email.is_empty() {
        query = query.filter(person::Column::Email.contains(filter_email));
    }
    for field in &sort.fields {
        let col = match field.key.as_str() {
            "name" => person::Column::Name,
            "email" => person::Column::Email,
            // unreachable: `validated` already rejected anything else.
            _ => continue,
        };
        query = match field.direction {
            SortDirection::Ascending => query.order_by_asc(col),
            SortDirection::Descending => query.order_by_desc(col),
        };
    }

    let people = query.all(&db).await.unwrap_or_default();
    let rows: Vec<admin_views::people::PersonRow<'_>> = people
        .iter()
        .map(|p| admin_views::people::PersonRow {
            id: p.id,
            name: &p.name,
            email: &p.email,
        })
        .collect();
    let extra_query: [(&str, &str); 2] = [
        ("filter[name]", filter_name),
        ("filter[email]", filter_email),
    ];
    admin_views::people::list(&rows, token, &sort, &extra_query).into_response()
}

async fn people_new(session: Option<Extension<SessionData>>) -> Markup {
    let token = csrf_token(session.as_deref());
    admin_views::people::new_form(&admin_views::people::PersonForm {
        csrf_token: token,
        ..Default::default()
    })
}

#[derive(Deserialize)]
struct PersonInput {
    name: String,
    email: String,
    /// `client`, `staff`, or `admin`. Missing or unrecognized values
    /// fall back to `client` (the safe default) — the OPA policy is
    /// the second line of defense.
    #[serde(default)]
    role: String,
}

fn parse_role(s: &str) -> store::entity::person::Role {
    use store::entity::person::Role;
    match s.trim() {
        "admin" => Role::Admin,
        "staff" => Role::Staff,
        _ => Role::Client,
    }
}

fn is_bootstrap_admin_email(state_email: Option<&str>, row_email: &str) -> bool {
    matches!(state_email, Some(e) if e.eq_ignore_ascii_case(row_email))
}

async fn people_create(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    Form(input): Form<PersonInput>,
) -> Response {
    let token = csrf_token(session.as_deref());
    if input.name.trim().is_empty() || !input.email.contains('@') {
        return admin_views::people::new_form(&admin_views::people::PersonForm {
            name: &input.name,
            email: &input.email,
            role: &input.role,
            role_locked: false,
            csrf_token: token,
            error: Some("Name is required and email must contain an @."),
            xero_contact_id: None,
        })
        .into_response();
    }
    let role = parse_role(&input.role);
    match (person::ActiveModel {
        name: ActiveValue::Set(input.name),
        email: ActiveValue::Set(input.email),
        role: ActiveValue::Set(role),
        ..Default::default()
    })
    .insert(&db)
    .await
    {
        Ok(_) => Redirect::to("/portal/admin/people").into_response(),
        Err(e) if store::is_unique_violation(&e) => {
            tracing::warn!(error = %e, "admin: create person email conflict");
            (
                StatusCode::CONFLICT,
                admin_views::people::new_form(&admin_views::people::PersonForm {
                    name: "",
                    email: "",
                    role: "",
                    role_locked: false,
                    csrf_token: token,
                    error: Some("Could not create — that email is already in use."),
                    xero_contact_id: None,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "admin: create person failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response()
        }
    }
}

/// Query flags carried back to the person show view after a per-record
/// action redirect. Today only the welcome-email send sets one
/// (`?notice=welcome_sent` / `welcome_failed`), surfaced as a flash toast
/// on arrival — the show-view sibling of the sign-in page's `?notice=`.
#[derive(Deserialize, Default)]
struct PersonShowQuery {
    notice: Option<String>,
}

/// Map the show view's `?notice=` flag to a toned flash toast, personalized
/// with the recipient. `welcome_sent` is green (the success confirmation the
/// staff member asked for); `welcome_failed` is red; any other value (or
/// none) renders no toast, so a plain visit to the page stays clean.
fn welcome_notice(notice: Option<&str>, recipient: &str) -> Option<views::components::Toast> {
    match notice {
        Some("welcome_sent") => Some(views::components::Toast::success(format!(
            "Welcome email sent to {recipient}."
        ))),
        Some("welcome_failed") => Some(views::components::Toast::danger(format!(
            "Couldn't send the welcome email to {recipient}. Check the email log."
        ))),
        _ => None,
    }
}

async fn people_edit(
    State(s): State<AdminState>,
    session: Option<Extension<SessionData>>,
    Path(id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<PersonShowQuery>,
) -> Response {
    let token = csrf_token(session.as_deref());
    match person::Entity::find_by_id(id).one(&s.db).await {
        Ok(Some(p)) => {
            let role_token = p.role.as_str();
            let locked = is_bootstrap_admin_email(s.bootstrap_admin_email.as_deref(), &p.email);
            let notice = welcome_notice(q.notice.as_deref(), &p.email);
            admin_views::people::edit_form(
                p.id,
                &admin_views::people::PersonForm {
                    name: &p.name,
                    email: &p.email,
                    role: role_token,
                    role_locked: locked,
                    csrf_token: token,
                    error: None,
                    xero_contact_id: p.xero_contact_id.as_deref(),
                },
                notice.as_ref(),
            )
            .into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "admin: load person failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response()
        }
    }
}

async fn people_update(
    State(s): State<AdminState>,
    Path(id): Path<Uuid>,
    Form(input): Form<PersonInput>,
) -> Response {
    let existing = match person::Entity::find_by_id(id).one(&s.db).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "admin: load person failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response();
        }
    };
    let is_bootstrap_admin =
        is_bootstrap_admin_email(s.bootstrap_admin_email.as_deref(), &existing.email);
    // Bootstrap admin row: the UI sends back the existing role via the
    // disabled <select>, but a hostile client could rewrite the field.
    // Force-set Admin unconditionally so an accidental demotion can't
    // leak through.
    let new_role = if is_bootstrap_admin {
        store::entity::person::Role::Admin
    } else {
        parse_role(&input.role)
    };
    let mut active: person::ActiveModel = existing.into();
    active.name = ActiveValue::Set(input.name);
    active.email = ActiveValue::Set(input.email);
    active.role = ActiveValue::Set(new_role);
    match active.update(&s.db).await {
        Ok(_) => Redirect::to("/portal/admin/people").into_response(),
        Err(e) if store::is_unique_violation(&e) => {
            tracing::warn!(error = %e, "admin: update person email conflict");
            (
                StatusCode::CONFLICT,
                "That email is already in use by another person.",
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "admin: update person failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response()
        }
    }
}

async fn people_delete(
    State(s): State<AdminState>,
    Path(id): Path<Uuid>,
    headers: axum::http::HeaderMap,
) -> Response {
    // The bootstrap admin person — whatever NAVIGATOR_BOOTSTRAP_ADMIN_EMAIL
    // resolves to — is undeletable. Without this guard a staff member
    // with admin access could wipe the row that
    // `oauth::resolve_person_from_claims` re-tags as bootstrap admin on
    // every login, locking out the role grant on the next session.
    let target = match person::Entity::find_by_id(id).one(&s.db).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "admin: load person for delete failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response();
        }
    };
    if is_bootstrap_admin_email(s.bootstrap_admin_email.as_deref(), &target.email) {
        tracing::warn!(person_id = %id, "admin: blocked attempt to delete bootstrap admin person");
        return (
            StatusCode::CONFLICT,
            "Cannot delete the bootstrap admin person (configured via NAVIGATOR_BOOTSTRAP_ADMIN_EMAIL).",
        )
            .into_response();
    }
    if let Err(e) = person::Entity::delete_by_id(id).exec(&s.db).await {
        tracing::warn!(error = %e, person_id = %id, "people_delete: delete failed");
        return delete_error_toast(
            &headers,
            &format!(
                "Couldn't delete this person — {}.",
                store::db_error::describe_write_failure(&e)
            ),
            "/portal/admin/people",
        );
    }
    delete_response(&headers, "/portal/admin/people")
}

/// POST /portal/admin/people/{id}/welcome — render the welcome email
/// body for this person and dispatch it through the configured
/// `EmailService`. The `LoggingEmail` decorator journals one row to
/// `sent_emails` per attempt; the admin can inspect the result at
/// `/portal/admin/email-log`.
///
/// Errors are logged and surfaced through the redirect's query
/// string rather than as a 5xx so the staff member always lands
/// back on the people index. A future enhancement is a structured
/// flash message in the layout; today we lean on the email log.
/// Per-page row count for the email log. 50 is roughly one screen of
/// scannable rows on a laptop without forcing a horizontal scroll.
const EMAIL_LOG_PER_PAGE: u64 = 50;

#[derive(Deserialize, Default)]
struct EmailLogQuery {
    /// 1-indexed page number. Defaults to 1 when missing or zero.
    page: Option<u64>,
}

/// GET /portal/admin/email-log — read-only paginated view over `sent_emails`.
/// Newest first; metadata only (body intentionally not shown).
async fn email_log_index(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<EmailLogQuery>,
) -> Response {
    let page = q.page.unwrap_or(1).max(1);
    let paginator = sent_email::Entity::find()
        .order_by_desc(sent_email::Column::SentAt)
        .paginate(&db, EMAIL_LOG_PER_PAGE);
    let total_pages = paginator.num_pages().await.unwrap_or(0).max(1);
    let rows_raw = paginator
        .fetch_page(page.saturating_sub(1))
        .await
        .unwrap_or_default();
    let rows: Vec<admin_views::email_log::Row<'_>> = rows_raw
        .iter()
        .map(|r| admin_views::email_log::Row {
            id: r.id,
            recipient: &r.recipient,
            subject: &r.subject,
            sender: &r.sender,
            template_slug: r.template_slug.as_deref(),
            outcome: &r.outcome,
            sent_at: &r.sent_at,
        })
        .collect();
    let pagination = admin_views::email_log::Pagination {
        page,
        per_page: EMAIL_LOG_PER_PAGE,
        total_pages,
    };
    admin_views::email_log::list(&rows, &pagination).into_response()
}

async fn people_send_welcome(State(state): State<AdminState>, Path(id): Path<Uuid>) -> Response {
    let person = match person::Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "admin: load person for welcome failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };
    let body = crate::welcome::render_welcome_body(&person.name, &person.email);
    let base_url = workflows::email::base_url_from_env();
    let html = crate::welcome::render_welcome_html(&person.name, &person.email, &base_url);
    let msg = crate::email::OutboundEmail::new(
        person.email.clone(),
        crate::welcome::welcome_subject(),
        body,
    )
    .with_template("welcome")
    .with_html(html)
    .with_person(id.to_string());
    // Carry the outcome back as a `?notice=` flag so the show view floats a
    // green confirmation (or a red failure) toast on arrival — staff get a
    // clear "the welcome email was sent" signal, not a silent reload.
    let notice = match state.email.send(msg).await {
        Ok(_) => {
            tracing::info!(person_id = %id, recipient = %person.email, "admin: welcome email sent");
            "welcome_sent"
        }
        Err(e) => {
            tracing::warn!(error = %e, person_id = %id, "admin: welcome email send failed");
            "welcome_failed"
        }
    };
    // Back to the person's show view (where the button now lives), not the
    // list — the staff member stays on the record they just acted on.
    Redirect::to(&format!("/portal/admin/people/{id}/edit?notice={notice}")).into_response()
}

// ---- Entities ----

/// JSON:API 1.1 query parameters honored by `/portal/admin/entities`.
/// Today only `sort=` is supported — filters are a follow-up once
/// the schema lands a stable text-search column.
#[derive(Deserialize, Default, Debug)]
struct EntitiesListQuery {
    #[serde(default)]
    sort: Option<String>,
}

async fn entities_index(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    axum::extract::Query(q): axum::extract::Query<EntitiesListQuery>,
) -> Response {
    use views::components::{SortDirection, SortSpec};
    let token = csrf_token(session.as_deref());
    let allowed: std::collections::HashSet<&str> = ["name", "entity_type", "jurisdiction"]
        .into_iter()
        .collect();
    let sort = match SortSpec::parse(q.sort.as_deref()).validated(&allowed) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let types = entity_type::Entity::find()
        .all(&db)
        .await
        .unwrap_or_default();
    let jurs = jurisdiction::Entity::find()
        .all(&db)
        .await
        .unwrap_or_default();

    // `name` lives on `entity`; `entity_type` / `jurisdiction` are
    // foreign-key names resolved client-side after the fetch. Only
    // the `name` sort can be pushed into Postgres; the other two are
    // sorted in-memory below.
    let mut query = entity::Entity::find();
    for field in &sort.fields {
        if field.key == "name" {
            query = match field.direction {
                SortDirection::Ascending => query.order_by_asc(entity::Column::Name),
                SortDirection::Descending => query.order_by_desc(entity::Column::Name),
            };
        }
    }
    let mut rows_raw = query.all(&db).await.unwrap_or_default();

    let by_type = |id: Uuid| {
        types
            .iter()
            .find(|t| t.id == id)
            .map_or("?", |t| t.name.as_str())
    };
    let by_jur = |id: Uuid| {
        jurs.iter()
            .find(|j| j.id == id)
            .map_or("?", |j| j.name.as_str())
    };

    // In-memory sort for the resolved-name columns. Stable so a
    // tie on `entity_type` falls back to the existing order.
    for field in &sort.fields {
        match field.key.as_str() {
            "entity_type" => match field.direction {
                SortDirection::Ascending => {
                    rows_raw
                        .sort_by(|a, b| by_type(a.entity_type_id).cmp(by_type(b.entity_type_id)));
                }
                SortDirection::Descending => {
                    rows_raw
                        .sort_by(|a, b| by_type(b.entity_type_id).cmp(by_type(a.entity_type_id)));
                }
            },
            "jurisdiction" => match field.direction {
                SortDirection::Ascending => {
                    rows_raw
                        .sort_by(|a, b| by_jur(a.jurisdiction_id).cmp(by_jur(b.jurisdiction_id)));
                }
                SortDirection::Descending => {
                    rows_raw
                        .sort_by(|a, b| by_jur(b.jurisdiction_id).cmp(by_jur(a.jurisdiction_id)));
                }
            },
            _ => {}
        }
    }

    let rows: Vec<admin_views::entities::EntityRow<'_>> = rows_raw
        .iter()
        .map(|e| admin_views::entities::EntityRow {
            id: e.id,
            name: &e.name,
            entity_type: by_type(e.entity_type_id),
            jurisdiction: by_jur(e.jurisdiction_id),
        })
        .collect();
    admin_views::entities::list(&rows, token, &sort).into_response()
}

async fn entities_new(State(db): State<Db>) -> Markup {
    let (types, jurs) = load_entity_choices(&db).await;
    let type_choices: Vec<_> = types
        .iter()
        .map(|t| admin_views::entities::TypeChoice {
            id: t.id,
            name: &t.name,
        })
        .collect();
    let jur_choices: Vec<_> = jurs
        .iter()
        .map(|j| admin_views::entities::JurisdictionChoice {
            id: j.id,
            name: &j.name,
            code: &j.code,
        })
        .collect();
    admin_views::entities::new_form(
        &admin_views::entities::EntityForm::default(),
        &type_choices,
        &jur_choices,
    )
}

#[derive(Deserialize)]
struct EntityInput {
    name: String,
    entity_type_id: Uuid,
    jurisdiction_id: Uuid,
}

async fn entities_create(State(db): State<Db>, Form(input): Form<EntityInput>) -> Response {
    if input.name.trim().is_empty() {
        return reload_form_with_error(&db, "Name is required.").await;
    }
    match (entity::ActiveModel {
        name: ActiveValue::Set(input.name),
        entity_type_id: ActiveValue::Set(input.entity_type_id),
        jurisdiction_id: ActiveValue::Set(input.jurisdiction_id),
        ..Default::default()
    })
    .insert(&db)
    .await
    {
        Ok(_) => Redirect::to("/portal/admin/entities").into_response(),
        Err(e) if store::is_unique_violation(&e) => {
            tracing::warn!(error = %e, "admin: create entity uniqueness conflict");
            (
                StatusCode::CONFLICT,
                reload_form_with_error(&db, "An entity with that key already exists.").await,
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!(error = %e, "admin: create entity failed");
            reload_form_with_error(
                &db,
                "Could not create entity (invalid type or jurisdiction?).",
            )
            .await
        }
    }
}

async fn reload_form_with_error(db: &Db, message: &str) -> Response {
    let (types, jurs) = load_entity_choices(db).await;
    let type_choices: Vec<_> = types
        .iter()
        .map(|t| admin_views::entities::TypeChoice {
            id: t.id,
            name: &t.name,
        })
        .collect();
    let jur_choices: Vec<_> = jurs
        .iter()
        .map(|j| admin_views::entities::JurisdictionChoice {
            id: j.id,
            name: &j.name,
            code: &j.code,
        })
        .collect();
    admin_views::entities::new_form(
        &admin_views::entities::EntityForm {
            name: "",
            entity_type_id: None,
            jurisdiction_id: None,
            error: Some(message),
        },
        &type_choices,
        &jur_choices,
    )
    .into_response()
}

async fn entities_edit(State(db): State<Db>, Path(id): Path<Uuid>) -> Response {
    let existing = match entity::Entity::find_by_id(id).one(&db).await {
        Ok(Some(e)) => e,
        Ok(None) => return (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "admin: load entity failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };
    let (types, jurs) = load_entity_choices(&db).await;
    let type_choices: Vec<_> = types
        .iter()
        .map(|t| admin_views::entities::TypeChoice {
            id: t.id,
            name: &t.name,
        })
        .collect();
    let jur_choices: Vec<_> = jurs
        .iter()
        .map(|j| admin_views::entities::JurisdictionChoice {
            id: j.id,
            name: &j.name,
            code: &j.code,
        })
        .collect();
    admin_views::entities::edit_form(
        existing.id,
        &admin_views::entities::EntityForm {
            name: &existing.name,
            entity_type_id: Some(existing.entity_type_id),
            jurisdiction_id: Some(existing.jurisdiction_id),
            error: None,
        },
        &type_choices,
        &jur_choices,
    )
    .into_response()
}

async fn entities_update(
    State(db): State<Db>,
    Path(id): Path<Uuid>,
    Form(input): Form<EntityInput>,
) -> Response {
    let existing = match entity::Entity::find_by_id(id).one(&db).await {
        Ok(Some(e)) => e,
        Ok(None) => return (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "admin: load entity failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };
    let mut active: entity::ActiveModel = existing.into();
    active.name = ActiveValue::Set(input.name);
    active.entity_type_id = ActiveValue::Set(input.entity_type_id);
    active.jurisdiction_id = ActiveValue::Set(input.jurisdiction_id);
    match active.update(&db).await {
        Ok(_) => Redirect::to("/portal/admin/entities").into_response(),
        Err(e) if store::is_unique_violation(&e) => {
            tracing::warn!(error = %e, "admin: update entity uniqueness conflict");
            (
                StatusCode::CONFLICT,
                "An entity with that key already exists.",
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "admin: update entity failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response()
        }
    }
}

async fn entities_delete(
    State(db): State<Db>,
    Path(id): Path<Uuid>,
    headers: axum::http::HeaderMap,
) -> Response {
    match entity::Entity::delete_by_id(id).exec(&db).await {
        Ok(_) => delete_response(&headers, "/portal/admin/entities"),
        Err(e) => {
            tracing::warn!(error = %e, entity_id = %id, "entities_delete: delete failed");
            delete_error_toast(
                &headers,
                &format!(
                    "Couldn't delete this entity — {}.",
                    store::db_error::describe_write_failure(&e)
                ),
                "/portal/admin/entities",
            )
        }
    }
}

async fn load_entity_choices(db: &Db) -> (Vec<entity_type::Model>, Vec<jurisdiction::Model>) {
    let types = entity_type::Entity::find()
        .all(db)
        .await
        .unwrap_or_default();
    let jurs = jurisdiction::Entity::find()
        .all(db)
        .await
        .unwrap_or_default();
    (types, jurs)
}

/// Shared `?sort=` query for the read-only listing endpoints. They
/// honor only `sort` today — no filters, no pagination — so a single
/// shape is enough.
#[derive(Deserialize, Default, Debug)]
struct ReadOnlySortQuery {
    #[serde(default)]
    sort: Option<String>,
}

// ---- Entity types (read-only) ----

async fn entity_types_index(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<ReadOnlySortQuery>,
) -> Response {
    use views::components::{SortDirection, SortSpec};
    let allowed: std::collections::HashSet<&str> = ["name"].into_iter().collect();
    let sort = match SortSpec::parse(q.sort.as_deref()).validated(&allowed) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let mut query = entity_type::Entity::find();
    for field in &sort.fields {
        if field.key == "name" {
            query = match field.direction {
                SortDirection::Ascending => query.order_by_asc(entity_type::Column::Name),
                SortDirection::Descending => query.order_by_desc(entity_type::Column::Name),
            };
        }
    }
    let rows_raw = query.all(&db).await.unwrap_or_default();
    let rows: Vec<admin_views::entity_types::Row<'_>> = rows_raw
        .iter()
        .map(|r| admin_views::entity_types::Row {
            id: r.id,
            name: &r.name,
        })
        .collect();
    admin_views::entity_types::list(&rows, &sort).into_response()
}

// ---- Templates (read-only) ----

async fn templates_index(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<ReadOnlySortQuery>,
) -> Response {
    use views::components::{SortDirection, SortSpec};
    let allowed: std::collections::HashSet<&str> =
        ["code", "title", "respondent_type"].into_iter().collect();
    let sort = match SortSpec::parse(q.sort.as_deref()).validated(&allowed) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    // Public catalog only — project-scoped templates are hidden here.
    let mut query = template::Entity::find().filter(template::Column::ProjectId.is_null());
    for field in &sort.fields {
        let col = match field.key.as_str() {
            "code" => template::Column::Code,
            "title" => template::Column::Title,
            "respondent_type" => template::Column::RespondentType,
            _ => continue,
        };
        query = match field.direction {
            SortDirection::Ascending => query.order_by_asc(col),
            SortDirection::Descending => query.order_by_desc(col),
        };
    }
    let rows_raw = query.all(&db).await.unwrap_or_default();
    let rows: Vec<admin_views::templates::Row<'_>> = rows_raw
        .iter()
        .map(|r| admin_views::templates::Row {
            id: r.id,
            code: &r.code,
            title: &r.title,
            respondent_type: &r.respondent_type,
        })
        .collect();
    admin_views::templates::list(&rows, &sort).into_response()
}

// ---- Questions (read-only) ----

async fn questions_index(
    State(db): State<Db>,
    axum::extract::Query(q): axum::extract::Query<ReadOnlySortQuery>,
) -> Response {
    use views::components::{SortDirection, SortSpec};
    let allowed: std::collections::HashSet<&str> = ["code", "answer_type"].into_iter().collect();
    let sort = match SortSpec::parse(q.sort.as_deref()).validated(&allowed) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let mut query = question::Entity::find();
    for field in &sort.fields {
        let col = match field.key.as_str() {
            "code" => question::Column::Code,
            "answer_type" => question::Column::AnswerType,
            _ => continue,
        };
        query = match field.direction {
            SortDirection::Ascending => query.order_by_asc(col),
            SortDirection::Descending => query.order_by_desc(col),
        };
    }
    let rows_raw = query.all(&db).await.unwrap_or_default();
    let rows: Vec<admin_views::questions::Row<'_>> = rows_raw
        .iter()
        .map(|r| admin_views::questions::Row {
            id: r.id,
            code: &r.code,
            prompt: &r.prompt,
            answer_type: &r.answer_type,
        })
        .collect();
    admin_views::questions::list(&rows, &sort).into_response()
}

// ---- Projects ----

/// JSON:API 1.1 query parameters honored by `/portal/projects`.
#[derive(Deserialize, Default, Debug)]
struct ProjectsListQuery {
    #[serde(default)]
    sort: Option<String>,
}

async fn projects_index(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    axum::extract::Query(q): axum::extract::Query<ProjectsListQuery>,
) -> Response {
    use views::components::{SortDirection, SortSpec};
    let token = csrf_token(session.as_deref());
    let allowed: std::collections::HashSet<&str> =
        ["name", "status", "entity_name"].into_iter().collect();
    let sort = match SortSpec::parse(q.sort.as_deref()).validated(&allowed) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let entities = entity::Entity::find().all(&db).await.unwrap_or_default();
    let by_entity = |id: Uuid| {
        entities
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.name.as_str())
    };

    // Row-level visibility: admin sees every project; staff and client
    // see only projects with a matching person_project_roles row. The
    // OPA layer already 403s a missing session; reaching this handler
    // means a session is in hand.
    let (person_id, role) = match session.as_deref() {
        Some(s) => (s.person_id, s.role),
        None => (None, store::entity::person::Role::Client),
    };
    let mut rows_raw = match crate::access::visible_projects(&db, person_id, role).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(error = %e, "projects_index: visible_projects failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response();
        }
    };

    // `name` and `status` sort in memory now that we've filtered;
    // `entity_name` was always an in-memory sort by resolved FK name.
    for field in &sort.fields {
        match field.key.as_str() {
            "name" => match field.direction {
                SortDirection::Ascending => rows_raw.sort_by(|a, b| a.name.cmp(&b.name)),
                SortDirection::Descending => rows_raw.sort_by(|a, b| b.name.cmp(&a.name)),
            },
            "status" => match field.direction {
                SortDirection::Ascending => rows_raw.sort_by(|a, b| a.status.cmp(&b.status)),
                SortDirection::Descending => rows_raw.sort_by(|a, b| b.status.cmp(&a.status)),
            },
            _ => {}
        }
    }
    for field in &sort.fields {
        if field.key == "entity_name" {
            match field.direction {
                SortDirection::Ascending => rows_raw.sort_by(|a, b| {
                    by_entity(a.entity_id)
                        .unwrap_or("")
                        .cmp(by_entity(b.entity_id).unwrap_or(""))
                }),
                SortDirection::Descending => rows_raw.sort_by(|a, b| {
                    by_entity(b.entity_id)
                        .unwrap_or("")
                        .cmp(by_entity(a.entity_id).unwrap_or(""))
                }),
            }
        }
    }

    // Lifecycle flags: every matter should open on an onboarding
    // (`onboarding__*`) notation, and a `closed` matter should carry a
    // `closing__letter`. Resolve which matters have each, in two batched
    // queries, then flag the gaps in the list.
    let (has_onboarding, has_closing) = matter_lifecycle_sets(&db, &rows_raw).await;

    let rows: Vec<admin_views::projects::Row<'_>> = rows_raw
        .iter()
        .map(|r| {
            let (missing_retainer, missing_closing_letter) = matter_flags(
                has_onboarding.contains(&r.id),
                &r.status,
                has_closing.contains(&r.id),
            );
            admin_views::projects::Row {
                id: r.id,
                name: &r.name,
                status: &r.status,
                entity_name: by_entity(r.entity_id),
                missing_retainer,
                missing_closing_letter,
            }
        })
        .collect();
    admin_views::projects::list(&rows, token, &sort).into_response()
}

/// Derive the matter-lifecycle warning flags from what notations a matter
/// carries. Pure so the rule is unit-testable in isolation: a matter is
/// missing its retainer when it has no onboarding notation, and missing
/// its closing letter only when it is `closed` and has no closing notation
/// (an open matter does not owe one yet).
#[must_use]
pub fn matter_flags(has_onboarding: bool, status: &str, has_closing: bool) -> (bool, bool) {
    let missing_retainer = !has_onboarding;
    let missing_closing_letter = status == "closed" && !has_closing;
    (missing_retainer, missing_closing_letter)
}

/// For the given matters, return `(project_ids with an onboarding notation,
/// project_ids with a closing__letter notation)` in two batched queries
/// (notations for these projects, then the templates they bind).
async fn matter_lifecycle_sets(
    db: &Db,
    projects: &[project::Model],
) -> (
    std::collections::HashSet<Uuid>,
    std::collections::HashSet<Uuid>,
) {
    use std::collections::{HashMap, HashSet};
    let project_ids: Vec<Uuid> = projects.iter().map(|p| p.id).collect();
    let mut has_onboarding = HashSet::new();
    let mut has_closing = HashSet::new();
    if project_ids.is_empty() {
        return (has_onboarding, has_closing);
    }
    let notations = notation::Entity::find()
        .filter(notation::Column::ProjectId.is_in(project_ids))
        .all(db)
        .await
        .unwrap_or_default();
    let template_ids: Vec<Uuid> = notations.iter().map(|n| n.template_id).collect();
    let code_by_template: HashMap<Uuid, String> = template::Entity::find()
        .filter(template::Column::Id.is_in(template_ids))
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|t| (t.id, t.code))
        .collect();
    for n in &notations {
        if let Some(code) = code_by_template.get(&n.template_id) {
            if code.starts_with("onboarding__") {
                has_onboarding.insert(n.project_id);
            }
            if code == "closing__letter" {
                has_closing.insert(n.project_id);
            }
        }
    }
    (has_onboarding, has_closing)
}

async fn projects_new_staff_only(
    State(state): State<AdminState>,
    session: Option<Extension<SessionData>>,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        return not_found_response();
    }
    let entities = entity::Entity::find()
        .all(&state.db)
        .await
        .unwrap_or_default();
    let choices: Vec<_> = entities
        .iter()
        .map(|e| admin_views::projects::EntityChoice {
            id: e.id,
            name: &e.name,
        })
        .collect();
    // The onboarding-template picker for the optional retainer block, plus
    // the required client picker (existing clients only).
    let retainer_templates = crate::retainer_walk::onboarding_templates(&state.db).await;
    let clients = client_people(&state.db).await;
    let client_choices: Vec<_> = clients
        .iter()
        .map(|p| admin_views::projects::PersonChoice {
            id: p.id,
            name: &p.name,
            email: &p.email,
        })
        .collect();
    admin_views::projects::new_form(
        &admin_views::projects::Form {
            client_dri_choices: &client_choices,
            retainer_templates: &retainer_templates,
            ..Default::default()
        },
        &choices,
    )
    .into_response()
}

#[derive(Deserialize)]
struct ProjectInput {
    name: String,
    status: String,
    #[serde(default)]
    entity_id: Option<Uuid>,
    /// The matter's scope narrative ("this project's story"). Persisted to
    /// `projects.description` and, when a retainer is opened in the same
    /// action, seeded as the notation's position-0 custom clause.
    #[serde(default)]
    description: String,
    /// The required client-side DRI: which existing `Role::Client` person
    /// this matter is opened for. The client must pre-exist (the picker
    /// lists existing clients); validated below.
    #[serde(default)]
    client_dri_person_id: Option<Uuid>,
    /// The onboarding template for the matter's retainer. **Required** —
    /// every matter opens on a retainer (a project is not official until
    /// one exists), so there is no "plain project" path. Validated below.
    #[serde(default)]
    retainer_template_code: String,
    #[serde(default)]
    scope_of_services: String,
    /// Set (to `"1"`) when staff tick the conflict-acknowledgment
    /// checkbox to override review-level conflict findings. `None` on a
    /// first submit; the handler re-renders the form with the override
    /// checkbox until it is present. A *blocking* conflict ignores this —
    /// there is no override for adversity to a current client.
    #[serde(default)]
    conflict_ack: Option<String>,
}

fn nonblank(s: &str) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Resolve the firm-side person who is the matter's staff DRI: the opening
/// staffer when their session is linked to a Person, else the firm's
/// default principal (resolved by role) so a matter still opens with a
/// real, NOT-NULL DRI under the dev auth-bypass. `None` only when neither
/// exists — an unseeded DB with an unlinked session, which the caller
/// rejects.
async fn resolve_staff_dri(db: &Db, session: Option<&SessionData>) -> Option<Uuid> {
    if let Some(id) = session.and_then(|s| s.person_id) {
        return Some(id);
    }
    store::persons::default_firm_dri(db).await.ok().flatten()
}

/// The existing `Role::Client` persons offered in the create form's
/// required client-DRI picker. A matter's client must pre-exist (the
/// client field exists before the project), so the form selects one rather
/// than conjuring a client mid-open.
async fn client_people(db: &Db) -> Vec<person::Model> {
    person::Entity::find()
        .filter(person::Column::Role.eq(person::Role::Client))
        .order_by_asc(person::Column::Name)
        .all(db)
        .await
        .unwrap_or_default()
}

/// Validate the create form's selected client DRI: it must be present,
/// exist, and carry `Role::Client`. Returns the client's `person::Model`
/// (so the caller has its email/name for the retainer) or a re-rendered
/// `422` form error. The client-side DRI is a real client of record, never
/// a firm attorney — both the engineering and legal councils flagged the
/// firm-as-its-own-client default as a conflict/loyalty problem.
async fn selected_client_dri(
    state: &AdminState,
    input: &ProjectInput,
) -> Result<person::Model, Response> {
    let Some(id) = input.client_dri_person_id else {
        return Err(retainer_form_error(state, input, "Pick the client this matter is for.").await);
    };
    let row = match person::Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return Err(retainer_form_error(
                state,
                input,
                "That client was not found — pick an existing client (create them first if needed).",
            )
            .await);
        }
        Err(e) => {
            tracing::error!(error = %e, "projects_create: client DRI lookup failed");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response());
        }
    };
    if row.role != person::Role::Client {
        return Err(retainer_form_error(
            state,
            input,
            "The client DRI must be an existing client person.",
        )
        .await);
    }
    Ok(row)
}

/// Re-render the project create form with a retainer-validation error and
/// the submitted values echoed back, as `422 Unprocessable Entity`. No
/// matter is created — the caller returns this before any insert, or
/// after dropping (rolling back) the open transaction.
/// Re-render the matter-open form with a message. `allow_conflict_override`
/// adds the conflict-acknowledgment checkbox so authorized staff can
/// proceed past *review-level* conflict findings; it stays `false` for
/// ordinary validation errors and for hard conflict blocks.
async fn project_form_response(
    state: &AdminState,
    input: &ProjectInput,
    msg: &str,
    allow_conflict_override: bool,
) -> Response {
    let entities = entity::Entity::find()
        .all(&state.db)
        .await
        .unwrap_or_default();
    let choices: Vec<_> = entities
        .iter()
        .map(|e| admin_views::projects::EntityChoice {
            id: e.id,
            name: &e.name,
        })
        .collect();
    let retainer_templates = crate::retainer_walk::onboarding_templates(&state.db).await;
    let clients = client_people(&state.db).await;
    let client_choices: Vec<_> = clients
        .iter()
        .map(|p| admin_views::projects::PersonChoice {
            id: p.id,
            name: &p.name,
            email: &p.email,
        })
        .collect();
    let form = admin_views::projects::Form {
        name: &input.name,
        status: &input.status,
        entity_id: input.entity_id,
        client_dri_person_id: input.client_dri_person_id,
        client_dri_choices: &client_choices,
        description: &input.description,
        error: Some(msg),
        retainer_template_code: &input.retainer_template_code,
        scope_of_services: &input.scope_of_services,
        retainer_templates: &retainer_templates,
        allow_conflict_override,
    };
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        admin_views::projects::new_form(&form, &choices),
    )
        .into_response()
}

async fn retainer_form_error(state: &AdminState, input: &ProjectInput, msg: &str) -> Response {
    project_form_response(state, input, msg, false).await
}

/// Re-render the form with conflict findings and the override checkbox so
/// authorized staff can acknowledge review-level findings and proceed.
async fn retainer_conflict_warning(
    state: &AdminState,
    input: &ProjectInput,
    msg: &str,
) -> Response {
    project_form_response(state, input, msg, true).await
}

/// POST `/portal/projects` — open a matter and, when the "Send retainer
/// for signature" box is ticked, create the retainer in the same action:
/// client Person + `client` role + retainer Notation + seeded answers,
/// driven to the `staff_review` gate. Unchecked, it stays a plain project
/// create. The whole thing is one transaction, so a failed retainer never
/// leaves a half-open matter.
#[allow(clippy::too_many_lines)]
async fn projects_create_staff_only(
    State(state): State<AdminState>,
    session: Option<Extension<SessionData>>,
    Form(input): Form<ProjectInput>,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        return not_found_response();
    }
    if input.name.trim().is_empty() {
        return Redirect::to("/portal/projects/new").into_response();
    }

    // The client-side DRI is required on **every** create and must be a
    // real, pre-existing `Role::Client` person — never a firm attorney
    // (both the engineering and legal councils flagged the
    // firm-as-its-own-client default as a conflict/loyalty problem). The
    // client field exists before the project: validate it before any row.
    let client = match selected_client_dri(&state, &input).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    // Every matter opens on a retainer — a project is not official until
    // one exists — so the onboarding template is required, always. There is
    // no plain-project path. The retainer is sent to the client above.
    if input.retainer_template_code.trim().is_empty() {
        return retainer_form_error(
            &state,
            &input,
            "Pick an onboarding template — every matter opens on a retainer.",
        )
        .await;
    }

    // A matter always opens against a **pre-existing** entity
    // (`projects.entity_id` is NOT NULL). Require one and confirm it
    // exists before any row is created — never conjure one mid-open.
    let Some(entity_id) = input.entity_id else {
        return retainer_form_error(
            &state,
            &input,
            "Pick an entity to open the matter against (create the entity first if needed).",
        )
        .await;
    };
    match entity::Entity::find_by_id(entity_id).one(&state.db).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return retainer_form_error(
                &state,
                &input,
                "That entity was not found — open the matter against an existing entity.",
            )
            .await;
        }
        Err(e) => {
            tracing::error!(error = %e, "projects_create: entity lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    }

    // Conflict check — runs on **every** matter open, before any row is
    // written. The relationship graph (`store::conflicts`) is advisory to
    // clear but authoritative to block: a confident, direct adverse link
    // to a current client hard-stops the open; softer entanglements
    // (shared party, recorded disclosure) surface for authorized staff to
    // acknowledge. The graph can raise a conflict; only a person clears
    // one — it is never assumed complete.
    let conflict = match store::conflicts::check_new_matter(&state.db, client.id, entity_id).await {
        Ok(report) => report,
        Err(e) => {
            tracing::error!(error = %e, "projects_create: conflict check failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };
    if conflict.has_blocking() {
        let msg = format!(
            "Conflict check blocked this matter — it is adverse to a current client. \
             Resolve the conflict or record a waiver before opening.\n\n{}",
            conflict.summary_lines().join("\n"),
        );
        return retainer_form_error(&state, &input, &msg).await;
    }
    if !conflict.is_clear() && input.conflict_ack.as_deref() != Some("1") {
        let msg = format!(
            "Conflict check flagged this matter for review. Confirm you have reviewed \
             these findings and are authorized to proceed.\n\n{}",
            conflict.summary_lines().join("\n"),
        );
        return retainer_conflict_warning(&state, &input, &msg).await;
    }

    // Resolve the matter's **staff-side** DRI — a required, NOT NULL column
    // (every matter names exactly one accountable attorney/admin). The
    // opening staffer is it; a session not linked to a firm Person (the dev
    // bypass) falls back to the seeded firm principal so a matter never
    // opens without a real responsible person, with no sentinel row.
    let Some(staff_dri_id) = resolve_staff_dri(&state.db, session.as_deref()).await else {
        return retainer_form_error(
            &state,
            &input,
            "Your session isn't linked to a firm person — cannot open a matter.",
        )
        .await;
    };

    // One transaction: the project, its DRI role, and — when the box is
    // ticked — the client, role, retainer Notation, and seeded answers, so
    // a failure rolls back as a unit.
    let txn = match state.db.begin().await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "projects_create: txn begin failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    // Both DRI columns are NOT NULL and set to real persons at insert: the
    // staff side to the opener/firm principal, the client side to the
    // pre-existing client just validated. No sentinel, no placeholder.
    let project_id = match (project::ActiveModel {
        name: ActiveValue::Set(input.name.clone()),
        status: ActiveValue::Set(input.status.clone()),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(staff_dri_id)),
        client_dri_person_id: ActiveValue::Set(Some(client.id)),
        description: ActiveValue::Set(nonblank(&input.description)),
        ..Default::default()
    })
    .insert(&txn)
    .await
    {
        Ok(p) => p.id,
        Err(e) => {
            tracing::error!(error = %e, "projects_create: project insert failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    // Record a conflict-review override to the relationship log, in the
    // same transaction as the matter it authorized. We only reach here
    // with findings when staff ticked the acknowledgment (a block already
    // returned; a clean check has none), so this row is the durable audit
    // trail of who opened a flagged matter and over what findings.
    if !conflict.is_clear() {
        let detail = format!(
            "Conflict review acknowledged at matter open:\n{}",
            conflict.summary_lines().join("\n"),
        );
        if let Err(e) = (relationship_log::ActiveModel {
            actor_person_id: ActiveValue::Set(session.as_deref().and_then(|s| s.person_id)),
            subject_type: ActiveValue::Set("project".to_string()),
            subject_id: ActiveValue::Set(project_id),
            action: ActiveValue::Set("conflict_review_acknowledged".to_string()),
            detail: ActiveValue::Set(detail),
            ..Default::default()
        })
        .insert(&txn)
        .await
        {
            tracing::error!(error = %e, "projects_create: conflict override audit insert failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    }

    // Designate the opening staffer as the matter's **staff-side** DRI —
    // the firm-internal person accountable for it — as a participation
    // role (not a bare column). The staffer pre-exists (they are logged
    // in); a session with no linked Person (the dev bypass) opens the
    // matter with no staff DRI yet.
    if let Some(staff_dri) = session.as_deref().and_then(|s| s.person_id) {
        if let Err(e) = (person_project_role::ActiveModel {
            person_id: ActiveValue::Set(staff_dri),
            project_id: ActiveValue::Set(project_id),
            participation: ActiveValue::Set(
                store::entity::person_project_role::PARTICIPATION_STAFF_DRI.to_string(),
            ),
            ..Default::default()
        })
        .insert(&txn)
        .await
        {
            tracing::error!(error = %e, "projects_create: staff DRI role insert failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    }

    // Resolve the chosen onboarding template inside the txn.
    let code = input.retainer_template_code.trim();
    let template_row = match template::Entity::find()
        .filter(template::Column::Code.eq(code))
        .one(&txn)
        .await
    {
        Ok(Some(t)) => t,
        Ok(None) => {
            return retainer_form_error(&state, &input, "That onboarding template was not found.")
                .await
        }
        Err(e) => {
            tracing::error!(error = %e, "projects_create: template lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    // Attach the retainer Notation to the matter, sent to the selected
    // client (the same person already on the project's client_dri column).
    // `link_retainer_rows` resolves them by email — they pre-exist, so this
    // links rather than creates — and attaches the `client` participation
    // row for portal visibility. The matter-open client is *emailed* a
    // signing link; they are not in the room and have no portal session yet.
    let rows = match crate::retainer_walk::link_retainer_rows(
        &txn,
        template_row.id,
        project_id,
        &client.email,
        Some(&client.name),
        store::entity::notation::DELIVERY_EMAILED,
    )
    .await
    {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    // Seed the matter's scope narrative as the retainer's position-0
    // custom clause ("the firm's standing terms plus this project's
    // story"). System provenance (`authored_by_person_id = None`) marks it
    // as auto-seeded, not staff-authored; it is a draft that the attorney
    // edits at the `staff_review` gate before any send. The notation is
    // fresh, so position 0 renders it first at the `{{custom_clauses}}`
    // marker, ahead of any clause staff add by hand.
    if let Some(description) = nonblank(&input.description) {
        if let Err(e) = (store::entity::notation_clause::ActiveModel {
            notation_id: ActiveValue::Set(rows.notation_id),
            position: ActiveValue::Set(0),
            body_markdown: ActiveValue::Set(description),
            authored_by_person_id: ActiveValue::Set(None),
            ..Default::default()
        })
        .insert(&txn)
        .await
        {
            tracing::error!(error = %e, "projects_create: scope clause seed failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    }

    // Seed the retainer questionnaire from the form so the assembled
    // agreement renders complete at the review screen: the matter's own
    // name is the `project_name`; the scope fills `product_description`.
    let staffer = session.as_deref().and_then(|s| s.person_id);
    if let Err(e) = crate::retainer_walk::seed_staff_answers(
        &txn,
        rows.notation_id,
        rows.person_id,
        staffer,
        &[
            ("client_name", client.name.trim()),
            ("client_email", client.email.trim()),
            ("project_name", input.name.trim()),
            ("product_description", input.scope_of_services.trim()),
        ],
    )
    .await
    {
        tracing::error!(error = %e, "projects_create: seed answers failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }

    if let Err(e) = txn.commit().await {
        tracing::error!(error = %e, "projects_create: commit failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }

    // Drive the workflow to the `staff_review` gate (never auto-send),
    // then land staff on the review/approve screen. The human approve
    // step is the only thing that emits the envelope.
    if let Err(e) = crate::retainer_walk::advance_to_staff_review(&state, rows.notation_id).await {
        tracing::error!(
            error = %e,
            notation_id = %rows.notation_id,
            "projects_create: drive to staff_review failed",
        );
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }
    Redirect::to(&format!(
        "/portal/admin/notations/{}/review",
        rows.notation_id
    ))
    .into_response()
}

async fn projects_edit_staff_only(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    Path(id): Path<Uuid>,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        return not_found_response();
    }
    let existing = match project::Entity::find_by_id(id).one(&db).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response(),
    };
    let entities = entity::Entity::find().all(&db).await.unwrap_or_default();
    let choices: Vec<_> = entities
        .iter()
        .map(|e| admin_views::projects::EntityChoice {
            id: e.id,
            name: &e.name,
        })
        .collect();
    admin_views::projects::edit_form(
        existing.id,
        &admin_views::projects::Form {
            name: &existing.name,
            status: &existing.status,
            entity_id: Some(existing.entity_id),
            description: existing.description.as_deref().unwrap_or(""),
            error: None,
            // No retainer block on the edit form (empty templates hides it).
            ..Default::default()
        },
        &choices,
    )
    .into_response()
}

/// `GET /portal/projects/{id}` — role-aware project detail.
///
/// - **Client** → forward to the lightweight view in
///   [`crate::portal::projects::detail`] (no Edit / Upload / Delete
///   chrome). The handler already gates on
///   [`crate::access::can_see_project`] and returns `404` for
///   non-participants.
/// - **Staff / Admin** → render the admin chrome (`projects_detail`):
///   header + documents table + multipart upload form + drive sync
///   button. Edit form stays at `/portal/projects/{id}/edit`.
async fn projects_detail_role_aware(
    State(db): State<Db>,
    State(storage): State<std::sync::Arc<dyn cloud::StorageService>>,
    Path(id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        // Re-extract the typed `Extension<SessionData>` for the
        // portal handler, which requires a session.
        return match session {
            Some(s) => {
                crate::portal::projects::detail(State(db), State(storage), Path(id), s).await
            }
            None => not_found_response(),
        };
    }
    // Staff reach the admin chrome but are row-scoped exactly like
    // clients — only admin bypasses (per docs/access-model.md). A staff
    // member who isn't on the matter gets a 404, not a peek: the matter
    // "doesn't exist" for them.
    let (person_id, role) = match session.as_deref() {
        Some(s) => (s.person_id, s.role),
        None => (None, store::entity::person::Role::Client),
    };
    match crate::access::can_see_project(&db, person_id, role, id).await {
        Ok(true) => {}
        Ok(false) => return (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "admin: can_see_project failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    }
    let existing = match project::Entity::find_by_id(id).one(&db).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "admin: load project failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    // Resolve the matter's entity name (single fetch). Every matter is
    // opened against an entity, so this is always present.
    let entity_name = entity::Entity::find_by_id(existing.entity_id)
        .one(&db)
        .await
        .ok()
        .flatten()
        .map(|e| e.name);

    // Resolve the two Directly Responsible Individuals (staff + client) for
    // display from the authoritative project columns — a first-class matter
    // attribute, distinct from the participation ledger.
    let staff_dri = person_name(&db, existing.staff_dri_person_id).await;
    let client_dri = person_name(&db, existing.client_dri_person_id).await;

    // Documents — list view is just filename + download link. Blob
    // content type / size / SHA live on the per-document detail page,
    // which fetches its own blob row.
    let docs_raw = document::Entity::find()
        .filter(document::Column::ProjectId.eq(id))
        .order_by_desc(document::Column::InsertedAt)
        .all(&db)
        .await
        .unwrap_or_default();
    let doc_rows: Vec<admin_views::projects::DocumentRow<'_>> = docs_raw
        .iter()
        .map(|d| admin_views::projects::DocumentRow {
            id: d.id,
            filename: &d.filename,
        })
        .collect();

    // Northstar: surface the transcript-driven estate notation (if any)
    // so staff disclosed to the matter can drive the recorded-sitting flow
    // from the matter page — the transcript-upload form at BEGIN, the
    // workflow status thereafter.
    let estate = estate_notation_for_project(&db, id).await;
    let estate_drafts = if let Some((nid, _)) = &estate {
        store::review_documents::for_notation(&db, *nid)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let draft_rows: Vec<admin_views::projects::EstateDraftRow<'_>> = estate_drafts
        .iter()
        .map(|d| admin_views::projects::EstateDraftRow {
            title: &d.title,
            kind: &d.kind,
            status: &d.status,
        })
        .collect();
    let estate_view = estate
        .as_ref()
        .map(|(nid, state)| admin_views::projects::EstateMatter {
            notation_id: *nid,
            state,
            drafts: &draft_rows,
        });

    let token = csrf_token(session.as_deref());
    admin_views::projects::detail(&admin_views::projects::Detail {
        id: existing.id,
        name: &existing.name,
        status: &existing.status,
        entity_name: entity_name.as_deref(),
        staff_dri: staff_dri.as_deref(),
        client_dri: client_dri.as_deref(),
        documents: &doc_rows,
        estate: estate_view,
        csrf_token: if token.is_empty() { None } else { Some(token) },
    })
    .into_response()
}

/// Resolve a person's display name from a nullable DRI column, for display.
/// `None` when the matter has no DRI of that side yet (a legacy row) or the
/// row is missing.
async fn person_name(db: &Db, person_id: Option<Uuid>) -> Option<String> {
    person::Entity::find_by_id(person_id?)
        .one(db)
        .await
        .ok()
        .flatten()
        .map(|p| p.name)
}

/// Find the project's transcript-driven estate notation, as
/// `(id, current state)` for the admin matter page. Delegates to the
/// shared, data-driven detector in [`crate::estate`].
async fn estate_notation_for_project(db: &Db, project_id: Uuid) -> Option<(Uuid, String)> {
    crate::estate::transcript_driven_notation(db, project_id)
        .await
        .map(|n| (n.id, n.state))
}

async fn projects_update_staff_only(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    Path(id): Path<Uuid>,
    Form(input): Form<ProjectInput>,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        return not_found_response();
    }
    let existing = match project::Entity::find_by_id(id).one(&db).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response(),
    };
    let mut active: project::ActiveModel = existing.into();
    active.name = ActiveValue::Set(input.name);
    active.status = ActiveValue::Set(input.status);
    if let Some(eid) = input.entity_id {
        active.entity_id = ActiveValue::Set(eid);
    }
    active.description = ActiveValue::Set(nonblank(&input.description));
    let _ = active.update(&db).await;
    Redirect::to("/portal/projects").into_response()
}

async fn projects_delete_staff_only(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    Path(id): Path<Uuid>,
    headers: axum::http::HeaderMap,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        return not_found_response();
    }
    match project::Entity::delete_by_id(id).exec(&db).await {
        Ok(_) => delete_response(&headers, "/portal/projects"),
        Err(e) => {
            tracing::warn!(error = %e, project_id = %id, "projects_delete: delete failed");
            delete_error_toast(
                &headers,
                &format!(
                    "Couldn't delete this matter — {}.",
                    store::db_error::describe_write_failure(&e)
                ),
                "/portal/projects",
            )
        }
    }
}

// ---- Read-only listings (one handler per remaining domain table) ----

/// Run a read-only admin listing: execute `query`, map each model to a
/// row of display strings via `row`, and render the shared table.
///
/// A DB error is logged and rendered as a visible failure panel —
/// unlike the historical `.unwrap_or_default()`, a failed query no
/// longer masquerades as an empty table. Handlers that need a join or
/// aggregation (e.g. `mailrooms_list`, `letters_list`) stay bespoke;
/// this collapses only the single-entity listings.
async fn render_listing<E, F>(
    db: &Db,
    query: sea_orm::Select<E>,
    title: &'static str,
    heading: &'static str,
    headers: &'static [&'static str],
    row: F,
) -> Markup
where
    E: EntityTrait,
    F: Fn(E::Model) -> Vec<String>,
{
    match query.all(db).await {
        Ok(rows) => admin_views::render_list(&admin_views::ListPage {
            title,
            heading,
            headers,
            rows: rows.into_iter().map(row).collect(),
        }),
        Err(e) => {
            tracing::error!(error = %e, heading, "admin listing query failed");
            admin_views::render_load_error(title, heading)
        }
    }
}

async fn notations_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        notation::Entity::find(),
        "Notations — Admin",
        "Notations",
        &["Template", "Person", "Entity", "State"],
        |n| {
            vec![
                n.template_id.to_string(),
                n.person_id.to_string(),
                n.entity_id.map_or("—".into(), |x| x.to_string()),
                n.state,
            ]
        },
    )
    .await
}

async fn answers_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        answer::Entity::find(),
        "Answers — Admin",
        "Answers",
        &["Question", "Person", "Value"],
        |a| {
            vec![
                a.question_id.to_string(),
                a.person_id.to_string(),
                answer::display_value(&a.value),
            ]
        },
    )
    .await
}

async fn addresses_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        address::Entity::find(),
        "Addresses — Admin",
        "Addresses",
        &["Owner", "Line 1", "City", "Region", "Country"],
        |a| {
            let owner = a.person_id.map_or_else(
                || a.entity_id.map_or("—".into(), |id| format!("entity/{id}")),
                |id| format!("person/{id}"),
            );
            vec![owner, a.line1, a.city, a.region, a.country]
        },
    )
    .await
}

async fn mailrooms_list(State(db): State<Db>) -> Markup {
    let rows_raw = mailroom::Entity::find()
        .order_by_asc(mailroom::Column::Id)
        .all(&db)
        .await
        .unwrap_or_default();
    let addresses = address::Entity::find().all(&db).await.unwrap_or_default();
    let by_address = |id: Uuid| {
        addresses.iter().find(|a| a.id == id).map_or_else(
            || format!("(unknown address #{id})"),
            |a| format!("{}, {}, {}", a.line1, a.city, a.region),
        )
    };
    admin_views::render_list(&admin_views::ListPage {
        title: "Mailrooms — Admin",
        heading: "Mailrooms",
        headers: &["Name", "Address"],
        rows: rows_raw
            .into_iter()
            .map(|m| vec![m.name.clone(), by_address(m.address_id)])
            .collect(),
    })
}

async fn entity_cap_table(
    State(db): State<Db>,
    Path(id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<ReadOnlySortQuery>,
) -> Response {
    use views::components::{SortDirection, SortSpec};
    let allowed: std::collections::HashSet<&str> =
        ["holder_name", "shares", "percent"].into_iter().collect();
    let sort = match SortSpec::parse(q.sort.as_deref()).validated(&allowed) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let entity_row = entity::Entity::find_by_id(id)
        .one(&db)
        .await
        .unwrap_or_default();
    let entity_name = entity_row
        .as_ref()
        .map_or_else(|| format!("(unknown entity #{id})"), |e| e.name.clone());

    let issuances = share_issuance::Entity::find()
        .filter(share_issuance::Column::EntityId.eq(id))
        .all(&db)
        .await
        .unwrap_or_default();

    // Aggregate by holder_name. Insertion-ordered so the rendered
    // table is deterministic for tests.
    let mut totals: Vec<(String, i64)> = Vec::new();
    for iss in &issuances {
        if let Some(slot) = totals.iter_mut().find(|(name, _)| name == &iss.holder_name) {
            slot.1 += iss.shares;
        } else {
            totals.push((iss.holder_name.clone(), iss.shares));
        }
    }
    // Default ordering — biggest holder first, alphabetical on tie —
    // preserved when no ?sort= is supplied.
    if sort.fields.is_empty() {
        totals.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    } else {
        for field in &sort.fields {
            match field.key.as_str() {
                "holder_name" => match field.direction {
                    SortDirection::Ascending => totals.sort_by(|a, b| a.0.cmp(&b.0)),
                    SortDirection::Descending => totals.sort_by(|a, b| b.0.cmp(&a.0)),
                },
                "shares" | "percent" => match field.direction {
                    // shares and percent are monotonically related so
                    // sorting by either is the same comparison.
                    SortDirection::Ascending => totals.sort_by_key(|t| t.1),
                    SortDirection::Descending => totals.sort_by_key(|t| std::cmp::Reverse(t.1)),
                },
                _ => {}
            }
        }
    }
    let total_shares: i64 = totals.iter().map(|(_, s)| *s).sum();

    let rows: Vec<admin_views::cap_table::CapTableRow<'_>> = totals
        .iter()
        .map(|(name, shares)| admin_views::cap_table::CapTableRow {
            holder_name: name,
            shares: *shares,
            // Share counts comfortably below 2^52 in any real cap
            // table — precision loss isn't a practical concern.
            #[allow(clippy::cast_precision_loss)]
            percent: if total_shares > 0 {
                (*shares as f64) * 100.0 / (total_shares as f64)
            } else {
                0.0
            },
        })
        .collect();

    admin_views::cap_table::render(&admin_views::cap_table::CapTablePage {
        entity_id: id,
        entity_name: &entity_name,
        total_shares,
        rows: &rows,
        sort,
    })
    .into_response()
}

async fn letter_detail(State(db): State<Db>, Path(id): Path<Uuid>) -> Markup {
    let Some(letter) = letter::Entity::find_by_id(id)
        .one(&db)
        .await
        .unwrap_or_default()
    else {
        return admin_views::letters::not_found(id);
    };
    let mailroom_row = mailroom::Entity::find_by_id(letter.mailroom_id)
        .one(&db)
        .await
        .unwrap_or_default();
    let (mailroom_name, mailroom_address) = if let Some(m) = mailroom_row {
        let address = address::Entity::find_by_id(m.address_id)
            .one(&db)
            .await
            .unwrap_or_default()
            .map_or_else(
                || format!("(unknown address #{})", m.address_id),
                |a| format!("{}, {}, {}", a.line1, a.city, a.region),
            );
        (m.name, address)
    } else {
        (
            format!("(unknown mailroom #{})", letter.mailroom_id),
            String::new(),
        )
    };
    admin_views::letters::detail(&admin_views::letters::LetterDetail {
        id: letter.id,
        direction: &letter.direction,
        sender: &letter.sender,
        recipient: &letter.recipient,
        summary: &letter.summary,
        mailroom_name: &mailroom_name,
        mailroom_address: &mailroom_address,
    })
}

async fn letters_list(State(db): State<Db>) -> Markup {
    let rows_raw = letter::Entity::find()
        .order_by_asc(letter::Column::Id)
        .all(&db)
        .await
        .unwrap_or_default();
    let mailrooms = mailroom::Entity::find().all(&db).await.unwrap_or_default();
    let by_mailroom = |id: Uuid| {
        mailrooms
            .iter()
            .find(|m| m.id == id)
            .map_or_else(|| format!("(unknown #{id})"), |m| m.name.clone())
    };
    admin_views::render_list(&admin_views::ListPage {
        title: "Letters — Admin",
        heading: "Letters",
        headers: &["Mailroom", "Direction", "Sender", "Recipient", "Summary"],
        rows: rows_raw
            .into_iter()
            .map(|l| {
                vec![
                    by_mailroom(l.mailroom_id),
                    l.direction,
                    l.sender,
                    l.recipient,
                    l.summary,
                ]
            })
            .collect(),
    })
}

async fn blobs_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        blob::Entity::find(),
        "Blobs — Admin",
        "Blobs",
        &["Storage key", "Content type", "Bytes", "SHA-256"],
        |b| {
            vec![
                b.storage_key,
                b.content_type,
                b.byte_size.to_string(),
                b.sha256_hex,
            ]
        },
    )
    .await
}

async fn documents_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        document::Entity::find(),
        "Documents — Admin",
        "Documents",
        &["Project", "Blob", "Filename", "Kind"],
        |d| {
            vec![
                d.project_id.to_string(),
                d.blob_id.to_string(),
                d.filename,
                d.kind,
            ]
        },
    )
    .await
}

async fn person_entity_roles_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        person_entity_role::Entity::find(),
        "Person-entity roles — Admin",
        "Person-entity roles",
        &["Person", "Entity", "Role"],
        |r| vec![r.person_id.to_string(), r.entity_id.to_string(), r.role],
    )
    .await
}

async fn person_project_roles_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        person_project_role::Entity::find(),
        "Person-project roles — Admin",
        "Person-project roles",
        &["Person", "Project", "Participation"],
        |r| {
            vec![
                r.person_id.to_string(),
                r.project_id.to_string(),
                r.participation,
            ]
        },
    )
    .await
}

async fn entity_billing_profiles_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        entity_billing_profile::Entity::find(),
        "Entity billing profiles — Admin",
        "Entity billing profiles",
        &["Entity", "Billing email", "Billing address"],
        |p| {
            vec![
                p.entity_id.to_string(),
                p.billing_email,
                p.billing_address_id.map_or("—".into(), |x| x.to_string()),
            ]
        },
    )
    .await
}

async fn invoices_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        invoice::Entity::find(),
        "Invoices — Admin",
        "Invoices",
        &["Profile", "Number", "Status", "Total (cents)", "Currency"],
        |i| {
            vec![
                i.entity_billing_profile_id.to_string(),
                i.number,
                i.status,
                i.total_cents.to_string(),
                i.currency,
            ]
        },
    )
    .await
}

async fn invoice_line_items_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        invoice_line_item::Entity::find(),
        "Invoice line items — Admin",
        "Invoice line items",
        &["Invoice", "Description", "Qty", "Unit price (cents)"],
        |l| {
            vec![
                l.invoice_id.to_string(),
                l.description,
                l.quantity.to_string(),
                l.unit_price_cents.to_string(),
            ]
        },
    )
    .await
}

async fn jurisdictions_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        jurisdiction::Entity::find().order_by_asc(jurisdiction::Column::Code),
        "Jurisdictions — Admin",
        "Jurisdictions",
        &["Name", "Code"],
        |j| vec![j.name, j.code],
    )
    .await
}

async fn git_repositories_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        git_repository::Entity::find(),
        "Git repositories — Admin",
        "Git repositories",
        &["Remote hash", "Last commit SHA"],
        |g| vec![g.remote_hash, g.last_commit_sha],
    )
    .await
}

async fn disclosures_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        disclosure::Entity::find(),
        "Disclosures — Admin",
        "Disclosures",
        &["Entity", "Project", "Kind", "Summary"],
        |d| {
            vec![
                d.entity_id.map_or("—".into(), |x| x.to_string()),
                d.project_id.map_or("—".into(), |x| x.to_string()),
                d.kind,
                d.summary,
            ]
        },
    )
    .await
}

async fn relationship_logs_list(State(db): State<Db>) -> Markup {
    render_listing(
        &db,
        relationship_log::Entity::find(),
        "Relationship logs — Admin",
        "Relationship logs",
        &["Actor", "Subject type", "Subject", "Action", "Detail"],
        |r| {
            vec![
                r.actor_person_id.map_or("—".into(), |x| x.to_string()),
                r.subject_type,
                r.subject_id.to_string(),
                r.action,
                r.detail,
            ]
        },
    )
    .await
}

// --- CSV exports -----------------------------------------------------
//
// One endpoint per CRUD admin resource. Returns an RFC 4180 CSV with
// the columns the admin list pages already show, plus the resolved
// names of foreign-key references (entity type / jurisdiction /
// related entity) so the spreadsheet stays readable without joining
// back to other exports.

async fn people_csv(State(db): State<Db>) -> crate::admin_csv::CsvBody {
    let rows_raw = person::Entity::find()
        .order_by_asc(person::Column::Id)
        .all(&db)
        .await
        .unwrap_or_default();
    let rows: Vec<Vec<String>> = rows_raw
        .into_iter()
        .map(|p| vec![p.id.to_string(), p.name, p.email])
        .collect();
    crate::admin_csv::CsvBody {
        filename: "people.csv",
        headers: vec!["id", "name", "email"],
        rows,
    }
}

async fn entities_csv(State(db): State<Db>) -> crate::admin_csv::CsvBody {
    let rows_raw = entity::Entity::find()
        .order_by_asc(entity::Column::Id)
        .all(&db)
        .await
        .unwrap_or_default();
    let types = entity_type::Entity::find()
        .all(&db)
        .await
        .unwrap_or_default();
    let jurs = jurisdiction::Entity::find()
        .all(&db)
        .await
        .unwrap_or_default();
    let by_type = |id: Uuid| {
        types
            .iter()
            .find(|t| t.id == id)
            .map_or(String::new(), |t| t.name.clone())
    };
    let by_jur = |id: Uuid| {
        jurs.iter()
            .find(|j| j.id == id)
            .map_or(String::new(), |j| j.name.clone())
    };
    let rows: Vec<Vec<String>> = rows_raw
        .into_iter()
        .map(|e| {
            vec![
                e.id.to_string(),
                e.name,
                by_type(e.entity_type_id),
                by_jur(e.jurisdiction_id),
            ]
        })
        .collect();
    crate::admin_csv::CsvBody {
        filename: "entities.csv",
        headers: vec!["id", "name", "entity_type", "jurisdiction"],
        rows,
    }
}

async fn projects_csv(State(db): State<Db>) -> crate::admin_csv::CsvBody {
    let rows_raw = project::Entity::find()
        .order_by_asc(project::Column::Id)
        .all(&db)
        .await
        .unwrap_or_default();
    let entities = entity::Entity::find().all(&db).await.unwrap_or_default();
    let by_entity = |id: Uuid| {
        entities
            .iter()
            .find(|e| e.id == id)
            .map_or(String::new(), |e| e.name.clone())
    };
    let rows: Vec<Vec<String>> = rows_raw
        .into_iter()
        .map(|p| vec![p.id.to_string(), p.name, p.status, by_entity(p.entity_id)])
        .collect();
    crate::admin_csv::CsvBody {
        filename: "projects.csv",
        headers: vec!["id", "name", "status", "entity_name"],
        rows,
    }
}
