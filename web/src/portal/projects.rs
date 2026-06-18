//! `/portal/projects/:id` — single-matter detail, scoped per row.
//!
//! Admins see any project; staff and client see only the projects
//! they have a `person_project_roles` row for. Callers without a
//! matching row get `404` — never `403`. The matter doesn't exist
//! from their perspective. See
//! [`docs/access-model.md`](../../../../docs/access-model.md).

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use std::sync::Arc;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use store::entity::{document, notation, project, template};
use store::Db;
use uuid::Uuid;
use views::pages::portal as portal_views;

use crate::access::can_see_project;
use crate::retainer_walk::{
    certificate_of_completion_storage_key, document_pdf_storage_key, signed_document_storage_key,
};
use crate::session::SessionData;

/// Owned backing data for one notation row — the view borrows `&str`
/// from these, so they outlive the `NotationDocRow` slice.
struct NotationRowData {
    id: Uuid,
    title: String,
    status: &'static str,
    rendered_ready: bool,
    signed_ready: bool,
    certificate_ready: bool,
}

/// Client-friendly status for a notation, derived from its workflow state
/// and which PDFs have materialized — never the raw docket state.
fn notation_status_label(state: &str, signed_ready: bool, rendered_ready: bool) -> &'static str {
    if signed_ready {
        "Signed"
    } else if state.starts_with("sent_for_signature") {
        "Awaiting your signature"
    } else if rendered_ready {
        "Ready for signature"
    } else {
        "In preparation"
    }
}

/// Build the per-notation rows for a project: title (plain template name),
/// a client-friendly status, and which of the three PDFs exist. Storage
/// `exists` is a metadata-only HEAD on GCS, so a handful of probes per
/// matter is cheap.
async fn notation_rows(
    db: &Db,
    storage: &dyn cloud::StorageService,
    project_id: Uuid,
) -> Vec<NotationRowData> {
    let notations = notation::Entity::find()
        .filter(notation::Column::ProjectId.eq(project_id))
        .order_by_desc(notation::Column::InsertedAt)
        .all(db)
        .await
        .unwrap_or_default();
    let mut rows = Vec::with_capacity(notations.len());
    for n in &notations {
        let title = template::Entity::find_by_id(n.template_id)
            .one(db)
            .await
            .ok()
            .flatten()
            .map_or_else(|| "Agreement".to_string(), |t| t.title);
        let rendered_ready = storage
            .exists(&document_pdf_storage_key(n.id))
            .await
            .unwrap_or(false);
        let signed_ready = storage
            .exists(&signed_document_storage_key(n.id))
            .await
            .unwrap_or(false);
        let certificate_ready = storage
            .exists(&certificate_of_completion_storage_key(n.id))
            .await
            .unwrap_or(false);
        rows.push(NotationRowData {
            id: n.id,
            title,
            status: notation_status_label(&n.state, signed_ready, rendered_ready),
            rendered_ready,
            signed_ready,
            certificate_ready,
        });
    }
    rows
}

/// Render `/portal/projects/:id`.
///
/// Order matters: the row-visibility check runs before the row load,
/// so an unauthorised caller never even pulls the project name into
/// the response — they get the same 404 a missing id would produce.
pub async fn detail(
    State(db): State<Db>,
    State(storage): State<Arc<dyn cloud::StorageService>>,
    Path(id): Path<Uuid>,
    Extension(session): Extension<SessionData>,
) -> Response {
    let visible = can_see_project(&db, session.person_id, session.role, id)
        .await
        .unwrap_or(false);
    if !visible {
        return not_found();
    }
    let Ok(Some(project)) = project::Entity::find_by_id(id).one(&db).await else {
        return not_found();
    };

    // The matter's notations (retainer, etc.), named in plain words, each
    // with download links for whichever of its three PDFs exist. Served by
    // the notation-documents route under the same project-participation ACL.
    let notation_data = notation_rows(&db, storage.as_ref(), id).await;
    let notation_rows: Vec<portal_views::project_detail::NotationDocRow<'_>> = notation_data
        .iter()
        .map(|n| portal_views::project_detail::NotationDocRow {
            id: n.id,
            title: &n.title,
            status: n.status,
            rendered_ready: n.rendered_ready,
            signed_ready: n.signed_ready,
            certificate_ready: n.certificate_ready,
        })
        .collect();
    // Documents the client may read and comment on — only those an
    // attorney has advanced past `draft` (the human-in-the-loop gate).
    let review_docs = store::review_documents::client_visible_for_project(&db, id)
        .await
        .unwrap_or_default();
    let review_rows: Vec<portal_views::project_detail::ReviewDocRow<'_>> = review_docs
        .iter()
        .map(|d| portal_views::project_detail::ReviewDocRow {
            id: d.id,
            title: &d.title,
            kind: &d.kind,
            status: &d.status,
        })
        .collect();

    // Northstar: offer the "Approve my plan" control only when this is an
    // estate matter parked at client_review with every released draft
    // still awaiting the client (none approved yet — approve once).
    let show_approve_plan = crate::estate::transcript_driven_notation(&db, id)
        .await
        .is_some_and(|n| n.state == "client_review")
        && !review_docs.is_empty()
        && review_docs
            .iter()
            .all(|d| d.status == store::entity::review_document::STATUS_PENDING_REVIEW);

    // The matter's documents the client can take or ask to delete, each
    // flagged with whether a deletion request is already pending.
    let docs = document::Entity::find()
        .filter(document::Column::ProjectId.eq(id))
        .order_by_desc(document::Column::InsertedAt)
        .all(&db)
        .await
        .unwrap_or_default();
    let mut doc_rows: Vec<portal_views::project_detail::ClientDocRow<'_>> =
        Vec::with_capacity(docs.len());
    for d in &docs {
        let pending = store::expunge_requests::pending_for_document(&db, d.id)
            .await
            .unwrap_or(None)
            .is_some();
        doc_rows.push(portal_views::project_detail::ClientDocRow {
            id: d.id,
            filename: &d.filename,
            deletion_requested: pending,
        });
    }

    // The matter's invoice, read from the local Xero mirror (never Xero
    // live). Row-scoped by the visibility check above; the Xero invoice id
    // stays server-side — only the amount + status reach the client.
    let invoice_row = store::xero_invoices::for_projects(&db, &[id])
        .await
        .unwrap_or_default()
        .into_iter()
        .next();
    let invoice_amount = invoice_row.as_ref().map(|r| format_usd(r.amount_cents));
    let invoice = match (&invoice_row, &invoice_amount) {
        (Some(r), Some(amount)) => Some(portal_views::project_detail::InvoiceView {
            amount,
            status: &r.status,
            paid: r.amount_cents > 0 && r.amount_paid_cents >= r.amount_cents,
        }),
        _ => None,
    };

    portal_views::project_detail::render(&portal_views::project_detail::Detail {
        id: project.id,
        name: &project.name,
        status: &project.status,
        invoice,
        notations: &notation_rows,
        review_docs: &review_rows,
        documents: &doc_rows,
        csrf_token: &session.csrf_token,
        show_approve_plan,
    })
    .into_response()
}

/// Format integer cents as a US dollar amount with thousands separators,
/// e.g. `333_300` → `"$3,333.00"`. The portal shows the org's base
/// currency (USD); money never flows through a float.
fn format_usd(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let digits = (abs / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let grouped: String = grouped.chars().rev().collect();
    format!("${sign}{grouped}.{:02}", abs % 100)
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        views::not_found_page_with_auth(views::AuthState::Authenticated),
    )
        .into_response()
}
