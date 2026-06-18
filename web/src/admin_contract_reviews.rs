//! Attorney review surface for an inbound contract review —
//! `/portal/admin/contract-reviews/:id`.
//!
//! After the web-side analysis ([`crate::contract_review_walk`]) opens a
//! `contract_reviews` row of machine-proposed findings, the matter parks at
//! `staff_review`. Here a licensed attorney (`staff` tier — `staff` includes
//! attorneys) acts on the review:
//!
//! - **edits and decides each finding** — `attorney_note`, `suggested_redline`,
//!   `severity`, and an explicit *accept* or *reject*. There is **no
//!   bulk-accept**: every save is a per-finding decision, and nothing is
//!   accepted until the attorney acts. Each decision is written to
//!   `notation_events` (the immutable audit trail) so the memo is provably
//!   attorney-reviewed;
//! - **edits the risk summary**;
//! - **approves** — only once *every* finding has been acted on. Approval
//!   assembles the review memo from the exact signed-off findings + risk
//!   summary + the load-bearing disclaimers, renders it to a PDF filed into
//!   the Project, and drives the workflow `approved` →
//!   `document_open__review_memo` → `memo_rendered` → `END`;
//! - **rejects** — `staff_review --rejected--> END`, no memo.
//!
//! Authorization: the route lives under `/portal/admin/*`, so OPA's
//! `staff_tier` rule gates it; the handlers add a per-matter row scope (a
//! client role, or a staff member not disclosed to the project, gets `404`).

