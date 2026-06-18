//! Tag the locally-built `navigator-web:dev` image and push it to the
//! Navigator Artifact Registry repo provisioned by
//! [`super::artifact_registry`]. This is the smallest possible bridge
//! between `devx image` (which produces `navigator-web:dev`) and the
//! remote GAR repo at
//! `us-west4-docker.pkg.dev/<project>/navigator/navigator-web:<tag>`.
//!
//! ## Auth
//!
//! Docker authenticates to GAR via the host's docker credential
//! helper, configured once with
//! `gcloud auth configure-docker us-west4-docker.pkg.dev`. We never
//! mint a token ourselves; if push fails with 401 the operator
//! re-runs that gcloud command.
//!
//! ## No tests for the shell-out path
//!
//! `tag` and `push` shell out to the docker CLI, which would need a
//! real daemon to exercise. The only piece worth unit-testing is the
//! URL builder — see the `tests` module below.

use std::process::Command;

use anyhow::{bail, Context, Result};

use super::artifact_registry::REPO_ID;
use super::DEFAULT_REGION;

/// The local image tag `devx image` produces. Hardcoded for now; if
/// we ever build multiple web variants, lift this onto a parameter.
pub const SOURCE_IMAGE: &str = "navigator-web:dev";

/// The image name (without tag) we push as. The full destination is
/// `<region>-docker.pkg.dev/<project>/<repo>/<image>:<tag>`.
pub const PUSHED_IMAGE_NAME: &str = "navigator-web";

/// Build the Artifact Registry push URL for `project_id` at `tag`.
/// The shape must match what `artifact_registry::ensure_repo` creates;
/// both functions read [`DEFAULT_REGION`] and [`REPO_ID`].
#[must_use]
pub fn image_url(project_id: &str, tag: &str) -> String {
    format!("{DEFAULT_REGION}-docker.pkg.dev/{project_id}/{REPO_ID}/{PUSHED_IMAGE_NAME}:{tag}")
}

/// `docker tag` the local `navigator-web:dev` to the GAR URL, then
/// `docker push` it. Returns the URL that was pushed so the caller
/// can log it.
pub fn run(project_id: &str, tag: &str) -> Result<String> {
    let target = image_url(project_id, tag);
    eprintln!("==> docker tag {SOURCE_IMAGE} {target}");
    run_cmd(
        Command::new("docker")
            .arg("tag")
            .arg(SOURCE_IMAGE)
            .arg(&target),
    )?;
    eprintln!("==> docker push {target}");
    run_cmd(Command::new("docker").arg("push").arg(&target))?;
    Ok(target)
}

fn run_cmd(cmd: &mut Command) -> Result<()> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let status = cmd.status().with_context(|| format!("spawn {program}"))?;
    if !status.success() {
        bail!("command failed ({status}): {program}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::image_url;

    #[test]
    fn image_url_targets_the_navigator_gar_repo() {
        let url = image_url("my-project", "fa989a6");
        assert_eq!(
            url,
            "us-west4-docker.pkg.dev/my-project/navigator/navigator-web:fa989a6"
        );
    }
}
