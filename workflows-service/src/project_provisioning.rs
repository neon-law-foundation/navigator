//! Durable Project repo provisioning.
//!
//! Matter creation treats the per-Project git repo as required
//! infrastructure. This Restate workflow wraps the repo creation + database
//! stamp in one `ctx.run` step so replay reuses the journaled result instead
//! of re-running `git init` or re-stamping `projects.git_initialized_at`.

use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use store::Db;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProvisionProjectRepoRequest {
    pub project_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ProvisionProjectRepoResponse {
    pub project_id: Uuid,
    pub path: String,
}

#[restate_sdk::workflow]
#[name = "ProjectProvisioning"]
pub trait ProjectProvisioning {
    async fn provision(
        req: Json<ProvisionProjectRepoRequest>,
    ) -> Result<Json<ProvisionProjectRepoResponse>, HandlerError>;
}

#[derive(Clone)]
pub struct ProjectProvisioningService {
    db: Db,
}

impl ProjectProvisioningService {
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl ProjectProvisioning for ProjectProvisioningService {
    async fn provision(
        &self,
        ctx: WorkflowContext<'_>,
        req: Json<ProvisionProjectRepoRequest>,
    ) -> Result<Json<ProvisionProjectRepoResponse>, HandlerError> {
        let project_id = req.into_inner().project_id;
        let db = self.db.clone();

        let response = ctx
            .run(move || async move {
                let path = store::projects::provision_repo_hard_from_env(&db, project_id)
                    .await
                    .map_err(|e| HandlerError::from(anyhow::anyhow!(e)))?;
                Ok(Json(ProvisionProjectRepoResponse {
                    project_id,
                    path: path.to_string_lossy().into_owned(),
                }))
            })
            .name("create-project-repo")
            .await?
            .into_inner();

        Ok(Json(response))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn durable_step_name_is_stable() {
        let source = include_str!("project_provisioning.rs");
        assert!(
            source.contains(".name(\"create-project-repo\")"),
            "project repo provisioning must keep the stable ctx.run step name",
        );
    }
}
