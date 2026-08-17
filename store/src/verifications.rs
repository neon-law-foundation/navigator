//! `store::verifications` — recording that a licensed human checked a
//! citation, and invalidating that record when the draft moves (#891).
//!
//! Every command here keeps two properties:
//!
//! 1. **A verification names the revision it verified.** `revision_sha`
//!    is required at the type level, not merely at the column, so no
//!    caller can record an unpinned attestation.
//! 2. **Unverified is the only safe seed.** Recording an axis as passing
//!    when nobody checked it overclaims — it asserts diligence that did
//!    not happen — which is worse than having no verification at all.
//!
//! # This table lives in SurrealDB
//!
//! `verifications` moved with wave five of #1093 (ENG-121), in the
//! citation-apparatus slice. Matter scoping flows through
//! `citation_id → authority_use_id → project_id`; this table carries no
//! `project_id` of its own — see `store::authorities` for the rest of the
//! chain.

use rules::citation::{Axis, AxisStatus};
use surrealdb::types::SurrealValue;
use uuid::Uuid;

use crate::surreal::{record_id, record_uuid, SurrealDb};

const TABLE: &str = "verification";
const CITATION_TABLE: &str = "citation";
const PERSON_TABLE: &str = "person";

/// Why a verification command refused.
#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    /// A verification was offered without naming the revision it
    /// verified. Rejected before it reaches the database, because such a
    /// row is meaningless the moment the draft moves.
    #[error("a verification must name the draft revision it verified")]
    MissingRevision,
    /// A stored status is outside [`AxisStatus`], which means a row was
    /// written around the intended taxonomy.
    #[error("`{0}` is not a recognized axis status")]
    UnknownStatus(String),
    #[error("database: {0}")]
    Db(#[from] surrealdb::Error),
    /// A write reported success but returned no row, or returned one this
    /// module could not read back.
    #[error("writing a verification returned no usable row")]
    WriteReturnedNothing,
    /// [`set_axis`] was given a `verification_id` that does not exist.
    #[error("verification `{0}` not found")]
    NotFound(Uuid),
}

/// One verification: three independent axes, each pinned to the draft
/// revision it was checked against.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Verification {
    pub id: Uuid,
    pub citation_id: Uuid,
    pub revision_sha: String,
    pub status_citation: String,
    pub status_quote: String,
    pub status_proposition: String,
    pub verifier_person_id: Uuid,
    pub inserted_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(SurrealValue)]
struct VerificationRow {
    id: surrealdb::types::RecordId,
    citation_id: surrealdb::types::RecordId,
    revision_sha: String,
    status_citation: String,
    status_quote: String,
    status_proposition: String,
    verifier_person_id: surrealdb::types::RecordId,
    inserted_at: surrealdb::types::Datetime,
    updated_at: surrealdb::types::Datetime,
}

impl VerificationRow {
    /// `None` when a record id is not a native UUID key — a row written
    /// by something that bypassed [`crate::surreal::record_id`].
    fn into_verification(self) -> Option<Verification> {
        Some(Verification {
            id: record_uuid(&self.id)?,
            citation_id: record_uuid(&self.citation_id)?,
            revision_sha: self.revision_sha,
            status_citation: self.status_citation,
            status_quote: self.status_quote,
            status_proposition: self.status_proposition,
            verifier_person_id: record_uuid(&self.verifier_person_id)?,
            inserted_at: self.inserted_at.into(),
            updated_at: self.updated_at.into(),
        })
    }
}

const SELECT: &str = "id, citation_id, revision_sha, status_citation, status_quote, \
     status_proposition, verifier_person_id, inserted_at, updated_at";

fn many(mut response: surrealdb::IndexedResults) -> Result<Vec<Verification>, surrealdb::Error> {
    let rows: Vec<VerificationRow> = response.take(0)?;
    Ok(rows
        .into_iter()
        .filter_map(VerificationRow::into_verification)
        .collect())
}

