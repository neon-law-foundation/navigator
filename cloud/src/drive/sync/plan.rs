//! Pure diff function: given a Drive folder listing + what's already
//! in `documents` + `blobs` for a Project, decide which files to
//! download and which to skip.
//!
//! No I/O. No network. No database. The Restate workflow that
//! consumes the plan handles the actual download + ingest + retry.
//! Keeping the planner pure means it can be unit-tested without
//! `wiremock` or a Postgres container, and the workflow's durable
//! checkpoint is a small, JSON-serializable [`SyncPlan`] rather than
//! a tangled mess of HTTP futures.
//!
//! # Decision table
//!
//! | Drive file shape                       | Outcome                          |
//! |----------------------------------------|----------------------------------|
//! | Folder MIME                            | skip — [`SkipReason::Folder`]    |
//! | Binary, SHA in `known_blob_shas`       | skip — `AlreadyIngestedSha`      |
//! | Binary, SHA new (or absent)            | ingest                           |
//! | Google-native, supported export type,  |                                  |
//! |   revision in `known_revision_ids`     | skip — `AlreadyIngestedRevision` |
//! | Google-native, supported export type,  |                                  |
//! |   revision new (or absent)             | ingest                           |
//! | Google-native, unsupported (Forms,     |                                  |
//! |   Sites, Drawings, …)                  | skip — `UnsupportedGoogleNative` |
//!
//! "Revision new (or absent)" deliberately leans toward ingest: a
//! Drive file with no `headRevisionId` (rare for Docs but possible
//! for very fresh Google-native files) is still worth downloading
//! once. The ingest path may end up writing a duplicate blob if the
//! markdown export happens to be identical to a previous one, which
//! is acceptable — the blob layer dedupes by SHA so disk cost is
//! bounded; the second `documents` row is the only overhead.

use std::collections::HashSet;

use crate::drive::client::{export_mime_for, is_google_native, DriveFile, FOLDER_MIME};

/// What the planner knows about prior state for the Project being
/// synced. Both fields are borrowed sets so the caller controls
/// allocation and the planner stays cheap to call.
#[derive(Debug, Clone, Copy)]
pub struct PlanContext<'a> {
    /// SHA-256 (hex-encoded, lowercase) of every blob already linked
    /// to one of this Project's documents. Used to skip binary files
    /// whose bytes we already store.
    pub known_blob_shas: &'a HashSet<String>,

    /// Source revision ids (`headRevisionId` for Drive) already
    /// recorded on this Project's `documents` under
    /// `source = "drive_sync"`. Used to skip Google-native files
    /// whose live revision has already been exported and ingested.
    pub known_revision_ids: &'a HashSet<String>,
}

/// The full plan for one folder. The workflow iterates [`to_ingest`]
/// file-by-file (one transaction per file) and reports the count of
/// each [`skipped`] reason in its final status payload.
///
/// [`to_ingest`]: SyncPlan::to_ingest
/// [`skipped`]: SyncPlan::skipped
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncPlan {
    pub to_ingest: Vec<DriveFile>,
    pub skipped: Vec<SkippedItem>,
}

/// One file that the planner declined to ingest, with a structured
/// reason so the workflow's status payload can break down the skip
/// counts (`folder: 2, already_ingested_sha: 14, …`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedItem {
    pub file: DriveFile,
    pub reason: SkipReason,
}

/// Why a file ended up in [`SyncPlan::skipped`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// MIME type identifies a folder, not a file. Folders are not
    /// recursed; the planner is folder-scoped and the workflow runs
    /// once per Project's bound folder.
    Folder,
    /// A binary file whose Drive-computed SHA-256 already matches a
    /// blob linked to this Project. No download needed.
    AlreadyIngestedSha,
    /// A Google-native file whose `headRevisionId` is already
    /// recorded on this Project's `documents`. No export needed.
    AlreadyIngestedRevision,
    /// A Google-native type Drive can't export to one of the three
    /// targets we support (md / csv / pdf) — Forms, Sites, Drawings,
    /// Jamboard. The planner skips them so the workflow never hits a
    /// 4xx from `/files/{id}/export`.
    UnsupportedGoogleNative,
}

