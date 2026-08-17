//! `gcloud auth print-access-token` as a credential of last resort.
//!
//! `google-cloud-auth` supports fewer credential shapes than the `gcloud`
//! CLI does, and the gap bites in two places that look unrelated and are
//! the same thing:
//!
//! - **CI.** Workload Identity Federation — what
//!   `google-github-actions/auth` configures — writes an `external_account`
//!   credential file, which `google-cloud-auth` rejects outright with
//!   `unsupported account external_account`. Ship run 144483561 died there
//!   on the registry tag preflight, and release 26.8.12's
//!   `publish-cli-archives` jobs died there opening the GCS client, in both
//!   cases *after* the same job had already authenticated `gcloud` from
//!   that very file.
//! - **Operators.** `gcloud auth login` and `gcloud auth
//!   application-default login` write to separate stores, so a fresh login
//!   with stale ADC fails the same call — the recurring `missing field
//!   access_token` confusion in `docs/cloud-operations.md`.
//!
//! Both are cases where a perfectly good credential is already sitting in
//! `gcloud`. Ask it rather than failing the command.
//!
//! Two consumers share this module: [`gcs`](crate::gcs), through
//! [`GcloudTokenSourceProvider`], and the CLI's `devx gcp` REST client,
//! through [`access_token`] directly.

use std::process::Command;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};

/// Mint an access token with `gcloud auth print-access-token`.
///
/// Minted per call rather than cached: these tokens expire in about an
/// hour, a ship or an upload can outlive that, and the subprocess costs far
/// less than a half-finished rollout failing on an expired bearer.
pub fn access_token() -> Result<String> {
    let out = Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .context("run `gcloud auth print-access-token`")?;
    if !out.status.success() {
        return Err(anyhow!(
            "`gcloud auth print-access-token` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if token.is_empty() {
        return Err(anyhow!("`gcloud auth print-access-token` printed nothing"));
    }
    Ok(token)
}

/// Confirm `gcloud` can actually mint a token *now*, so an unauthenticated
/// or absent CLI surfaces at client-construction time with the ADC error
/// still attached, rather than midway through a flow as an opaque
/// per-request auth failure.
pub fn probe() -> Result<()> {
    access_token().map(|_| ())
}

/// A [`google_cloud_token::TokenSourceProvider`] backed by [`access_token`],
/// so `google-cloud-storage`'s client can present the credential `gcloud`
/// already holds.
///
/// This is a token-only identity: it carries no private key and no service
/// account email, so a client built on it can call the JSON API but cannot
/// locally sign a V4 URL.
#[derive(Debug, Clone, Copy)]
pub struct GcloudTokenSourceProvider;

impl google_cloud_token::TokenSourceProvider for GcloudTokenSourceProvider {
    fn token_source(&self) -> Arc<dyn google_cloud_token::TokenSource> {
        Arc::new(GcloudTokenSource {
            fetch: access_token,
        })
    }
}

/// The token source itself. `fetch` is a field rather than a direct call so
/// the header shape can be tested on a machine with no `gcloud` login.
#[derive(Debug, Clone, Copy)]
struct GcloudTokenSource {
    fetch: fn() -> Result<String>,
}

#[async_trait::async_trait]
impl google_cloud_token::TokenSource for GcloudTokenSource {
    async fn token(&self) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // `spawn_blocking` because this runs inside the storage client's
        // request path; a subprocess on the reactor thread would stall
        // every other in-flight call.
        let token = tokio::task::spawn_blocking(self.fetch)
            .await
            .map_err(|e| format!("join gcloud token task: {e}"))?
            .map_err(|e| format!("{e:#}"))?;
        // `google-cloud-storage` puts this value straight into the
        // `Authorization` header, so it has to carry the scheme the bare
        // CLI output omits. Without the prefix every request 401s.
        Ok(format!("Bearer {token}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{access_token, probe, GcloudTokenSource};
    use google_cloud_token::TokenSource;

    /// The header shape is the difference between a working upload and a
    /// 401: `google-cloud-storage` uses this string verbatim as
    /// `Authorization`, while `gcloud` prints the bare token.
    #[tokio::test]
    async fn the_token_source_emits_a_bearer_header_value() {
        let source = GcloudTokenSource {
            fetch: || Ok("ya29.a0-test".into()),
        };
        assert_eq!(source.token().await.unwrap(), "Bearer ya29.a0-test");
    }

    /// A `gcloud` that stops working mid-flow (an expired login, a revoked
    /// account) must surface its own words, not an empty string the
    /// storage client would send as a header.
    #[tokio::test]
    async fn a_failing_fetch_surfaces_its_reason() {
        let source = GcloudTokenSource {
            fetch: || Err(anyhow::anyhow!("reauthentication required")),
        };
        let err = source.token().await.unwrap_err();
        assert!(
            err.to_string().contains("reauthentication required"),
            "got {err}"
        );
    }

    /// `access_token` is the whole contract of the fallback, and both of its
    /// outcomes are legitimate depending on the machine: CI and a logged-in
    /// operator get a token, a bare developer box does not. What must never
    /// happen is the ambiguous middle — a blank token reported as success,
    /// which would be selected as the credential and then 401 every request
    /// with no explanation.
    ///
    /// Deliberately no `set_var` here: mutating the environment leaks across
    /// nextest's shared process.
    #[test]
    fn a_gcloud_fetch_either_yields_a_real_token_or_explains_itself() {
        match access_token() {
            Ok(token) => assert!(
                !token.trim().is_empty(),
                "a successful fetch must carry a non-empty token"
            ),
            Err(error) => assert!(
                !format!("{error:#}").trim().is_empty(),
                "a failed fetch must say why"
            ),
        }
    }

    /// `probe` is what decides whether the fallback is offered at all, so it
    /// must agree with `access_token` rather than reporting its own opinion.
    #[test]
    fn probe_agrees_with_access_token() {
        assert_eq!(
            probe().is_ok(),
            access_token().is_ok(),
            "probe must not advertise a fallback that access_token cannot honor"
        );
    }
}