/// Record a verification of `citation_id` against `revision_sha`.
///
/// All three axes start [`AxisStatus::Unverified`]. Callers move each one
/// deliberately through [`set_axis`]; there is no constructor that takes
/// a passing status, so a bulk import cannot backfill an axis as verified
/// without going through the same per-axis attestation a human would.
///
/// # Errors
/// [`VerificationError::MissingRevision`] when `revision_sha` is blank,
/// or a database error.
pub async fn record(
    db: &SurrealDb,
    citation_id: Uuid,
    revision_sha: &str,
    verifier_person_id: Uuid,
) -> Result<Verification, VerificationError> {
    // Checked here rather than relying on the column alone: an empty
    // string satisfies a NOT NULL constraint and names no revision.
    if revision_sha.trim().is_empty() {
        return Err(VerificationError::MissingRevision);
    }

    let id = Uuid::now_v7();
    let unverified = AxisStatus::Unverified.as_str().to_string();
    let mut response = db
        .query(format!(
            "CREATE $id SET \
             citation_id = $citation_id, revision_sha = $revision_sha, \
             status_citation = $status, status_quote = $status, status_proposition = $status, \
             verifier_person_id = $verifier_person_id \
             RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, id)))
        .bind(("citation_id", record_id(CITATION_TABLE, citation_id)))
        .bind(("revision_sha", revision_sha.to_string()))
        .bind(("status", unverified))
        .bind((
            "verifier_person_id",
            record_id(PERSON_TABLE, verifier_person_id),
        ))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<VerificationRow> = response.take(0)?;
    row.and_then(VerificationRow::into_verification)
        .ok_or(VerificationError::WriteReturnedNothing)
}

async fn find_by_id(db: &SurrealDb, id: Uuid) -> Result<Option<Verification>, surrealdb::Error> {
    let mut response = db
        .query(format!("SELECT {SELECT} FROM ONLY $id LIMIT 1"))
        .bind(("id", record_id(TABLE, id)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<VerificationRow> = response.take(0)?;
    Ok(row.and_then(VerificationRow::into_verification))
}

/// Set one `axis` of `verification_id` to `status`, leaving the other two
/// untouched.
///
/// The axes are independent on purpose. A citation can be real and
/// correctly formatted, its quote accurate, and still not support the
/// assertion it is cited for — collapsing them into one control is
/// exactly what hides that case.
///
/// # Errors
/// [`VerificationError::NotFound`] when `verification_id` does not exist,
/// or a database error.
pub async fn set_axis(
    db: &SurrealDb,
    verification_id: Uuid,
    axis: Axis,
    status: AxisStatus,
) -> Result<Verification, VerificationError> {
    if find_by_id(db, verification_id).await?.is_none() {
        return Err(VerificationError::NotFound(verification_id));
    }
    let field = match axis {
        Axis::Citation => "status_citation",
        Axis::Quote => "status_quote",
        Axis::Proposition => "status_proposition",
    };
    let mut response = db
        .query(format!(
            "UPDATE $id SET {field} = $status, updated_at = time::now() RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, verification_id)))
        .bind(("status", status.as_str().to_string()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<VerificationRow> = response.take(0)?;
    row.and_then(VerificationRow::into_verification)
        .ok_or(VerificationError::WriteReturnedNothing)
}

/// Read one axis of a stored row.
///
/// # Errors
/// [`VerificationError::UnknownStatus`] when the stored value is outside
/// the closed vocabulary.
pub fn axis_status(row: &Verification, axis: Axis) -> Result<AxisStatus, VerificationError> {
    let raw = match axis {
        Axis::Citation => &row.status_citation,
        Axis::Quote => &row.status_quote,
        Axis::Proposition => &row.status_proposition,
    };
    AxisStatus::parse(raw).ok_or_else(|| VerificationError::UnknownStatus(raw.clone()))
}

/// Every verification recorded against `citation_id`, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn for_citation(
    db: &SurrealDb,
    citation_id: Uuid,
) -> Result<Vec<Verification>, VerificationError> {
    let response = db
        .query(format!(
            "SELECT {SELECT} FROM {TABLE} WHERE citation_id = $citation ORDER BY id DESC"
        ))
        .bind(("citation", record_id(CITATION_TABLE, citation_id)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    Ok(many(response)?)
}

/// A single axis carried to [`AxisStatus::Stale`] by a draft edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleTransition {
    pub verification_id: Uuid,
    pub citation_id: Uuid,
    pub axis: Axis,
    /// The revision the verification was pinned to, which is no longer
    /// the current draft.
    pub from_revision_sha: String,
}

/// Carry every verification of `citation_id` that was pinned to a
/// revision other than `current_revision_sha` to
/// [`AxisStatus::Stale`], and return the transitions.
///
/// **A draft edit must not silently retain downstream verifications.**
/// The check may still be correct, but nothing records that anyone
/// confirmed it against the current text, and a verification quietly
/// going stale is exactly the condition a filing deadline turns into a
/// problem.
///
/// Only axes that make a claim about the text move: an unverified axis
/// claims nothing and an already-stale one has nowhere further to go, so
/// neither is carried. Manufacturing those transitions would pollute the
/// staleness rate the telemetry measures.
///
/// The returned transitions are what a caller emits as telemetry —
/// identifiers, an axis, and an outcome. No quote, no citation string,
/// no proposition.
///
/// # Errors
/// Propagates any database error.
pub async fn stale_after_revision(
    db: &SurrealDb,
    citation_id: Uuid,
    current_revision_sha: &str,
) -> Result<Vec<StaleTransition>, VerificationError> {
    let response = db
        .query(format!(
            "SELECT {SELECT} FROM {TABLE} \
             WHERE citation_id = $citation AND revision_sha != $revision"
        ))
        .bind(("citation", record_id(CITATION_TABLE, citation_id)))
        .bind(("revision", current_revision_sha.to_string()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let rows = many(response)?;

    let mut transitions = Vec::new();
    for row in rows {
        let mut moved = Vec::new();
        for axis in Axis::ALL {
            if axis_status(&row, *axis)?.goes_stale_on_edit() {
                moved.push(*axis);
            }
        }
        if moved.is_empty() {
            continue;
        }

        for axis in &moved {
            set_axis(db, row.id, *axis, AxisStatus::Stale).await?;
        }
        for axis in moved {
            transitions.push(StaleTransition {
                verification_id: row.id,
                citation_id,
                axis,
                from_revision_sha: row.revision_sha.clone(),
            });
        }
    }
    Ok(transitions)
}

#[cfg(test)]
mod tests {
    use super::{
        axis_status, for_citation, record, set_axis, stale_after_revision, VerificationError,
    };
    use crate::authorities::{
        cite, cite_in_matter, record as record_authority, NewAuthority, NewCitation,
    };
    use crate::surreal::test_support::mem;
    use crate::test_support::{dri_person, seed_project_surreal};
    use rules::citation::{AuthorityClass, Axis, AxisStatus, Disposition};
    use uuid::Uuid;

    /// A citation on a real matter, plus a person to attest to it.
    async fn fixture(surreal: &crate::surreal::SurrealDb) -> (Uuid, Uuid) {
        let project_id = seed_project_surreal(surreal, "verif").await;

        let authority = record_authority(
            surreal,
            &NewAuthority {
                class: AuthorityClass::CaseLaw,
                citation: "410 U.S. 113 (1973)",
                short_cite: None,
                title: "Example v. Example",
                publisher: None,
                issued_on: None,
                canonical_url: None,
                checked_on: None,
                archived_asset_id: None,
            },
        )
        .await
        .expect("authority");

        let use_row = cite_in_matter(
            surreal,
            project_id,
            authority.id,
            "ours",
            Disposition::ReliedOn,
            None,
        )
        .await
        .expect("use");

        let citation = cite(
            surreal,
            &NewCitation {
                authority_use_id: use_row.id,
                quote: "the standard is de novo",
                why: "states the standard of review this brief argues for",
                source_pin: None,
                draft_pin: None,
            },
        )
        .await
        .expect("citation");

        let person_id = dri_person(surreal).await;
        (citation.id, person_id)
    }

    /// Every axis seeds `unverified`. Overclaiming a verification is
    /// worse than having none: it asserts a human checked something
    /// nobody checked.
    #[tokio::test]
    async fn a_new_verification_seeds_every_axis_unverified() {
        let surreal = mem().await;
        let (citation_id, person_id) = fixture(&surreal).await;

        let v = record(&surreal, citation_id, "abc123", person_id)
            .await
            .expect("record");

        for axis in Axis::ALL {
            assert_eq!(
                axis_status(&v, *axis).expect("status"),
                AxisStatus::Unverified,
                "{} must not seed as passing",
                axis.as_str()
            );
        }
    }

    /// The three axes are independently settable. A citation can be real
    /// and correctly formatted, its quote accurate, and still fail the
    /// proposition axis — that is the case a boolean hides.
    #[tokio::test]
    async fn the_three_axes_are_set_independently() {
        let surreal = mem().await;
        let (citation_id, person_id) = fixture(&surreal).await;
        let v = record(&surreal, citation_id, "abc123", person_id)
            .await
            .expect("record");

        set_axis(&surreal, v.id, Axis::Citation, AxisStatus::Verified)
            .await
            .expect("citation axis");
        set_axis(&surreal, v.id, Axis::Quote, AxisStatus::Verified)
            .await
            .expect("quote axis");
        let after = set_axis(&surreal, v.id, Axis::Proposition, AxisStatus::Rejected)
            .await
            .expect("proposition axis");

        assert_eq!(
            axis_status(&after, Axis::Citation).expect("s"),
            AxisStatus::Verified
        );
        assert_eq!(
            axis_status(&after, Axis::Quote).expect("s"),
            AxisStatus::Verified
        );
        assert_eq!(
            axis_status(&after, Axis::Proposition).expect("s"),
            AxisStatus::Rejected,
            "a real case, accurately quoted, for something it does not say"
        );
    }

    /// A verification cannot be recorded without naming its revision —
    /// an all-whitespace SHA satisfies a NOT NULL column but names no
    /// revision.
    #[tokio::test]
    async fn a_verification_cannot_be_recorded_without_naming_its_revision() {
        let surreal = mem().await;
        let (citation_id, person_id) = fixture(&surreal).await;

        for blank in ["", "   "] {
            let err = record(&surreal, citation_id, blank, person_id)
                .await
                .expect_err("a blank revision must be refused");
            assert!(matches!(err, VerificationError::MissingRevision), "{err:?}");
        }
    }

    /// **The property #891 names.** The draft moves; verifications
    /// pinned to the old revision are carried to `stale` rather than
    /// silently retained.
    #[tokio::test]
    async fn a_draft_edit_marks_downstream_verifications_stale() {
        let surreal = mem().await;
        let (citation_id, person_id) = fixture(&surreal).await;

        let v = record(&surreal, citation_id, "revision-one", person_id)
            .await
            .expect("record");
        set_axis(&surreal, v.id, Axis::Citation, AxisStatus::Verified)
            .await
            .expect("citation");
        set_axis(&surreal, v.id, Axis::Quote, AxisStatus::Verified)
            .await
            .expect("quote");
        set_axis(&surreal, v.id, Axis::Proposition, AxisStatus::Rejected)
            .await
            .expect("proposition");

        let transitions = stale_after_revision(&surreal, citation_id, "revision-two")
            .await
            .expect("sweep");

        assert_eq!(
            transitions.len(),
            3,
            "every axis that claimed something about the text goes stale"
        );
        for t in &transitions {
            assert_eq!(t.verification_id, v.id);
            assert_eq!(t.citation_id, citation_id);
            assert_eq!(
                t.from_revision_sha, "revision-one",
                "the transition names the revision that was left behind"
            );
        }

        let after = for_citation(&surreal, citation_id)
            .await
            .expect("read back");
        for axis in Axis::ALL {
            assert_eq!(
                axis_status(&after[0], *axis).expect("status"),
                AxisStatus::Stale,
                "{} was silently retained across a draft edit",
                axis.as_str()
            );
        }
    }

    /// A verification pinned to the *current* revision is untouched. The
    /// sweep invalidates what the edit outran, not everything it can
    /// reach.
    #[tokio::test]
    async fn a_verification_of_the_current_revision_survives_the_sweep() {
        let surreal = mem().await;
        let (citation_id, person_id) = fixture(&surreal).await;

        let v = record(&surreal, citation_id, "revision-two", person_id)
            .await
            .expect("record");
        set_axis(&surreal, v.id, Axis::Quote, AxisStatus::Verified)
            .await
            .expect("quote");

        let transitions = stale_after_revision(&surreal, citation_id, "revision-two")
            .await
            .expect("sweep");
        assert!(
            transitions.is_empty(),
            "nothing to invalidate: this verification names the current draft"
        );

        let after = for_citation(&surreal, citation_id)
            .await
            .expect("read back");
        assert_eq!(
            axis_status(&after[0], Axis::Quote).expect("status"),
            AxisStatus::Verified
        );
    }

    /// An unverified axis claims nothing, so a draft edit cannot
    /// invalidate it, and a stale axis has nowhere further to go.
    /// Carrying either would manufacture a transition that did not
    /// happen and pollute the staleness rate the telemetry measures.
    #[tokio::test]
    async fn the_sweep_reports_only_transitions_that_actually_happened() {
        let surreal = mem().await;
        let (citation_id, person_id) = fixture(&surreal).await;

        let v = record(&surreal, citation_id, "revision-one", person_id)
            .await
            .expect("record");
        set_axis(&surreal, v.id, Axis::Quote, AxisStatus::Verified)
            .await
            .expect("quote");

        let first = stale_after_revision(&surreal, citation_id, "revision-two")
            .await
            .expect("first sweep");
        assert_eq!(first.len(), 1, "only the checked axis transitions");
        assert_eq!(first[0].axis, Axis::Quote);

        let second = stale_after_revision(&surreal, citation_id, "revision-three")
            .await
            .expect("second sweep");
        assert!(
            second.is_empty(),
            "an already-stale axis must not re-transition"
        );

        let after = for_citation(&surreal, citation_id)
            .await
            .expect("read back");
        assert_eq!(
            axis_status(&after[0], Axis::Citation).expect("status"),
            AxisStatus::Unverified,
            "an axis nobody checked is still unverified, not stale"
        );
    }

    /// The staleness transition carries identifiers and an outcome only
    /// — it is what a caller emits as telemetry, and it must not be able
    /// to carry document content across the trust boundary.
    #[tokio::test]
    async fn a_stale_transition_carries_no_document_content() {
        let surreal = mem().await;
        let (citation_id, person_id) = fixture(&surreal).await;
        let v = record(&surreal, citation_id, "revision-one", person_id)
            .await
            .expect("record");
        set_axis(&surreal, v.id, Axis::Proposition, AxisStatus::Verified)
            .await
            .expect("proposition");

        let transitions = stale_after_revision(&surreal, citation_id, "revision-two")
            .await
            .expect("sweep");
        let rendered = format!("{transitions:?}");

        assert!(
            !rendered.contains("the standard is de novo"),
            "the quote must not reach telemetry: {rendered}"
        );
        assert!(
            !rendered.contains("states the standard of review"),
            "the proposition must not reach telemetry: {rendered}"
        );
        assert!(
            !rendered.contains("410 U.S. 113"),
            "the citation string must not reach telemetry: {rendered}"
        );
        assert!(rendered.contains("Proposition"), "the axis does travel");
    }
}
