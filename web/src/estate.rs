//! Northstar estate pipeline — transcript → answers → review drafts.
//!
//! After the recorded sitting's transcript is filed
//! (`document_intake__transcript`), the matter has to turn that
//! transcript into the attorney-reviewable drafts the Phase A surface
//! renders. This module drives that, web-side, the same way the retainer
//! renders its document web-side (`retainer_walk`):
//!
//!   transcript_ready → extract__inputs   (write `answers`, source `extracted`)
//!   inputs_ready     → document_drafts__estate (render instruments → review_documents)
//!   drafts_persisted → staff_review      (the attorney gate)
//!
//! Extraction is a seam: [`EstateExtractor`] maps a transcript onto the
//! estate question codes. [`StubEstateExtractor`] ships now (deterministic
//! `Label: value` scanning, ~$0); the AIDA/Gemini Enterprise extractor
//! swaps in behind the same trait later. Machine-proposed answers are
//! written with `source = extracted` so an attorney can see and correct
//! them before any draft leaves `draft` — the human-in-the-loop boundary.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;
use workflows::{MachineKind, StateMachineRuntime};

use crate::admin::AdminState;
use crate::session::SessionData;
use store::entity::review_document::{STATUS_DRAFT, STATUS_PENDING_REVIEW};
use store::entity::{answer, notation, question};

/// One estate instrument: the catalog code of its template stub and the
/// `review_documents.kind` its rendered draft is filed under. The two
/// directives are separate rows so the client can comment on each
/// independently; the review listing groups them under one heading.
struct Instrument {
    template_code: &'static str,
    kind: &'static str,
}

const ESTATE_INSTRUMENTS: &[Instrument] = &[
    Instrument {
        template_code: "northstar__will",
        kind: "will",
    },
    Instrument {
        template_code: "northstar__trust",
        kind: "trust",
    },
    Instrument {
        template_code: "northstar__directive_health",
        kind: "directive_health",
    },
    Instrument {
        template_code: "northstar__directive_financial",
        kind: "directive_financial",
    },
];

/// Maps a recorded sitting's transcript onto answers to the estate
/// question set. The seam the Gemini extractor swaps in behind.
pub trait EstateExtractor: Send + Sync {
    /// Return `(question_code, value)` pairs for whatever the transcript
    /// answers. Codes it can't find are simply absent (a coverage gap),
    /// never an error.
    fn extract(&self, transcript: &str) -> Vec<(String, String)>;
}

/// Deterministic, dependency-free extractor: scans the transcript for
/// `Label: value` segments (value runs to the next `.`, `;`, or newline),
/// one set of labels per estate question code. Good enough to drive the
/// full pipeline and the demo at ~$0; the real extractor is AIDA on the
/// already-paid Gemini Enterprise, behind the same trait.
pub struct StubEstateExtractor;

/// `(state_name, &[label aliases])`. The first label found wins. The state
/// name matches the `custom_*__<role>` placeholders in the northstar
/// instrument bodies and disambiguates the several roles that share one
/// registry question.
const STUB_LABELS: &[(&str, &[&str])] = &[
    (
        "custom_text__testator_name",
        &["testator", "full legal name", "my name is"],
    ),
    ("custom_text__executor_name", &["executor"]),
    (
        "custom_text__successor_trustee",
        &["successor trustee", "trustee"],
    ),
    ("custom_text__guardian_for_minors", &["guardian"]),
    (
        "custom_text__residuary_beneficiary",
        &["residuary beneficiary", "beneficiary"],
    ),
    (
        "custom_text__healthcare_agent",
        &["health-care agent", "healthcare agent", "health care agent"],
    ),
    ("custom_text__financial_agent", &["financial agent"]),
];

impl EstateExtractor for StubEstateExtractor {
    fn extract(&self, transcript: &str) -> Vec<(String, String)> {
        let lower = transcript.to_lowercase();
        let mut out = Vec::new();

        // Two-party-consent confirmation: any mention of consent.
        if lower.contains("consent") {
            out.push((
                "custom_yes_no__recording_consent".to_string(),
                "Yes".to_string(),
            ));
        }

        for (code, labels) in STUB_LABELS {
            if let Some(value) = labels
                .iter()
                .find_map(|label| value_after_label(&lower, transcript, label))
            {
                out.push(((*code).to_string(), value));
            }
        }
        out
    }
}

