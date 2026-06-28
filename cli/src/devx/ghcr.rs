//! Shared GitHub Container Registry (ghcr.io) helpers.
//!
//! CI (`deploy.yml`) builds and publishes every navigator image to
//! public `ghcr.io` tagged `YY.MM.DD`. Three callers resolve and verify
//! those tags and must do it identically, so the logic lives here once:
//!
//! - `ship` — rolls **prod** onto a published tag.
//! - `deploy` / `up` — pull the published images into the **local KIND**
//!   cluster instead of building them on the host.
//! - `worktree_env --demo` — pulls the full stack into a per-worktree
//!   demo cluster.
//!
//! The registry is public, so every read here is anonymous: mint a
//! pull-scoped token, then hit the Docker Registry v2 API. Nothing in
//! this module needs a credential.

use anyhow::{bail, Context, Result};

/// The canonical ghcr owner. The default when `NAVIGATOR_GHCR_OWNER` is
/// unset — a fork overrides it via that env var rather than editing this
/// constant, keeping the white-label seam intact.
pub const DEFAULT_GHCR_OWNER: &str = "neon-law-foundation";

/// Resolve the ghcr owner (org/user) from the environment, lowercased.
/// ghcr image names are lowercase; lowercasing a mixed-case org (e.g.
/// `Neon-Law-Foundation`) keeps it resolvable. Defaults to the canonical
/// org; a fork overrides via `NAVIGATOR_GHCR_OWNER`.
#[must_use]
pub fn owner_from_env() -> String {
    std::env::var("NAVIGATOR_GHCR_OWNER")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_GHCR_OWNER.to_string())
        .to_ascii_lowercase()
}

/// The `ghcr.io/<owner>` registry prefix.
#[must_use]
pub fn registry(owner: &str) -> String {
    format!("ghcr.io/{owner}")
}

/// Full image reference `ghcr.io/<owner>/<image>:<tag>`.
#[must_use]
pub fn image_ref(owner: &str, image: &str, tag: &str) -> String {
    format!("{}/{image}:{tag}", registry(owner))
}

/// True when `tag` is the `YY.MM.DD` release shape — three dot-separated
/// two-digit groups (e.g. `26.06.23`) — with an optional `.HH` fourth
/// group for an ad-hoc same-day release (e.g. `26.06.25.14`).
#[must_use]
pub fn is_release_tag(tag: &str) -> bool {
    let parts: Vec<&str> = tag.split('.').collect();
    (parts.len() == 3 || parts.len() == 4)
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Reject a `--tag` that is not a `YY.MM.DD[.HH]` release tag — rolling a
/// `latest` or a `ci-<sha>` tag onto a workload is exactly the
/// un-auditable deploy we forbid.
pub fn validate_release_tag(tag: &str) -> Result<()> {
    if is_release_tag(tag) {
        Ok(())
    } else {
        bail!(
            "--tag must be a YY.MM.DD release tag, optionally with an .HH suffix for an ad-hoc same-day release (e.g. 26.06.23 or 26.06.25.14), got `{tag}`"
        );
    }
}

/// The newest `YY.MM.DD[.HH]` tag in `tags`. Zero-padded `YY.MM.DD` sorts
/// lexicographically the same as chronologically, and an `.HH` ad-hoc
/// suffix (e.g. `26.06.25.14`) sorts after the bare same-day tag it
/// extends, so `max` is the latest. Non-release tags (`latest`,
/// `ci-<sha>`) are ignored.
#[must_use]
pub fn pick_latest_release_tag(tags: &[String]) -> Option<String> {
    tags.iter().filter(|t| is_release_tag(t)).max().cloned()
}

/// Resolve the latest published `YY.MM.DD[.HH]` tag for
/// `ghcr.io/<owner>/<image>`. Errors when the package has no release tag
/// yet (e.g. the daily deploy has never run for this fork).
pub fn resolve_latest_tag(owner: &str, image: &str) -> Result<String> {
    let tags = fetch_tags(owner, image)?;
    pick_latest_release_tag(&tags).ok_or_else(|| {
        anyhow::anyhow!(
            "no YY.MM.DD[.HH] release tag on ghcr.io/{owner}/{image} — has the daily deploy published one yet?"
        )
    })
}

/// List a public ghcr package's tags anonymously: mint a pull-scoped
/// token, then GET `/v2/<owner>/<image>/tags/list`. Public packages need
/// no credential — the same path GKE / KIND anonymous pulls take. Builds
/// a private current-thread runtime so callers stay sync.
pub fn fetch_tags(owner: &str, image: &str) -> Result<Vec<String>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for ghcr tag resolution")?;
    let repo = format!("{owner}/{image}");
    runtime.block_on(async move {
        let client = reqwest::Client::new();
        let token_url = format!("https://ghcr.io/token?scope=repository:{repo}:pull");
        let token_body: serde_json::Value = client
            .get(&token_url)
            .send()
            .await
            .context("request ghcr pull token")?
            .json()
            .await
            .context("parse ghcr token response")?;
        let token = token_body
            .get("token")
            .and_then(serde_json::Value::as_str)
            .context("ghcr token missing from response")?;
        let list_url = format!("https://ghcr.io/v2/{repo}/tags/list");
        let resp = client
            .get(&list_url)
            .bearer_auth(token)
            .send()
            .await
            .context("request ghcr tags/list")?;
        if !resp.status().is_success() {
            bail!(
                "ghcr tags/list for {repo} returned {} — is the package public?",
                resp.status()
            );
        }
        let body: serde_json::Value = resp.json().await.context("parse ghcr tags/list")?;
        let tags = body
            .get("tags")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(tags)
    })
}