use axum::extract::{Extension, Form, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use std::collections::HashSet;
use uuid::Uuid;

use store::contract_reviews::{self, Finding};
use store::entity::{contract_review, notation, notation_event, person::Role, playbook};
use store::playbooks::{SEVERITY_HIGH, SEVERITY_LOW, SEVERITY_MEDIUM};
use store::Db;
use views::pages::admin::contract_reviews as views_reviews;
use workflows::{DocumentPayload, MachineKind, StateMachineRuntime};

use crate::admin::{csrf_token, AdminState};
use crate::session::SessionData;

/// `notation_events.machine_kind` token for the attorney's per-finding
/// decisions. A distinct kind so these attribution rows never participate in
/// the workflow / questionnaire state-projection reads.
pub const MACHINE_CONTRACT_REVIEW: &str = "contract_review";
const FINDING_ACCEPTED: &str = "finding_accepted";
const FINDING_REJECTED: &str = "finding_rejected";

/// Storage-key convention for a review memo PDF.
#[must_use]
pub fn memo_storage_key(notation_id: Uuid) -> String {
    format!("notations/{notation_id}/review-memo.pdf")
}

/// `documents.kind` the rendered memo is filed under in the Project.
const MEMO_KIND: &str = "review_memo";

// --- the loaded review + its matter ---------------------------------------

struct Loaded {
    review: contract_review::Model,
    notation: notation::Model,
    playbook: playbook::Model,
}

/// Load the review, its notation, and its playbook, enforcing the per-matter
/// row scope. Returns `Err(404)` for a missing review or a caller who may not
/// see the matter.
async fn load_scoped(
    db: &Db,
    review_id: Uuid,
    session: Option<&SessionData>,
) -> Result<Loaded, Response> {
    let Some(review) = contract_reviews::by_id(db, review_id).await.ok().flatten() else {
        return Err(not_found());
    };
    let Some(notation) = notation::Entity::find_by_id(review.notation_id)
        .one(db)
        .await
        .ok()
        .flatten()
    else {
        return Err(not_found());
    };
    // A client never reaches an admin surface; a staff member must be
    // disclosed to the matter (admin bypasses in `can_see_project`).
    let (person_id, role) = match session {
        Some(s) => (s.person_id, s.role),
        None => (None, Role::Staff),
    };
    if matches!(role, Role::Client) {
        return Err(not_found());
    }
    if !crate::access::can_see_project(db, person_id, role, notation.project_id)
        .await
        .unwrap_or(false)
    {
        return Err(not_found());
    }
    let Some(playbook) = playbook::Entity::find_by_id(review.playbook_id)
        .one(db)
        .await
        .ok()
        .flatten()
    else {
        return Err(not_found());
    };
    Ok(Loaded {
        review,
        notation,
        playbook,
    })
}

/// `GET /portal/admin/contract-reviews/:id` — the attorney review screen.
pub async fn show(
    State(state): State<AdminState>,
    Path(review_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let session = session.map(|Extension(s)| s);
    let loaded = match load_scoped(&state.db, review_id, session.as_ref()).await {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    render_review(&state.db, &loaded, csrf_token(session.as_ref())).await
}

#[derive(Deserialize)]
pub struct FindingEdit {
    /// `accept` or `reject` — the submit button the attorney clicked.
    decision: String,
    severity: String,
    suggested_redline: String,
    attorney_note: String,
}

/// `POST /portal/admin/contract-reviews/:id/findings/:idx` — save the edits
/// to one finding and record the accept / reject decision.
pub async fn save_finding(
    State(state): State<AdminState>,
    Path((review_id, idx)): Path<(Uuid, usize)>,
    session: Option<Extension<SessionData>>,
    Form(input): Form<FindingEdit>,
) -> Response {
    let session = session.map(|Extension(s)| s);
    let loaded = match load_scoped(&state.db, review_id, session.as_ref()).await {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    if !matches!(
        loaded.review.status.as_str(),
        contract_review::STATUS_ANALYZED
    ) {
        // Only an open (analyzed) review takes edits.
        return redirect_to(review_id);
    }
    let mut findings = contract_reviews::findings_of(&loaded.review).unwrap_or_default();
    let Some(finding) = findings.get_mut(idx) else {
        return not_found();
    };
    let accepted = input.decision == "accept";
    finding.accepted = accepted;
    finding.attorney_note = non_empty(&input.attorney_note);
    finding.suggested_redline = non_empty(&input.suggested_redline);
    if is_severity(&input.severity) {
        finding.severity = input.severity.to_lowercase();
    }
    let clause_ref = finding.clause_ref.clone();

    if let Err(e) = contract_reviews::update_findings(&state.db, review_id, &findings).await {
        tracing::error!(error = %e, %review_id, idx, "save finding failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }
    // Immutable per-finding attribution — who decided what, when.
    record_finding_decision(
        &state.db,
        loaded.notation.id,
        idx,
        &clause_ref,
        accepted,
        session.as_ref(),
    )
    .await;
    redirect_to(review_id)
}

#[derive(Deserialize)]
pub struct SummaryEdit {
    risk_summary: String,
}

/// `POST /portal/admin/contract-reviews/:id/summary` — edit the risk summary.
pub async fn save_summary(
    State(state): State<AdminState>,
    Path(review_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
    Form(input): Form<SummaryEdit>,
) -> Response {
    let session = session.map(|Extension(s)| s);
    if let Err(resp) = load_scoped(&state.db, review_id, session.as_ref()).await {
        return resp;
    }
    if let Err(e) =
        contract_reviews::update_risk_summary(&state.db, review_id, input.risk_summary.trim()).await
    {
        tracing::error!(error = %e, %review_id, "save risk summary failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }
    redirect_to(review_id)
}

/// `POST /portal/admin/contract-reviews/:id/approve` — assemble + deliver the
/// memo and approve.
pub async fn approve(
    State(state): State<AdminState>,
    Path(review_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let session = session.map(|Extension(s)| s);
    let loaded = match load_scoped(&state.db, review_id, session.as_ref()).await {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    if loaded.notation.state != "staff_review" {
        return redirect_to(review_id);
    }
    let findings = contract_reviews::findings_of(&loaded.review).unwrap_or_default();
    // Force per-finding action: every finding must have a recorded decision.
    let acted = acted_indices(&state.db, loaded.notation.id).await;
    if (0..findings.len()).any(|i| !acted.contains(&i)) {
        return render_review_with_error(
            &state.db,
            &loaded,
            csrf_token(session.as_ref()),
            "Every finding must be accepted or rejected before the memo can be approved.",
        )
        .await;
    }

    match deliver_memo(&state, &loaded, &findings).await {
        Ok(()) => redirect_to(review_id),
        Err(e) => {
            tracing::error!(error = %e, %review_id, "approve / memo delivery failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response()
        }
    }
}

/// `POST /portal/admin/contract-reviews/:id/reject` — reject without a memo.
pub async fn reject(
    State(state): State<AdminState>,
    Path(review_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let session = session.map(|Extension(s)| s);
    let loaded = match load_scoped(&state.db, review_id, session.as_ref()).await {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    if loaded.notation.state != "staff_review" {
        return redirect_to(review_id);
    }
    let next = match StateMachineRuntime::signal(
        state.workflow_runtime.as_ref(),
        MachineKind::Workflow,
        loaded.notation.id,
        "rejected",
        None,
    )
    .await
    {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(error = %e, %review_id, "reject signal failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };
    let _ = sync_notation_state(&state.db, loaded.notation.id, next.as_str()).await;
    let _ =
        contract_reviews::set_status(&state.db, review_id, contract_review::STATUS_REJECTED).await;
    redirect_to(review_id)
}

// --- memo delivery ---------------------------------------------------------

/// Assemble the memo from the exact signed-off findings, render + file it into
/// the Project, and drive the workflow to `END`.
async fn deliver_memo(
    state: &AdminState,
    loaded: &Loaded,
    findings: &[Finding],
) -> anyhow::Result<()> {
    let notation_id = loaded.notation.id;
    let risk_summary = loaded.review.risk_summary.clone().unwrap_or_default();
    let accepted: Vec<&Finding> = findings.iter().filter(|f| f.accepted).collect();
    let typst_source = assemble_memo_typst(&MemoInput {
        playbook_name: &loaded.playbook.name,
        risk_summary: &risk_summary,
        accepted_findings: &accepted,
    });

    // Render web-side and file the PDF into the Project (a `documents` row +
    // git commit, the per-Project system of record). The worker also persists
    // it to the storage key on the `document_open__review_memo` step below —
    // the two writes are idempotent (same Typst → same bytes).
    let bytes = pdf::render(&typst_source)?;
    let args = store::documents::IngestArgs {
        project_id: loaded.notation.project_id,
        source: store::documents::source::UPLOAD,
        filename: "review-memo.pdf",
        kind: MEMO_KIND,
        content_type: "application/pdf",
        description: Some("Inbound contract review memo"),
        source_revision_id: None,
    };
    store::documents::ingest_bytes(&state.db, &state.storage, &args, &bytes).await?;

    // approved → document_open__review_memo (worker renders + persists),
    // then memo_rendered → END.
    let payload = serde_json::to_string(&DocumentPayload::Typst {
        storage_key: memo_storage_key(notation_id),
        typst_source,
    })?;
    let runtime = state.workflow_runtime.as_ref();
    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "approved",
        Some(&payload),
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;
    contract_reviews::set_status(
        &state.db,
        loaded.review.id,
        contract_review::STATUS_APPROVED,
    )
    .await?;
    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "memo_rendered",
        None,
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;
    Ok(())
}

/// What the memo is assembled from.
pub struct MemoInput<'a> {
    pub playbook_name: &'a str,
    pub risk_summary: &'a str,
    pub accepted_findings: &'a [&'a Finding],
}

/// Assemble the review-memo Typst source from the signed-off findings + risk
/// summary + the load-bearing disclaimers (named playbook; not a full audit;
/// attorney accountable; zero-retention AI). Every dynamic value is inserted
/// as a Typst string literal (`#"…"`) so arbitrary attorney/finding text can
/// never break the markup.
#[must_use]
pub fn assemble_memo_typst(input: &MemoInput<'_>) -> String {
    let mut out = String::new();
    out.push_str("#set page(paper: \"us-letter\", margin: 1in)\n");
    out.push_str("#set text(size: 11pt)\n");
    out.push_str("#set par(justify: true)\n\n");
    out.push_str(
        "#align(center)[#text(size: 16pt, weight: \"bold\")[Inbound Contract Review Memo]]\n\n",
    );
    out.push_str("*Measured against playbook:* ");
    out.push_str(&typ_str(input.playbook_name));
    out.push_str("\n\n== Risk summary\n");
    out.push_str(&typ_str(input.risk_summary));
    out.push_str("\n\n== Findings\n");
    if input.accepted_findings.is_empty() {
        out.push_str(&typ_str(
            "No deviations were flagged for delivery against this playbook.",
        ));
        out.push('\n');
    } else {
        for f in input.accepted_findings {
            out.push_str("\n=== ");
            out.push_str(&typ_str(&f.clause_ref));
            out.push_str(" — ");
            out.push_str(&typ_str(&severity_label(&f.severity)));
            out.push_str("\n\n");
            out.push_str(&typ_str(&f.deviation));
            out.push_str("\n\n");
            if let Some(redline) = f.suggested_redline.as_deref().filter(|s| !s.is_empty()) {
                out.push_str("*Suggested redline:* ");
                out.push_str(&typ_str(redline));
                out.push_str("\n\n");
            }
            if let Some(note) = f.attorney_note.as_deref().filter(|s| !s.is_empty()) {
                out.push_str("*Attorney note:* ");
                out.push_str(&typ_str(note));
                out.push_str("\n\n");
            }
        }
    }
    out.push_str("\n== Scope and disclaimers\n");
    out.push_str("This memo measures the contract against the ");
    out.push_str(&typ_str(input.playbook_name));
    out.push_str(
        " playbook only — it is not a full audit. A clause this memo does not flag is not \
         thereby approved; silence means the clause was outside the playbook's scope. A \
         licensed Neon Law attorney has reviewed and is accountable for every finding above. \
         To produce the review, the contract text was processed through a zero-retention AI \
         service that is not trained on Company data; the contract and this memo are \
         confidential.\n",
    );
    out
}

/// Insert `s` as a Typst string-literal expression (`#"…"`), escaping the two
/// characters that are significant inside a Typst string. In content (markup)
/// context this displays the string's characters verbatim, with no markup
/// interpretation — so arbitrary text is safe.
fn typ_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("#\"{escaped}\"")
}

fn severity_label(severity: &str) -> String {
    match severity {
        SEVERITY_HIGH => "High severity".to_string(),
        SEVERITY_MEDIUM => "Medium severity".to_string(),
        SEVERITY_LOW => "Low severity".to_string(),
        other => other.to_string(),
    }
}

// --- attribution -----------------------------------------------------------

/// Append one immutable per-finding decision to `notation_events`.
async fn record_finding_decision(
    db: &Db,
    notation_id: Uuid,
    idx: usize,
    clause_ref: &str,
    accepted: bool,
    session: Option<&SessionData>,
) {
    let by = session
        .and_then(|s| s.person_id.map(|p| p.to_string()))
        .unwrap_or_else(|| "unknown".to_string());
    let payload = serde_json::json!({
        "index": idx,
        "clause_ref": clause_ref,
        "accepted": accepted,
        "by_person_id": by,
    })
    .to_string();
    let condition = if accepted {
        FINDING_ACCEPTED
    } else {
        FINDING_REJECTED
    };
    let active = notation_event::ActiveModel {
        notation_id: ActiveValue::Set(notation_id),
        machine_kind: ActiveValue::Set(MACHINE_CONTRACT_REVIEW.to_string()),
        from_state: ActiveValue::Set("staff_review".to_string()),
        to_state: ActiveValue::Set("staff_review".to_string()),
        condition: ActiveValue::Set(condition.to_string()),
        payload: ActiveValue::Set(Some(payload)),
        recorded_at: ActiveValue::Set(chrono::Utc::now().to_rfc3339()),
        ..Default::default()
    };
    if let Err(e) = active.insert(db).await {
        tracing::error!(error = %e, %notation_id, idx, "record finding decision failed");
    }
}

/// The set of finding indices that have a recorded accept / reject decision.
async fn acted_indices(db: &Db, notation_id: Uuid) -> HashSet<usize> {
    let events = notation_event::Entity::find()
        .filter(notation_event::Column::NotationId.eq(notation_id))
        .filter(notation_event::Column::MachineKind.eq(MACHINE_CONTRACT_REVIEW))
        .all(db)
        .await
        .unwrap_or_default();
    events
        .iter()
        .filter_map(|e| {
            e.payload
                .as_deref()
                .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
                .and_then(|v| v.get("index").and_then(serde_json::Value::as_u64))
                .and_then(|i| usize::try_from(i).ok())
        })
        .collect()
}

// --- rendering -------------------------------------------------------------

async fn render_review(db: &Db, loaded: &Loaded, csrf: &str) -> Response {
    render_review_inner(db, loaded, csrf, None).await
}

async fn render_review_with_error(db: &Db, loaded: &Loaded, csrf: &str, error: &str) -> Response {
    render_review_inner(db, loaded, csrf, Some(error)).await
}

async fn render_review_inner(
    db: &Db,
    loaded: &Loaded,
    csrf: &str,
    error: Option<&str>,
) -> Response {
    let findings = contract_reviews::findings_of(&loaded.review).unwrap_or_default();
    let acted = acted_indices(db, loaded.notation.id).await;
    let finding_views: Vec<views_reviews::FindingView<'_>> = findings
        .iter()
        .enumerate()
        .map(|(i, f)| views_reviews::FindingView {
            index: i,
            clause_ref: &f.clause_ref,
            deviation: &f.deviation,
            severity: &f.severity,
            suggested_redline: f.suggested_redline.as_deref().unwrap_or(""),
            attorney_note: f.attorney_note.as_deref().unwrap_or(""),
            accepted: f.accepted,
            acted: acted.contains(&i),
        })
        .collect();
    let all_acted = (0..findings.len()).all(|i| acted.contains(&i));
    let view = views_reviews::ReviewView {
        review_id: loaded.review.id,
        playbook_name: &loaded.playbook.name,
        status: &loaded.review.status,
        notation_state: &loaded.notation.state,
        risk_summary: loaded.review.risk_summary.as_deref().unwrap_or(""),
        findings: finding_views,
        all_acted,
        error,
        csrf_token: csrf,
    };
    views_reviews::review_page(&view).into_response()
}

// --- small helpers ---------------------------------------------------------

fn redirect_to(review_id: Uuid) -> Response {
    Redirect::to(&format!("/portal/admin/contract-reviews/{review_id}")).into_response()
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn is_severity(s: &str) -> bool {
    matches!(
        s.to_lowercase().as_str(),
        SEVERITY_LOW | SEVERITY_MEDIUM | SEVERITY_HIGH
    )
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        views::not_found_page_with_auth(views::AuthState::Authenticated),
    )
        .into_response()
}

async fn sync_notation_state(
    db: &Db,
    notation_id: Uuid,
    new_state: &str,
) -> Result<(), sea_orm::DbErr> {
    use sea_orm::ActiveModelTrait;
    let existing = notation::Entity::find_by_id(notation_id)
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::RecordNotFound(format!("notation {notation_id}")))?;
    let mut active: notation::ActiveModel = existing.into();
    active.state = ActiveValue::Set(new_state.to_string());
    active.update(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{assemble_memo_typst, typ_str, MemoInput};
    use store::contract_reviews::Finding;
    use store::playbooks::SEVERITY_HIGH;

    fn finding(clause: &str, deviation: &str) -> Finding {
        Finding {
            clause_ref: clause.into(),
            deviation: deviation.into(),
            severity: SEVERITY_HIGH.into(),
            suggested_redline: Some("Add a mutual cap.".into()),
            attorney_note: Some("Push this.".into()),
            accepted: true,
        }
    }

    #[test]
    fn typ_str_escapes_quotes_and_backslashes() {
        assert_eq!(typ_str("a\"b\\c"), "#\"a\\\"b\\\\c\"");
    }

    #[test]
    fn memo_includes_playbook_summary_and_findings_and_disclaimers() {
        let f = finding("§7.2 Liability", "Liability is uncapped.");
        let refs = [&f];
        let typ = assemble_memo_typst(&MemoInput {
            playbook_name: "Vendor MSA",
            risk_summary: "One high-severity deviation.",
            accepted_findings: &refs,
        });
        assert!(typ.contains("Vendor MSA"));
        assert!(typ.contains("One high-severity deviation."));
        assert!(typ.contains("§7.2 Liability"));
        assert!(typ.contains("Suggested redline:"));
        assert!(typ.contains("not a full audit"));
        assert!(typ.contains("zero-retention AI"));
    }

    #[test]
    fn memo_with_markup_chars_renders_to_a_real_pdf() {
        // Arbitrary attorney text full of Typst metacharacters must not break
        // the compile — proving the `#"…"` insertion is safe.
        let f = Finding {
            clause_ref: "#1 *Indemnity* [draft]".into(),
            deviation: "Caps at $0; see § 9.1 _and_ <Exhibit A> @ref `code`.".into(),
            severity: SEVERITY_HIGH.into(),
            suggested_redline: Some("Replace with = mutual cap #here".into()),
            attorney_note: None,
            accepted: true,
        };
        let refs = [&f];
        let typ = assemble_memo_typst(&MemoInput {
            playbook_name: "Edge \"quoted\" \\ playbook",
            risk_summary: "Summary with # and * and $.",
            accepted_findings: &refs,
        });
        let pdf = pdf::render(&typ).expect("memo Typst compiles to a PDF");
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn memo_with_no_accepted_findings_states_so() {
        let typ = assemble_memo_typst(&MemoInput {
            playbook_name: "P",
            risk_summary: "Nothing material.",
            accepted_findings: &[],
        });
        assert!(typ.contains("No deviations were flagged"));
        assert!(pdf::render(&typ).is_ok());
    }
}
