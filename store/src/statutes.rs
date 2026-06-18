//! Read + insert-only-upsert helpers for the public legal-code
//! reference (`statutes` + `statute_revisions`).
//!
//! The data layer for the NRS scraper. The `statutes` crate calls
//! [`upsert_section`] / [`mark_missing_repealed`] from the weekly sync;
//! `web::statutes` calls the read helpers to render the public pages.
//! Kept here, beside the other store orchestration modules, so neither
//! consumer re-imports the entities.
//!
//! The model is **insert-only**: a section's text lives in append-only
//! [`crate::entity::statute_revision`] rows that are never updated or
//! deleted. "Current" is the revision with the greatest `observed_at`.
//! See `prompts/nrs-statute-scraper-design.md`.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder,
};
use uuid::Uuid;

use crate::entity::statute::{self, Status};
use crate::entity::statute_revision;
use crate::Db;

/// One section as parsed from the source, ready to reconcile against the
/// stored history. `body_sha256` is computed by the caller over the
/// **normalized** text (whitespace collapsed, chrome stripped) so
/// cosmetic re-formatting on the source doesn't forge a spurious
/// revision; `body` is the cleaned display text to store and render.
#[derive(Debug, Clone)]
pub struct SectionUpsert<'a> {
    pub jurisdiction: &'a str,
    pub code: &'a str,
    pub chapter: &'a str,
    pub chapter_title: &'a str,
    pub section: &'a str,
    pub source_url: &'a str,
    pub section_title: &'a str,
    pub body: &'a str,
    pub body_sha256: &'a str,
    pub history_note: Option<&'a str>,
}

/// What one [`upsert_section`] call did — drives the run summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// First time we've seen this section: identity row + first revision.
    Created,
    /// Section seen before, text unchanged: only `last_checked_at` moved.
    Unchanged,
    /// Text differs from the latest revision: a new revision was
    /// appended; the prior revision is untouched.
    Revised,
}

/// A section paired with its current text (latest revision) — the shape
/// the render surface consumes.
#[derive(Debug, Clone)]
pub struct CurrentSection {
    pub statute: statute::Model,
    pub revision: statute_revision::Model,
}

/// A chapter available in the reference, with how many sections it has.
#[derive(Debug, Clone)]
pub struct ChapterSummary {
    pub code: String,
    pub chapter: String,
    pub chapter_title: String,
    pub section_count: u64,
}

/// Reconcile one parsed section against the stored history, insert-only.
///
/// - No identity row yet → insert `statute` (`active`) + first revision.
/// - Latest revision's hash matches → bump `last_checked_at` only
///   (re-activating a previously repealed section that reappeared
///   unchanged).
/// - Hash differs → append a new revision, bump `last_changed_at`.
///
/// `run_at` is the RFC 3339 run timestamp, shared across the whole run.
///
/// # Errors
///
/// Propagates any database error.
pub async fn upsert_section(
    db: &Db,
    parsed: &SectionUpsert<'_>,
    run_at: &str,
) -> Result<(Uuid, Outcome), sea_orm::DbErr> {
    let existing = statute::Entity::find()
        .filter(statute::Column::Code.eq(parsed.code))
        .filter(statute::Column::Section.eq(parsed.section))
        .one(db)
        .await?;

    let Some(row) = existing else {
        // Case 1 — brand new section.
        let inserted = statute::ActiveModel {
            jurisdiction: ActiveValue::Set(parsed.jurisdiction.to_string()),
            code: ActiveValue::Set(parsed.code.to_string()),
            chapter: ActiveValue::Set(parsed.chapter.to_string()),
            chapter_title: ActiveValue::Set(parsed.chapter_title.to_string()),
            section: ActiveValue::Set(parsed.section.to_string()),
            source_url: ActiveValue::Set(parsed.source_url.to_string()),
            status: ActiveValue::Set(Status::Active),
            first_seen_at: ActiveValue::Set(run_at.to_string()),
            last_checked_at: ActiveValue::Set(run_at.to_string()),
            last_changed_at: ActiveValue::Set(run_at.to_string()),
            ..Default::default()
        }
        .insert(db)
        .await?;
        insert_revision(db, inserted.id, parsed, run_at).await?;
        return Ok((inserted.id, Outcome::Created));
    };

    let latest = latest_revision(db, row.id).await?;
    let unchanged = latest
        .as_ref()
        .is_some_and(|r| r.body_sha256 == parsed.body_sha256);

    if unchanged {
        // Case 2 — text unchanged. Bump the cheap "we looked" stamp and
        // re-activate if it had been marked repealed and reappeared.
        let mut active: statute::ActiveModel = row.clone().into();
        active.last_checked_at = ActiveValue::Set(run_at.to_string());
        if row.status == Status::Repealed {
            active.status = ActiveValue::Set(Status::Active);
        }
        active.update(db).await?;
        return Ok((row.id, Outcome::Unchanged));
    }

    // Case 3 — text differs (or, rarely, an active row somehow has no
    // revision): append a new revision and move the change clock.
    insert_revision(db, row.id, parsed, run_at).await?;
    let mut active: statute::ActiveModel = row.clone().into();
    active.last_checked_at = ActiveValue::Set(run_at.to_string());
    active.last_changed_at = ActiveValue::Set(run_at.to_string());
    active.status = ActiveValue::Set(Status::Active);
    active.update(db).await?;
    Ok((row.id, Outcome::Revised))
}

