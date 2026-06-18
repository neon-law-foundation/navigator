//! Thin trait abstracting the Drive operations the sync orchestrator
//! needs. Lets the orchestrator (which lives in `store::drive_syncs`,
//! one layer up the dependency graph) consume Drive without touching
//! `reqwest` directly — and lets its tests run without `wiremock`.
//!
//! The trait is intentionally minimal: just the two methods a sync
//! actually calls. Anything else on [`DriveClient`] (auth flows, the
//! `drives` listing) stays accessible to direct callers.
//!
//! [`DriveClient`]: super::super::client::DriveClient

use async_trait::async_trait;

use super::super::client::{DownloadedBytes, DriveClient, DriveFile};
use super::super::DriveError;

/// What the sync orchestrator needs from a Drive backend.
#[async_trait]
pub trait DriveAccessor: Send + Sync {
    /// List the files immediately under `folder_id` inside
    /// `drive_id`. Mirrors [`DriveClient::list_folder_files`].
    async fn list_folder_files(
        &self,
        drive_id: &str,
        folder_id: &str,
    ) -> Result<Vec<DriveFile>, DriveError>;

    /// Fetch one file's bytes (binary via `alt=media`, Google-native
    /// via `/export`). Mirrors [`DriveClient::download_file`].
    async fn download_file(
        &self,
        file_id: &str,
        mime_type: &str,
    ) -> Result<DownloadedBytes, DriveError>;
}

#[async_trait]
impl DriveAccessor for DriveClient {
    async fn list_folder_files(
        &self,
        drive_id: &str,
        folder_id: &str,
    ) -> Result<Vec<DriveFile>, DriveError> {
        DriveClient::list_folder_files(self, drive_id, folder_id).await
    }

    async fn download_file(
        &self,
        file_id: &str,
        mime_type: &str,
    ) -> Result<DownloadedBytes, DriveError> {
        DriveClient::download_file(self, file_id, mime_type).await
    }
}