/// Find `label:` in `lower` (lowercased haystack), then slice the
/// corresponding span out of the original `transcript`, taking the text
/// after the colon up to the next sentence/segment break. Returns the
/// trimmed value if non-empty.
fn value_after_label(lower: &str, transcript: &str, label: &str) -> Option<String> {
    let needle = format!("{label}:");
    let at = lower.find(&needle)?;
    let start = at + needle.len();
    let tail = &transcript[start..];
    let end = tail.find(['.', ';', '\n']).unwrap_or(tail.len());
    let value = tail[..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Which estate questions the sitting answered vs. left open. Surfaced so
/// staff know what to follow up on before releasing drafts to the client.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CoverageReport {
    /// Instrument fields the transcript answered (a non-empty value).
    pub answered: Vec<String>,
    /// Instrument fields the transcript left unanswered — the drafts
    /// render with a blank in their place until staff fill them.
    pub unanswered: Vec<String>,
}

/// Failure of the estate pipeline. The caller (the transcript handler)
/// logs this and still redirects: the transcript is already filed, so a
/// pipeline hiccup must not 500 the staff upload.
#[derive(Debug, thiserror::Error)]
pub enum EstatePipelineError {
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("workflow runtime: {0}")]
    Runtime(#[from] workflows::WorkflowRuntimeError),
    #[error("template body: {0}")]
    TemplateBody(#[from] store::templates::TemplateBodyError),
    #[error("notation {0} vanished mid-pipeline")]
    NotationMissing(Uuid),
}

/// Drive the estate pipeline from a freshly-filed transcript through to
/// the attorney gate (`staff_review`), returning the coverage report.
pub async fn drive_estate_pipeline(
    state: &AdminState,
    notation_id: Uuid,
    transcript: &str,
    extractor: &dyn EstateExtractor,
) -> Result<CoverageReport, EstatePipelineError> {
    let runtime = state.workflow_runtime.as_ref();
    let notation_row = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await?
        .ok_or(EstatePipelineError::NotationMissing(notation_id))?;
    let respondent_id = notation_row.person_id;

    // transcript_ready → extract__inputs.
    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "transcript_ready",
        None,
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;

    // Extract and persist the answers (source = extracted).
    let extracted = extractor.extract(transcript);
    let mut answers: BTreeMap<String, String> = BTreeMap::new();
    for (code, value) in extracted {
        if value.trim().is_empty() {
            continue;
        }
        write_extracted_answer(&state.db, notation_id, respondent_id, &code, &value).await?;
        answers.insert(code, value);
    }

    // inputs_ready → document_drafts__estate.
    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "inputs_ready",
        None,
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;

    // Render each instrument from the answers into one review_documents
    // row at `draft` (hidden from the client until an attorney advances
    // it past draft). Track which fields the drafts needed and which the
    // sitting actually answered.
    let mut needed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for inst in ESTATE_INSTRUMENTS {
        let Some(template) = store::templates::resolve(&state.db, None, inst.template_code).await?
        else {
            tracing::warn!(
                code = inst.template_code,
                "estate instrument template not seeded — skipping draft"
            );
            continue;
        };
        let body = store::templates::body(&state.db, &state.storage, &template).await?;
        for code in data_placeholders(&body) {
            needed.insert(code);
        }
        let rendered = views::markdown::render(&substitute(&body, &answers)).into_string();
        store::review_documents::create(
            &state.db,
            &store::review_documents::NewReviewDocument {
                notation_id,
                kind: inst.kind,
                title: &template.title,
                body_html: &rendered,
            },
        )
        .await?;
    }

    // drafts_persisted → staff_review (the attorney gate).
    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "drafts_persisted",
        None,
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;

    let mut report = CoverageReport::default();
    for code in needed {
        if answers.contains_key(&code) {
            report.answered.push(code);
        } else {
            report.unanswered.push(code);
        }
    }
    tracing::info!(
        %notation_id,
        answered = report.answered.len(),
        unanswered = ?report.unanswered,
        "estate pipeline: drafts rendered, coverage computed"
    );
    Ok(report)
}

/// Find the project's transcript-driven onboarding notation — the
/// Northstar estate matter. Data-driven, never a hard-coded template
/// code: a notation qualifies when its bound template's workflow has a
/// `transcript_uploaded` edge out of `BEGIN` (the signal the creation
/// flow, the transcript handler, and the matter page all key off).
pub async fn transcript_driven_notation(
    db: &store::Db,
    project_id: Uuid,
) -> Option<notation::Model> {
    use store::entity::template;
    let notations = notation::Entity::find()
        .filter(notation::Column::ProjectId.eq(project_id))
        .all(db)
        .await
        .unwrap_or_default();
    for n in notations {
        let Some(t) = template::Entity::find_by_id(n.template_id)
            .one(db)
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        let transcript_driven = workflows::bundled_spec_yaml(&t.code)
            .and_then(|yaml| workflows::workflow_spec_from_yaml(yaml).ok())
            .is_some_and(|spec| {
                spec.transitions_from(&workflows::StateName::begin())
                    .is_some_and(|tm| {
                        tm.lookup(crate::transcript_intake::TRANSCRIPT_UPLOADED)
                            .is_some()
                    })
            });
        if transcript_driven {
            return Some(n);
        }
    }
    None
}

/// `POST /portal/projects/:id/approve-plan` — the client approves the plan.
///
/// The mirror of the staff release: at `client_review`, the client (or a
/// staff/admin acting on the matter) fires `client_approved`, advancing
/// `client_review --client_approved--> sent_for_signature__pending` and
/// flipping every released draft from `pending_review` to `approved`.
///
/// The substantive gate OPA can't see (no DB state) lives here and 404s
/// otherwise: the caller must see the matter, the matter must be at
/// `client_review`, and **every** draft must already be `pending_review`
/// (released by an attorney, and not already approved — approve only once).
pub async fn approve_plan_post(
    State(state): State<AdminState>,
    AxumPath(project_id): AxumPath<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let Some(Extension(session)) = session else {
        return not_found();
    };
    let Some(person_id) = session.person_id else {
        return not_found();
    };
    if !crate::access::can_see_project(&state.db, Some(person_id), session.role, project_id)
        .await
        .unwrap_or(false)
    {
        return not_found();
    }
    let Some(notation_row) = transcript_driven_notation(&state.db, project_id).await else {
        return not_found();
    };
    if notation_row.state != "client_review" {
        return not_found();
    }
    // Approve only once every draft has been released to pending_review.
    let docs = store::review_documents::for_notation(&state.db, notation_row.id)
        .await
        .unwrap_or_default();
    if docs.is_empty() || docs.iter().any(|d| d.status != STATUS_PENDING_REVIEW) {
        return not_found();
    }

    match StateMachineRuntime::signal(
        state.workflow_runtime.as_ref(),
        MachineKind::Workflow,
        notation_row.id,
        "client_approved",
        None,
    )
    .await
    {
        Ok(next) => {
            if let Err(e) = sync_notation_state(&state.db, notation_row.id, next.as_str()).await {
                tracing::warn!(error = %e, notation_id = %notation_row.id, "approve-plan: state sync failed");
            }
        }
        Err(e) => {
            tracing::error!(error = %e, notation_id = %notation_row.id, "approve-plan: client_approved signal failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    }
    if let Err(e) = advance_drafts(
        &state.db,
        notation_row.id,
        STATUS_PENDING_REVIEW,
        store::entity::review_document::STATUS_APPROVED,
    )
    .await
    {
        tracing::error!(error = %e, notation_id = %notation_row.id, "approve-plan: status flip failed");
    }
    Redirect::to(&format!("/portal/projects/{project_id}")).into_response()
}

/// `POST /portal/admin/notations/:id/release-drafts` — the attorney gate.
///
/// At `staff_review`, a staff member disclosed to the matter approves the
/// generated drafts: this advances `staff_review --approved--> client_review`
/// and flips every `draft` instrument to `pending_review`, which is what
/// makes it visible to the client on the Phase A review surface. No
/// client-facing auto-generated legal document leaves `draft` without this
/// human step. Row-scoped: a non-participant (non-admin) gets `404`.
pub async fn release_drafts_post(
    State(state): State<AdminState>,
    AxumPath(notation_id): AxumPath<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let Some(notation_row) = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await
        .ok()
        .flatten()
    else {
        return not_found();
    };
    // Row-scope to the matter: admin bypasses, staff must be disclosed.
    let (person_id, role) = match session.as_deref() {
        Some(s) => (s.person_id, s.role),
        None => (None, store::entity::person::Role::Staff),
    };
    if matches!(role, store::entity::person::Role::Client) {
        return not_found();
    }
    if !crate::access::can_see_project(&state.db, person_id, role, notation_row.project_id)
        .await
        .unwrap_or(false)
    {
        return not_found();
    }

    // Advance the matter to the client-review gate, then release each
    // draft. Order matters only for the durable state; the row flips are
    // idempotent.
    match StateMachineRuntime::signal(
        state.workflow_runtime.as_ref(),
        MachineKind::Workflow,
        notation_id,
        "approved",
        None,
    )
    .await
    {
        Ok(next) => {
            if let Err(e) = sync_notation_state(&state.db, notation_id, next.as_str()).await {
                tracing::warn!(error = %e, %notation_id, "release-drafts: state sync failed");
            }
        }
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "release-drafts: approve signal failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    }
    if let Err(e) =
        advance_drafts(&state.db, notation_id, STATUS_DRAFT, STATUS_PENDING_REVIEW).await
    {
        tracing::error!(error = %e, %notation_id, "release-drafts: status flip failed");
    }
    Redirect::to(&format!("/portal/projects/{}", notation_row.project_id)).into_response()
}

/// Flip every review document on the notation from `from` to `to`.
async fn advance_drafts(
    db: &store::Db,
    notation_id: Uuid,
    from: &str,
    to: &str,
) -> Result<(), sea_orm::DbErr> {
    let docs = store::review_documents::for_notation(db, notation_id).await?;
    for d in docs {
        if d.status == from {
            store::review_documents::set_status(db, d.id, to).await?;
        }
    }
    Ok(())
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        views::not_found_page_with_auth(views::AuthState::Authenticated),
    )
        .into_response()
}

/// Insert one machine-extracted answer for the respondent, keyed by the
/// questionnaire `state_name` (`custom_text__testator_name`). The registry
/// question is resolved from the typed prefix before `__`. No-op when the
/// question code isn't seeded (the suite-coverage test guarantees the
/// estate codes are, so this only guards against drift).
async fn write_extracted_answer(
    db: &store::Db,
    notation_id: Uuid,
    respondent_id: Uuid,
    state_name: &str,
    value: &str,
) -> Result<(), sea_orm::DbErr> {
    let registry_code = state_name
        .split_once("__")
        .map_or(state_name, |(code, _)| code);
    let Some(q) = question::Entity::find()
        .filter(question::Column::Code.eq(registry_code))
        .one(db)
        .await?
    else {
        tracing::warn!(
            code = registry_code,
            "extracted answer for an unseeded question code — skipped"
        );
        return Ok(());
    };
    answer::ActiveModel {
        question_id: ActiveValue::Set(q.id),
        person_id: ActiveValue::Set(respondent_id),
        notation_id: ActiveValue::Set(Some(notation_id)),
        state_name: ActiveValue::Set(Some(state_name.to_string())),
        value: ActiveValue::Set(answer::primitive(value)),
        source: ActiveValue::Set(answer::SOURCE_EXTRACTED.to_string()),
        authored_by_person_id: ActiveValue::Set(None),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

/// Substitute `{{code}}` placeholders in a template body with answer
/// values, leaving an unanswered placeholder as a visible blank.
fn substitute(body: &str, answers: &BTreeMap<String, String>) -> String {
    let mut out = body.to_string();
    for code in data_placeholders(body) {
        let value = answers.get(&code).map_or("________", String::as_str);
        out = out.replace(&format!("{{{{{code}}}}}"), value);
    }
    out
}

/// Every `{{ … }}` data placeholder (no `.`, i.e. not a signature anchor).
fn data_placeholders(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let token = after[..close].trim();
        if !token.is_empty() && !token.contains('.') && !out.iter().any(|c| c == token) {
            out.push(token.to_string());
        }
        rest = &after[close + 2..];
    }
    out
}

/// Mirror the runtime's resulting state onto the `notations` row.
async fn sync_notation_state(
    db: &store::Db,
    notation_id: Uuid,
    new_state: &str,
) -> Result<(), sea_orm::DbErr> {
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
    use super::{data_placeholders, substitute, EstateExtractor, StubEstateExtractor};
    use std::collections::BTreeMap;

    #[test]
    fn stub_extracts_labelled_values_from_a_sentence_transcript() {
        let t = "Consent recorded. Executor: Aries. Successor trustee: Capricorn. \
                 Residuary beneficiary: Gemini.";
        let pairs = StubEstateExtractor.extract(t);
        let map: BTreeMap<_, _> = pairs.into_iter().collect();
        assert_eq!(
            map.get("custom_yes_no__recording_consent")
                .map(String::as_str),
            Some("Yes")
        );
        assert_eq!(
            map.get("custom_text__executor_name").map(String::as_str),
            Some("Aries")
        );
        assert_eq!(
            map.get("custom_text__successor_trustee")
                .map(String::as_str),
            Some("Capricorn")
        );
        assert_eq!(
            map.get("custom_text__residuary_beneficiary")
                .map(String::as_str),
            Some("Gemini")
        );
        // Nothing said about a financial agent → absent (a coverage gap).
        assert!(!map.contains_key("custom_text__financial_agent"));
    }

    #[test]
    fn substitute_fills_known_codes_and_blanks_the_rest() {
        let body = "Executor {{custom_text__executor_name}} and agent {{custom_text__financial_agent}}.";
        let mut answers = BTreeMap::new();
        answers.insert("custom_text__executor_name".to_string(), "Aries".to_string());
        let out = substitute(body, &answers);
        assert!(out.contains("Executor Aries"));
        assert!(out.contains("________"));
        assert!(!out.contains("{{"));
    }

    #[test]
    fn data_placeholders_skips_signature_anchors() {
        let codes =
            data_placeholders("{{custom_text__testator_name}} signs {{client.signature}} once.");
        assert_eq!(codes, vec!["custom_text__testator_name".to_string()]);
    }
}