/// Mark every still-`active` section of `(code, chapter)` whose section
/// number is **not** in `present` as `repealed` — case 4. Returns the
/// number of sections newly repealed. History is never deleted; the
/// last revision stays as the final observed text.
///
/// # Errors
///
/// Propagates any database error.
pub async fn mark_missing_repealed(
    db: &Db,
    code: &str,
    chapter: &str,
    present: &[String],
    run_at: &str,
) -> Result<u64, sea_orm::DbErr> {
    let rows = statute::Entity::find()
        .filter(statute::Column::Code.eq(code))
        .filter(statute::Column::Chapter.eq(chapter))
        .filter(statute::Column::Status.eq(Status::Active))
        .all(db)
        .await?;

    let mut repealed = 0u64;
    for row in rows {
        if present.iter().any(|s| s == &row.section) {
            continue;
        }
        let mut active: statute::ActiveModel = row.into();
        active.status = ActiveValue::Set(Status::Repealed);
        active.last_checked_at = ActiveValue::Set(run_at.to_string());
        active.update(db).await?;
        repealed += 1;
    }
    Ok(repealed)
}

/// The latest (greatest `observed_at`) revision for a section, if any.
/// `id` descending breaks ties within a run — `Uuid::now_v7` is
/// time-sortable, so the newest insert wins.
///
/// # Errors
///
/// Propagates any database error.
pub async fn latest_revision(
    db: &Db,
    statute_id: Uuid,
) -> Result<Option<statute_revision::Model>, sea_orm::DbErr> {
    statute_revision::Entity::find()
        .filter(statute_revision::Column::StatuteId.eq(statute_id))
        .order_by_desc(statute_revision::Column::ObservedAt)
        .order_by_desc(statute_revision::Column::Id)
        .one(db)
        .await
}

/// Every revision for a section, newest first — the history view.
///
/// # Errors
///
/// Propagates any database error.
pub async fn revisions(
    db: &Db,
    statute_id: Uuid,
) -> Result<Vec<statute_revision::Model>, sea_orm::DbErr> {
    statute_revision::Entity::find()
        .filter(statute_revision::Column::StatuteId.eq(statute_id))
        .order_by_desc(statute_revision::Column::ObservedAt)
        .order_by_desc(statute_revision::Column::Id)
        .all(db)
        .await
}

/// Distinct chapters available for a code, ordered by chapter, each with
/// its section count — the `/statutes` index.
///
/// # Errors
///
/// Propagates any database error.
pub async fn chapters(db: &Db, code: &str) -> Result<Vec<ChapterSummary>, sea_orm::DbErr> {
    let rows = statute::Entity::find()
        .filter(statute::Column::Code.eq(code))
        .order_by_asc(statute::Column::Chapter)
        .order_by_asc(statute::Column::Section)
        .all(db)
        .await?;

    let mut out: Vec<ChapterSummary> = Vec::new();
    for row in rows {
        match out.last_mut() {
            Some(last) if last.chapter == row.chapter && last.code == row.code => {
                last.section_count += 1;
            }
            _ => out.push(ChapterSummary {
                code: row.code,
                chapter: row.chapter,
                chapter_title: row.chapter_title,
                section_count: 1,
            }),
        }
    }
    Ok(out)
}

