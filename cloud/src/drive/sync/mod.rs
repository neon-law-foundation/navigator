//! Sync orchestrator for the Drive → `documents` + `blobs` pipeline.
//!
//! Lives under `cloud::drive::sync` so the planner stays peer to the
//! REST client that produces its inputs. The orchestrator that owns
//! per-file durability + retry semantics is a Restate workflow
//! (separate commit); this module only contains the **pure** diff
//! function the workflow consumes.
//!
//! The split matters: `store` owns entities + migrations, not network
//! I/O. Anything that holds an [`Arc<dyn DriveAuth>`] or talks to GCS
//! belongs here, not there.

pub mod accessor;
pub mod plan;

pub use accessor::DriveAccessor;
pub use plan::{plan, PlanContext, SkipReason, SkippedItem, SyncPlan};

use std::sync::Arc;

/// Composite Drive config — the accessor + the shared-drive id.
/// Lives in `cloud` so every surface that drives a sync (web, mcp,
/// future workers) can hold it without re-importing the abstraction
/// from each consumer crate. Both pieces are required to run a sync;
/// holding `Option<DriveSyncConfig>` is the canonical way for a
/// caller to express "drive sync is not configured in this deploy".
#[derive(Clone)]
pub struct DriveSyncConfig {
    pub accessor: Arc<dyn DriveAccessor>,
    /// Shared-drive id (e.g. `0AAA…` for `NeonLaw`). Read from
    /// `NAVIGATOR_DRIVE_SHARED_ID` at startup in production deploys.
    pub shared_drive_id: String,
}

/// Build a production [`DriveAccessor`] from the ambient environment —
/// a [`DriveClient`](crate::drive::DriveClient) authenticated by the
/// `navigator-drive-sync@…` service account via Workload Identity
/// (Application Default Credentials). Used by the `workflows-service`
/// worker so the Restate Drive-sync workflow can download files without
/// the accessor being threaded through `web`.
///
/// Fails when no ADC can be resolved (off-GKE, no metadata server).
/// The scope is the same read-only `drive.readonly` door
/// ([`WorkloadIdentitySaAuth`](crate::drive::WorkloadIdentitySaAuth)) —
/// durable sync does not widen it.
pub async fn accessor_from_env() -> Result<Arc<dyn DriveAccessor>, crate::drive::DriveError> {
    let auth = crate::drive::WorkloadIdentitySaAuth::new().await?;
    Ok(Arc::new(crate::drive::DriveClient::new(Arc::new(auth))))
}
