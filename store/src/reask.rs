//! Re-ask journal — which answers a `lawyer_review` flagged for
//! re-collection, recorded as an attributed, append-only event.
//!
//! When a `lawyer_review` returns `changes_requested`, the notation parks at
//! `reask__client` and the flagged question codes (plus an optional reviewer
//! note) are recorded here under a distinct `machine_kind` (`reask`) — so the
//! row never participates in the workflow / questionnaire state-projection
//! reads, exactly as the contract-review per-finding decisions do
//! ([`crate::contract_reviews`]). The re-ask surface reads the latest flagged
//! set, and answer re-collection is gated to it so only the flagged questions
//! can be re-answered — a rejected review re-collects the wrong answers, never
//! the whole questionnaire (issue #252).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::notation_events::{append_event, latest_for_kind, TransitionRecord};
use crate::surreal::SurrealDb;

/// `notation_events.machine_kind` token for a re-ask change request. A
/// distinct kind so these rows never participate in the workflow /
/// questionnaire state projections (mirrors the contract-review decisions).
pub const MACHINE_REASK: &str = "reask";
/// The condition recorded on a change-request event.
pub const CONDITION_CHANGES_REQUESTED: &str = "changes_requested";
/// The workflow state a change request parks the notation at.
pub const REASK_STATE: &str = "reask__client";
/// The `lawyer_review` state a change request leaves and, after
/// re-collection, returns to.
pub const REVIEW_STATE: &str = "lawyer_review";

/// Errors reading or writing the re-ask journal.
#[derive(Debug, thiserror::Error)]
pub enum ReaskError {
    #[error("notation event: {0}")]
    NotationEvent(#[from] crate::notation_events::NotationEventError),
    #[error("encoding change request: {0}")]
    Encode(String),
}

/// The flagged answers and reviewer note carried on a change-request event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeRequest {
    /// Question codes the `lawyer_review` flagged to be re-collected.
    pub flagged_questions: Vec<String>,
    /// Optional reviewer note — what to fix. Lawyer work product; part of
    /// the matter file, like an answer value, so it lives on the journal
    /// (which is never logged or traced).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Record a `lawyer_review` change request: append an attributed `reask`
/// event carrying the flagged question codes and note. The journal is
/// append-only, so each call is one recorded round; [`latest_change_request`]
/// projects the current (latest) one.
///
/// # Errors
/// Fails if the flagged set can't be encoded or the row can't be inserted.
pub async fn record_change_request(
    surreal: &SurrealDb,
    notation_id: Uuid,
    acting_person_id: Uuid,
    flagged_questions: &[String],
    note: Option<&str>,
) -> Result<ChangeRequest, ReaskError> {
    let request = ChangeRequest {
        flagged_questions: flagged_questions.to_vec(),
        note: note.map(str::to_string),
    };
    let payload = serde_json::to_string(&request).map_err(|e| ReaskError::Encode(e.to_string()))?;
    let now = chrono::Utc::now().to_rfc3339();
    append_event(
        surreal,
        TransitionRecord {
            notation_id,
            acting_person_id: Some(acting_person_id),
            machine_kind: MACHINE_REASK,
            from_state: REVIEW_STATE,
            to_state: REASK_STATE,
            condition: CONDITION_CHANGES_REQUESTED,
            payload_json: Some(payload),
            recorded_at: &now,
        },
    )
    .await?;
    Ok(request)
}

/// The latest change request recorded for `notation_id`, or `None` if the
/// notation has never been sent back for changes.
///
/// # Errors
/// Propagates a query failure.
pub async fn latest_change_request(
    surreal: &SurrealDb,
    notation_id: Uuid,
) -> Result<Option<ChangeRequest>, ReaskError> {
    let Some(event) = latest_for_kind(surreal, notation_id, MACHINE_REASK).await? else {
        return Ok(None);
    };
    Ok(event
        .payload
        .as_deref()
        .and_then(|p| serde_json::from_str(p).ok()))
}

/// The flagged question codes from the latest change request — the set the
/// re-ask surface presents and to which answer re-collection is gated. Empty
/// when the notation was never sent back for changes.
///
/// # Errors
/// Propagates a query failure.
pub async fn flagged_questions(
    surreal: &SurrealDb,
    notation_id: Uuid,
) -> Result<Vec<String>, ReaskError> {
    Ok(latest_change_request(surreal, notation_id)
        .await?
        .map(|r| r.flagged_questions)
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::{flagged_questions, latest_change_request, record_change_request};
    use crate::test_support;

    #[tokio::test]
    async fn change_request_round_trips_flagged_set_and_note() {
        let surreal = test_support::mem_surreal().await;
        let notation_id = test_support::seed_notation(&surreal).await;
        let lawyer = test_support::dri_person(&surreal).await;

        record_change_request(
            &surreal,
            notation_id,
            lawyer,
            &["person__client".into(), "project__engagement".into()],
            Some("client name is misspelled; confirm the entity type"),
        )
        .await
        .expect("record change request");

        let request = latest_change_request(&surreal, notation_id)
            .await
            .expect("read change request")
            .expect("a change request was recorded");
        assert_eq!(
            request.flagged_questions,
            vec![
                "person__client".to_string(),
                "project__engagement".to_string()
            ],
        );
        assert_eq!(
            request.note.as_deref(),
            Some("client name is misspelled; confirm the entity type"),
        );
        assert_eq!(
            flagged_questions(&surreal, notation_id).await.unwrap(),
            vec![
                "person__client".to_string(),
                "project__engagement".to_string()
            ],
        );
    }

    #[tokio::test]
    async fn flagged_questions_is_empty_when_never_sent_back() {
        let surreal = test_support::mem_surreal().await;
        let notation_id = test_support::seed_notation(&surreal).await;
        assert!(flagged_questions(&surreal, notation_id)
            .await
            .unwrap()
            .is_empty());
        assert!(latest_change_request(&surreal, notation_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn latest_change_request_projects_the_most_recent_round() {
        // Append-only: a second round of changes supersedes the first, so
        // the re-ask surface always re-collects the current flagged set.
        let surreal = test_support::mem_surreal().await;
        let notation_id = test_support::seed_notation(&surreal).await;
        let lawyer = test_support::dri_person(&surreal).await;

        record_change_request(
            &surreal,
            notation_id,
            lawyer,
            &["person__client".into()],
            None,
        )
        .await
        .unwrap();
        record_change_request(
            &surreal,
            notation_id,
            lawyer,
            &["project__engagement".into()],
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            flagged_questions(&surreal, notation_id).await.unwrap(),
            vec!["project__engagement".to_string()],
        );
    }
}