/// Every section of a chapter, in section order, each with its current
/// text (latest revision). Sections with no revision are skipped.
///
/// # Errors
///
/// Propagates any database error.
pub async fn sections_in_chapter(
    db: &Db,
    code: &str,
    chapter: &str,
) -> Result<Vec<CurrentSection>, sea_orm::DbErr> {
    let rows = statute::Entity::find()
        .filter(statute::Column::Code.eq(code))
        .filter(statute::Column::Chapter.eq(chapter))
        .order_by_asc(statute::Column::Section)
        .all(db)
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for statute in rows {
        if let Some(revision) = latest_revision(db, statute.id).await? {
            out.push(CurrentSection { statute, revision });
        }
    }
    Ok(out)
}

/// A single section by `(code, section)` with its current text.
///
/// # Errors
///
/// Propagates any database error.
pub async fn section(
    db: &Db,
    code: &str,
    section: &str,
) -> Result<Option<CurrentSection>, sea_orm::DbErr> {
    let Some(statute) = statute::Entity::find()
        .filter(statute::Column::Code.eq(code))
        .filter(statute::Column::Section.eq(section))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    Ok(latest_revision(db, statute.id)
        .await?
        .map(|revision| CurrentSection { statute, revision }))
}

async fn insert_revision(
    db: &Db,
    statute_id: Uuid,
    parsed: &SectionUpsert<'_>,
    run_at: &str,
) -> Result<(), sea_orm::DbErr> {
    statute_revision::ActiveModel {
        statute_id: ActiveValue::Set(statute_id),
        body: ActiveValue::Set(parsed.body.to_string()),
        body_sha256: ActiveValue::Set(parsed.body_sha256.to_string()),
        section_title: ActiveValue::Set(parsed.section_title.to_string()),
        history_note: ActiveValue::Set(parsed.history_note.map(str::to_string)),
        observed_at: ActiveValue::Set(run_at.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

/// Count of revision rows for a statute — test/diagnostic helper.
///
/// # Errors
///
/// Propagates any database error.
pub async fn revision_count(db: &Db, statute_id: Uuid) -> Result<u64, sea_orm::DbErr> {
    statute_revision::Entity::find()
        .filter(statute_revision::Column::StatuteId.eq(statute_id))
        .count(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::{
        chapters, mark_missing_repealed, revision_count, section, sections_in_chapter,
        upsert_section, Outcome, SectionUpsert,
    };
    use crate::entity::statute::Status;

    fn nrs_86_011(body: &str, sha: &str) -> SectionUpsert<'static> {
        // Leak to get 'static strs for terse fixtures; tests are short.
        SectionUpsert {
            jurisdiction: "NV",
            code: "NRS",
            chapter: "86",
            chapter_title: "Limited-Liability Companies",
            section: "86.011",
            source_url: "https://www.leg.state.nv.us/NRS/NRS-086.html#NRS086Sec011",
            section_title: "Definitions.",
            body: Box::leak(body.to_string().into_boxed_str()),
            body_sha256: Box::leak(sha.to_string().into_boxed_str()),
            history_note: Some("(Added to NRS by 1991, 1293)"),
        }
    }

    #[tokio::test]
    async fn first_run_creates_identity_and_one_revision() {
        let db = crate::test_support::pg().await;
        let (id, outcome) =
            upsert_section(&db, &nrs_86_011("v1", "hash-v1"), "2026-06-07T10:00:00Z")
                .await
                .unwrap();
        assert_eq!(outcome, Outcome::Created);
        assert_eq!(revision_count(&db, id).await.unwrap(), 1);

        let cur = section(&db, "NRS", "86.011").await.unwrap().unwrap();
        assert_eq!(cur.statute.status, Status::Active);
        assert_eq!(cur.revision.body, "v1");
        assert_eq!(cur.statute.first_seen_at, "2026-06-07T10:00:00Z");
    }

    #[tokio::test]
    async fn unchanged_run_adds_no_revision_and_only_bumps_checked() {
        let db = crate::test_support::pg().await;
        let (id, _) = upsert_section(&db, &nrs_86_011("v1", "hash-v1"), "2026-06-07T10:00:00Z")
            .await
            .unwrap();
        let (id2, outcome) =
            upsert_section(&db, &nrs_86_011("v1", "hash-v1"), "2026-06-14T10:00:00Z")
                .await
                .unwrap();
        assert_eq!(id, id2);
        assert_eq!(outcome, Outcome::Unchanged);
        assert_eq!(revision_count(&db, id).await.unwrap(), 1);

        let cur = section(&db, "NRS", "86.011").await.unwrap().unwrap();
        assert_eq!(cur.statute.last_checked_at, "2026-06-14T10:00:00Z");
        // change clock did NOT move on an unchanged run
        assert_eq!(cur.statute.last_changed_at, "2026-06-07T10:00:00Z");
    }

    #[tokio::test]
    async fn changed_body_appends_revision_and_keeps_history() {
        let db = crate::test_support::pg().await;
        let (id, _) = upsert_section(&db, &nrs_86_011("v1", "hash-v1"), "2026-06-07T10:00:00Z")
            .await
            .unwrap();
        let (_, outcome) = upsert_section(
            &db,
            &nrs_86_011("v2 amended", "hash-v2"),
            "2026-06-14T10:00:00Z",
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Revised);
        assert_eq!(revision_count(&db, id).await.unwrap(), 2);

        // current shows the new text; old revision is still retrievable
        let cur = section(&db, "NRS", "86.011").await.unwrap().unwrap();
        assert_eq!(cur.revision.body, "v2 amended");
        assert_eq!(cur.statute.last_changed_at, "2026-06-14T10:00:00Z");
        let all = super::revisions(&db, id).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].body, "v2 amended");
        assert_eq!(all[1].body, "v1");
    }

    #[tokio::test]
    async fn vanished_section_is_repealed_not_deleted() {
        let db = crate::test_support::pg().await;
        let (id, _) = upsert_section(&db, &nrs_86_011("v1", "hash-v1"), "2026-06-07T10:00:00Z")
            .await
            .unwrap();
        // next run: chapter 86 came back without section 86.011
        let repealed = mark_missing_repealed(
            &db,
            "NRS",
            "86",
            &["86.021".to_string()],
            "2026-06-14T10:00:00Z",
        )
        .await
        .unwrap();
        assert_eq!(repealed, 1);

        let cur = section(&db, "NRS", "86.011").await.unwrap().unwrap();
        assert_eq!(cur.statute.status, Status::Repealed);
        // text preserved
        assert_eq!(cur.revision.body, "v1");
        assert_eq!(revision_count(&db, id).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn reappearing_section_reactivates() {
        let db = crate::test_support::pg().await;
        upsert_section(&db, &nrs_86_011("v1", "hash-v1"), "2026-06-07T10:00:00Z")
            .await
            .unwrap();
        mark_missing_repealed(&db, "NRS", "86", &[], "2026-06-14T10:00:00Z")
            .await
            .unwrap();
        // it comes back, same text
        let (_, outcome) =
            upsert_section(&db, &nrs_86_011("v1", "hash-v1"), "2026-06-21T10:00:00Z")
                .await
                .unwrap();
        assert_eq!(outcome, Outcome::Unchanged);
        let cur = section(&db, "NRS", "86.011").await.unwrap().unwrap();
        assert_eq!(cur.statute.status, Status::Active);
    }

    #[tokio::test]
    async fn chapters_and_sections_group_and_count() {
        let db = crate::test_support::pg().await;
        upsert_section(&db, &nrs_86_011("v1", "hash-v1"), "2026-06-07T10:00:00Z")
            .await
            .unwrap();
        let mut s86_021 = nrs_86_011("other", "hash-x");
        s86_021.section = "86.021";
        s86_021.section_title = "Articles.";
        upsert_section(&db, &s86_021, "2026-06-07T10:00:00Z")
            .await
            .unwrap();

        let chs = chapters(&db, "NRS").await.unwrap();
        assert_eq!(chs.len(), 1);
        assert_eq!(chs[0].chapter, "86");
        assert_eq!(chs[0].section_count, 2);

        let secs = sections_in_chapter(&db, "NRS", "86").await.unwrap();
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].statute.section, "86.011");
        assert_eq!(secs[1].statute.section, "86.021");
    }
}
