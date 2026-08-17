//! Boundary for an isolated issue-triage Job.
//!
//! The durable worker calls this boundary from one `ctx.run` side effect.  The
//! issue thread, clone credential, and grounded plan therefore stay in the
//! short-lived Job path and never become Restate state or telemetry.

use async_trait::async_trait;
use thiserror::Error;

use crate::github::{CloneUrl, Issue, IssueComment, RepositoryRef};
use crate::runner::{RunnerError, RunnerTask, TriageRun};

/// Source-bearing invocation data passed only to an isolated runner Job.
///
/// This intentionally implements neither `Debug` nor `Serialize`: callers
/// must not place issue content or a credential-bearing clone URL in logs,
/// workflow state, or a durable result.
pub struct TriageJobRequest {
    pub repository: RepositoryRef,
    pub issue: Issue,
    pub comments: Vec<IssueComment>,
    pub task: RunnerTask,
    pub clone_url: CloneUrl,
}

/// An isolated runner Job capable of grounding one issue and returning its
/// bounded plan. The workflow must immediately post the plan and return only
/// metadata from its surrounding `ctx.run` step.
#[async_trait]
pub trait TriageJob: Send + Sync {
    async fn execute(&self, request: TriageJobRequest) -> Result<TriageRun, TriageJobError>;
}

/// Safe classification for the runner-Job boundary.
#[derive(Debug, Error)]
pub enum TriageJobError {
    #[error("triage runner is unavailable")]
    Unavailable,
    #[error("triage runner rejected its input")]
    InvalidInput,
    #[error("triage runner failed")]
    Failed,
}

impl From<RunnerError> for TriageJobError {
    fn from(error: RunnerError) -> Self {
        match error {
            RunnerError::Task | RunnerError::InvalidTriageResult => Self::InvalidInput,
            RunnerError::Harness | RunnerError::Gate | RunnerError::Checkout => Self::Unavailable,
            RunnerError::WorktreeNotEmpty
            | RunnerError::UnsignedCommit
            | RunnerError::UnexpectedChange => Self::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TriageJobError;
    use crate::runner::RunnerError;

    #[test]
    fn runner_failures_keep_retryable_and_terminal_classes_separate() {
        for error in [
            RunnerError::Harness,
            RunnerError::Gate,
            RunnerError::Checkout,
        ] {
            assert!(matches!(
                TriageJobError::from(error),
                TriageJobError::Unavailable
            ));
        }
        for error in [RunnerError::Task, RunnerError::InvalidTriageResult] {
            assert!(matches!(
                TriageJobError::from(error),
                TriageJobError::InvalidInput
            ));
        }
        for error in [
            RunnerError::WorktreeNotEmpty,
            RunnerError::UnsignedCommit,
            RunnerError::UnexpectedChange,
        ] {
            assert!(matches!(
                TriageJobError::from(error),
                TriageJobError::Failed
            ));
        }
    }
}