/// Whether `tag` is published for `ghcr.io/<owner>/<image>`. Conservative
/// on error: a failed lookup returns `false` (treat as "can't confirm →
/// don't pin"), so it never green-lights a tag it couldn't verify.
#[must_use]
pub fn tag_exists(owner: &str, image: &str, tag: &str) -> bool {
    fetch_tags(owner, image).is_ok_and(|tags| tags.iter().any(|t| t == tag))
}

/// Bail unless `tag` is published for `ghcr.io/<owner>/<image>`. Used to
/// fail fast — before any `kubectl set image` / `docker pull` — when an
/// image is missing the requested tag (which would otherwise wedge a
/// deployment in `ImagePullBackOff`). Distinguishes a lookup error
/// (network) from an honestly-absent tag.
pub fn ensure_tag_published(owner: &str, image: &str, tag: &str) -> Result<()> {
    let tags = fetch_tags(owner, image)
        .with_context(|| format!("check ghcr.io/{owner}/{image}:{tag} is published"))?;
    if tags.iter().any(|t| t == tag) {
        Ok(())
    } else {
        bail!(
            "ghcr.io/{owner}/{image}:{tag} is not published — publish it via the daily deploy \
             (or pick a tag that exists) first."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_release_tag_accepts_yy_mm_dd_and_optional_hh() {
        assert!(is_release_tag("26.06.23"));
        assert!(is_release_tag("00.01.09"));
        assert!(is_release_tag("26.06.25.14")); // ad-hoc same-day .HH suffix
        assert!(is_release_tag("26.06.25.00"));
        assert!(!is_release_tag("latest"));
        assert!(!is_release_tag("ci-6a5f96a"));
        assert!(!is_release_tag("2026.06.23")); // four-digit year
        assert!(!is_release_tag("26.6.23")); // unpadded month
        assert!(!is_release_tag("26.06")); // too few groups
        assert!(!is_release_tag("26.06.25.4")); // unpadded hour
        assert!(!is_release_tag("26.06.25.14.30")); // too many groups
    }

    #[test]
    fn pick_latest_release_tag_takes_the_newest_and_ignores_non_releases() {
        let tags = vec![
            "latest".to_string(),
            "26.06.10".to_string(),
            "ci-deadbeef".to_string(),
            "26.06.23".to_string(),
            "26.05.31".to_string(),
        ];
        assert_eq!(pick_latest_release_tag(&tags), Some("26.06.23".to_string()));
        // An ad-hoc `.HH` release sorts after the bare same-day tag.
        assert_eq!(
            pick_latest_release_tag(&[
                "26.06.25".to_string(),
                "26.06.25.14".to_string(),
                "26.06.10".to_string(),
            ]),
            Some("26.06.25.14".to_string())
        );
        assert_eq!(
            pick_latest_release_tag(&["latest".to_string(), "ci-x".to_string()]),
            None
        );
    }

    #[test]
    fn validate_release_tag_rejects_non_release() {
        assert!(validate_release_tag("26.06.23").is_ok());
        assert!(validate_release_tag("latest").is_err());
        assert!(validate_release_tag("ci-abc").is_err());
    }

    #[test]
    fn owner_default_and_refs() {
        // image_ref composes the canonical public path.
        assert_eq!(
            image_ref(DEFAULT_GHCR_OWNER, "navigator-web", "26.06.23"),
            "ghcr.io/neon-law-foundation/navigator-web:26.06.23"
        );
        assert_eq!(
            registry("neon-law-foundation"),
            "ghcr.io/neon-law-foundation"
        );
    }
}