/// Categorize each Drive file against prior project state. Pure.
#[must_use]
pub fn plan(files: &[DriveFile], ctx: &PlanContext<'_>) -> SyncPlan {
    let mut out = SyncPlan::default();
    for file in files {
        match classify(file, ctx) {
            Decision::Ingest => out.to_ingest.push(file.clone()),
            Decision::Skip(reason) => out.skipped.push(SkippedItem {
                file: file.clone(),
                reason,
            }),
        }
    }
    out
}

enum Decision {
    Ingest,
    Skip(SkipReason),
}

fn classify(file: &DriveFile, ctx: &PlanContext<'_>) -> Decision {
    if file.mime_type == FOLDER_MIME {
        return Decision::Skip(SkipReason::Folder);
    }
    if is_google_native(&file.mime_type) {
        if export_mime_for(&file.mime_type).is_none() {
            return Decision::Skip(SkipReason::UnsupportedGoogleNative);
        }
        if let Some(rev) = file.head_revision_id.as_deref() {
            if ctx.known_revision_ids.contains(rev) {
                return Decision::Skip(SkipReason::AlreadyIngestedRevision);
            }
        }
        return Decision::Ingest;
    }
    // Binary file path. SHA-256 is the cheap-and-correct dedup
    // signal when Drive provides it; otherwise let the workflow
    // hash the bytes itself.
    if let Some(sha) = file.sha256_checksum.as_deref() {
        if ctx.known_blob_shas.contains(sha) {
            return Decision::Skip(SkipReason::AlreadyIngestedSha);
        }
    }
    Decision::Ingest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::client::{DriveFile, FOLDER_MIME};

    fn empty_ctx() -> (HashSet<String>, HashSet<String>) {
        (HashSet::new(), HashSet::new())
    }

    fn ctx<'a>(shas: &'a HashSet<String>, revs: &'a HashSet<String>) -> PlanContext<'a> {
        PlanContext {
            known_blob_shas: shas,
            known_revision_ids: revs,
        }
    }

    fn binary(id: &str, sha: Option<&str>) -> DriveFile {
        DriveFile {
            id: id.into(),
            name: format!("{id}.pdf"),
            mime_type: "application/pdf".into(),
            size: Some(1024),
            head_revision_id: Some(format!("rev-{id}")),
            sha256_checksum: sha.map(str::to_string),
            parents: vec!["folder-1".into()],
        }
    }

    fn google_doc(id: &str, revision: Option<&str>) -> DriveFile {
        DriveFile {
            id: id.into(),
            name: format!("{id}-doc"),
            mime_type: "application/vnd.google-apps.document".into(),
            size: None,
            head_revision_id: revision.map(str::to_string),
            sha256_checksum: None,
            parents: vec!["folder-1".into()],
        }
    }

    fn folder(id: &str) -> DriveFile {
        DriveFile {
            id: id.into(),
            name: id.into(),
            mime_type: FOLDER_MIME.into(),
            size: None,
            head_revision_id: None,
            sha256_checksum: None,
            parents: vec!["folder-1".into()],
        }
    }

    #[test]
    fn empty_input_returns_empty_plan() {
        let (s, r) = empty_ctx();
        let p = plan(&[], &ctx(&s, &r));
        assert!(p.to_ingest.is_empty());
        assert!(p.skipped.is_empty());
    }

    #[test]
    fn folders_are_skipped_with_folder_reason() {
        let files = [folder("fld-1")];
        let (s, r) = empty_ctx();
        let p = plan(&files, &ctx(&s, &r));
        assert!(p.to_ingest.is_empty());
        assert_eq!(p.skipped.len(), 1);
        assert_eq!(p.skipped[0].reason, SkipReason::Folder);
    }

    #[test]
    fn binary_file_with_unknown_sha_is_ingested() {
        let files = [binary("f1", Some("deadbeef"))];
        let (s, r) = empty_ctx();
        let p = plan(&files, &ctx(&s, &r));
        assert_eq!(p.to_ingest.len(), 1);
        assert_eq!(p.to_ingest[0].id, "f1");
        assert!(p.skipped.is_empty());
    }

    #[test]
    fn binary_file_with_no_sha_metadata_is_ingested() {
        // Drive sometimes omits the checksum (very recent uploads).
        // Don't gate ingest on it — the workflow will SHA the bytes
        // itself and `ingest_bytes` dedupes at the blob layer.
        let files = [binary("f1", None)];
        let (s, r) = empty_ctx();
        let p = plan(&files, &ctx(&s, &r));
        assert_eq!(p.to_ingest.len(), 1);
    }

    #[test]
    fn binary_file_with_known_sha_is_skipped_already_ingested() {
        let shas: HashSet<String> = ["deadbeef".into()].into_iter().collect();
        let revs = HashSet::new();
        let files = [binary("f1", Some("deadbeef"))];
        let p = plan(&files, &ctx(&shas, &revs));
        assert!(p.to_ingest.is_empty());
        assert_eq!(p.skipped.len(), 1);
        assert_eq!(p.skipped[0].reason, SkipReason::AlreadyIngestedSha);
    }

    #[test]
    fn google_doc_with_new_revision_is_ingested() {
        let files = [google_doc("doc-1", Some("rev-A"))];
        let (s, r) = empty_ctx();
        let p = plan(&files, &ctx(&s, &r));
        assert_eq!(p.to_ingest.len(), 1);
        assert_eq!(p.to_ingest[0].id, "doc-1");
    }

    #[test]
    fn google_doc_with_known_revision_is_skipped() {
        let shas = HashSet::new();
        let revs: HashSet<String> = ["rev-A".into()].into_iter().collect();
        let files = [google_doc("doc-1", Some("rev-A"))];
        let p = plan(&files, &ctx(&shas, &revs));
        assert!(p.to_ingest.is_empty());
        assert_eq!(p.skipped.len(), 1);
        assert_eq!(p.skipped[0].reason, SkipReason::AlreadyIngestedRevision);
    }

    #[test]
    fn google_native_form_is_skipped_unsupported() {
        let file = DriveFile {
            id: "frm-1".into(),
            name: "Intake form".into(),
            mime_type: "application/vnd.google-apps.form".into(),
            size: None,
            head_revision_id: Some("rev-form".into()),
            sha256_checksum: None,
            parents: vec!["folder-1".into()],
        };
        let (s, r) = empty_ctx();
        let p = plan(std::slice::from_ref(&file), &ctx(&s, &r));
        assert!(p.to_ingest.is_empty());
        assert_eq!(p.skipped.len(), 1);
        assert_eq!(p.skipped[0].reason, SkipReason::UnsupportedGoogleNative);
    }

    #[test]
    fn google_native_without_revision_is_still_ingested() {
        // No revision id at all: planner cannot know whether this is
        // already ingested, so it leans toward ingesting once. A
        // duplicate export is acceptable (blob dedupe handles the
        // storage cost).
        let files = [google_doc("doc-2", None)];
        let (s, r) = empty_ctx();
        let p = plan(&files, &ctx(&s, &r));
        assert_eq!(p.to_ingest.len(), 1);
    }

    #[test]
    fn mixed_batch_is_partitioned_correctly() {
        let shas: HashSet<String> = ["sha-known".into()].into_iter().collect();
        let revs: HashSet<String> = ["rev-known".into()].into_iter().collect();
        let files = vec![
            folder("fld"),                              // skip Folder
            binary("bin-new", Some("sha-new")),         // ingest
            binary("bin-known", Some("sha-known")),     // skip AlreadyIngestedSha
            binary("bin-no-sha", None),                 // ingest
            google_doc("doc-new", Some("rev-new")),     // ingest
            google_doc("doc-known", Some("rev-known")), // skip AlreadyIngestedRevision
            DriveFile {
                // skip UnsupportedGoogleNative
                id: "frm-1".into(),
                name: "form".into(),
                mime_type: "application/vnd.google-apps.form".into(),
                size: None,
                head_revision_id: Some("rev-form".into()),
                sha256_checksum: None,
                parents: vec!["folder-1".into()],
            },
        ];
        let p = plan(&files, &ctx(&shas, &revs));

        let ingested_ids: Vec<&str> = p.to_ingest.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ingested_ids, vec!["bin-new", "bin-no-sha", "doc-new"]);

        let skipped: Vec<(&str, SkipReason)> = p
            .skipped
            .iter()
            .map(|s| (s.file.id.as_str(), s.reason))
            .collect();
        assert_eq!(
            skipped,
            vec![
                ("fld", SkipReason::Folder),
                ("bin-known", SkipReason::AlreadyIngestedSha),
                ("doc-known", SkipReason::AlreadyIngestedRevision),
                ("frm-1", SkipReason::UnsupportedGoogleNative),
            ]
        );
    }
}
