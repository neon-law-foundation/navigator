//! Entrypoint for one isolated, read-only issue-triage runner Job.
//!
//! The Job controller supplies the task and repository-scoped clone URL as
//! separate environment values. The latter is credential-bearing and is never
//! rendered in output. The only source-bearing stdout is the bounded sentinel
//! envelope consumed directly by the controller's GitHub-comment call.

use std::process::ExitCode;

use github_webhooks::harness::ClaudeCodeHarness;
use github_webhooks::runner::{
    execute_triage, format_triage_result, RunnerError, RunnerTask, RunnerTaskError,
};

const TASK_ENV: &str = "NAVIGATOR_RUNNER_TASK";
const CLONE_URL_ENV: &str = "NAVIGATOR_GITHUB_CLONE_URL";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Every RunnerError is source- and credential-free by contract.
            eprintln!("triage runner failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), RunnerError> {
    let task = std::env::var(TASK_ENV).map_err(|_| RunnerError::Task)?;
    let task = RunnerTask::from_json(&task).map_err(task_error)?;
    // The clone URL embeds a short-lived installation token. Keep it out of
    // every error and pass it only to the checkout helper.
    let clone_url = std::env::var(CLONE_URL_ENV).map_err(|_| RunnerError::Task)?;
    if clone_url.is_empty() {
        return Err(RunnerError::Task);
    }
    let harness = ClaudeCodeHarness::from_env().map_err(|_| RunnerError::Harness)?;
    let result = execute_triage(&task, &clone_url, &harness).await?;
    let envelope = format_triage_result(&result)?;
    print!("{envelope}");
    Ok(())
}

fn task_error(_: RunnerTaskError) -> RunnerError {
    RunnerError::Task
}
