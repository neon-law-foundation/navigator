//! Shared container-registry helpers.
//!
//! CI (`deploy.yml`) builds and pushes every navigator image once, to
//! `ghcr.io/<owner>` tagged `YY.M.D`. Three callers resolve and verify those
//! tags and must do it identically, so the logic lives here once:
//!
//! - `ship` — rolls **prod** onto a published tag.
//! - `deploy` / `up` — pull the published images into the **local KIND**
//!   cluster instead of building them on the host.
//! - `worktree_env --demo` — pulls the full stack into a per-worktree
//!   demo cluster.
//!
//! ## Why GHCR and not Artifact Registry
//!
//! Navigator is public and AGPL-3.0-only, so there is nothing for a
//! private per-org registry to protect. GHCR publishes from the same workflow
//! that builds the images, on the free tier a public repository gets, and it
//! authenticates pushes with the run's own `GITHUB_TOKEN` — which retires an
//! entire class of failure the GAR path carried: a per-deployment Workload
//! Identity Federation provider, a cross-project `artifactregistry.reader`
//! grant for every environment's service account, and an ADC token on the
//! developer's machine just to list tags.
//!
//! One registry, one namespace, and both deployments pull the same digest —
//! which is what makes staging a proving ring rather than a different build.
//!
//! The images are **public**, so a tag lookup needs no credential at all.
//! GHCR still speaks the Docker Registry v2 API and still wants a bearer
//! token, but it mints one for anyone who asks: `GET /token?scope=…` returns a
//! pull token for a public package. A private package would 401 there, which
//! is the one failure worth naming in the error.

use anyhow::{bail, Context, Result};

/// The registry namespace every image hangs off when `NAVIGATOR_IMAGE_REGISTRY`
/// is unset. A fork overrides the variable rather than editing this constant,
/// keeping the white-label seam intact.
pub const DEFAULT_REGISTRY: &str = "ghcr.io/neon-law-foundation";

/// Resolve the registry prefix from the environment.
///
/// One variable, where the GAR path needed three — a region, a hub project,
/// and a repository name, any two of which could disagree and produce a
/// syntactically valid reference to somewhere no image had ever been pushed.
#[must_use]
pub fn registry_from_env() -> String {
    env_or("NAVIGATOR_IMAGE_REGISTRY", DEFAULT_REGISTRY)
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Full image reference `<registry>/<image>:<tag>`.
#[must_use]
pub fn image_ref(registry: &str, image: &str, tag: &str) -> String {
    format!("{registry}/{image}:{tag}")
}

/// True when `tag` is the `YY.M.D` release shape — three dot-separated
/// numeric groups (e.g. `26.6.23`) — with an optional `.H` fourth group
/// for an ad-hoc same-day release (e.g. `26.6.25.14`).
///
/// Each component carries **no leading zeros** (the firm-wide version
/// convention: June is `6`), so groups are 1–2 digits — a four-digit year
/// (`2026.…`) is rejected.
#[must_use]
pub fn is_release_tag(tag: &str) -> bool {
    let parts: Vec<&str> = tag.split('.').collect();
    (parts.len() == 3 || parts.len() == 4)
        && parts
            .iter()
            .all(|p| (1..=2).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Reject a `--tag` that is not a `YY.M.D[.H]` release tag — rolling a
/// `latest` or a `ci-<sha>` tag onto a workload is exactly the
/// un-auditable deploy we forbid.
pub fn validate_release_tag(tag: &str) -> Result<()> {
    if is_release_tag(tag) {
        Ok(())
    } else {
        bail!(
            "--tag must be a YY.M.D release tag, optionally with an .H suffix for an ad-hoc same-day release (e.g. 26.6.23 or 26.6.25.14), got `{tag}`"
        );
    }
}

/// The newest `YY.M.D[.H]` tag in `tags`. Compares **numerically** per
/// component, not lexicographically: with no-leading-zeros tags the plain
/// string order is wrong (`26.6.5` would sort after `26.6.30`, and
/// `26.6.x` after `26.10.x`), so we parse each group to an integer and
/// take the max by `(year, month, day, hour)`. A bare same-day tag sorts
/// *before* any `.H` ad-hoc extension of it (`26.6.25` < `26.6.25.0` <
/// `26.6.25.14`) via a sentinel hour of `-1`. Non-release tags (`latest`,
/// `ci-<sha>`) are ignored.
#[must_use]
pub fn pick_latest_release_tag(tags: &[String]) -> Option<String> {
    tags.iter()
        .filter(|t| is_release_tag(t))
        .max_by_key(|t| release_sort_key(t))
        .cloned()
}

/// Parse a release tag into a numerically-comparable
/// `(year, month, day, hour)` key. The hour defaults to the sentinel `-1`
/// for a bare three-group tag so it orders before any `.H` extension of
/// the same day. Assumes `is_release_tag(tag)` already held, so every
/// group parses; an unexpected non-numeric group falls back to `0` rather
/// than panicking.
fn release_sort_key(tag: &str) -> (u32, u32, u32, i32) {
    let mut groups = tag.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let year = groups.next().unwrap_or(0);
    let month = groups.next().unwrap_or(0);
    let day = groups.next().unwrap_or(0);
    let hour = groups
        .next()
        .map_or(-1, |h| i32::try_from(h).unwrap_or(i32::MAX));
    (year, month, day, hour)
}

/// Resolve the latest published `YY.M.D[.H]` tag for `<registry>/<image>`.
/// Errors when the package has no release tag yet (e.g. the daily deploy
/// has never run for this fork).
pub fn resolve_latest_tag(registry: &str, image: &str) -> Result<String> {
    let tags = fetch_tags(registry, image)?;
    pick_latest_release_tag(&tags).ok_or_else(|| {
        anyhow::anyhow!(
            "no YY.M.D[.H] release tag on {registry}/{image} — has the daily deploy published one yet?"
        )
    })
}

/// List an image's tags via the Docker Registry v2 API at
/// `https://<host>/v2/<namespace>/<image>/tags/list`.
///
/// Builds a private current-thread runtime so callers stay sync.
///
/// The token exchange is the part worth reading. The v2 API always wants a
/// bearer token, even for a public package — an unauthenticated request gets
/// `401` with a `WWW-Authenticate` challenge rather than the tags. GHCR mints
/// a pull token for a public package to anyone who asks, so this fetches one
/// with no credential and uses it. That is why the whole ADC path is gone:
/// nothing here needs a Google credential, or any credential.
pub fn fetch_tags(registry: &str, image: &str) -> Result<Vec<String>> {
    let (host, namespace) = registry.split_once('/').with_context(|| {
        format!("registry `{registry}` is not `<host>/<namespace>` — cannot list tags")
    })?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for registry tag resolution")?;
    let repository = format!("{namespace}/{image}");
    let token_url =
        format!("https://{host}/token?scope=repository:{repository}:pull&service={host}");
    let list_url = format!("https://{host}/v2/{repository}/tags/list");
    runtime.block_on(async move {
        let client = reqwest::Client::new();
        let token: String = client
            .get(&token_url)
            .send()
            .await
            .context("request registry pull token")?
            .json::<serde_json::Value>()
            .await
            .context("parse registry token response")?
            .get("token")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .with_context(|| format!("{host} returned no pull token for {repository}"))?;
        let resp = client
            .get(&list_url)
            .bearer_auth(token)
            .send()
            .await
            .context("request registry tags/list")?;
        if !resp.status().is_success() {
            bail!(
                "tags/list for {repository} returned {} — is the package published, and is it \
                 public? A private package on {host} needs a token with `read:packages`.",
                resp.status()
            );
        }
        let body: serde_json::Value = resp.json().await.context("parse registry tags/list")?;
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

/// Whether `tag` is published for `<registry>/<image>`. Conservative on
/// error: a failed lookup returns `false` (treat as "can't confirm →
/// don't pin"), so it never green-lights a tag it couldn't verify.
#[must_use]
pub fn tag_exists(registry: &str, image: &str, tag: &str) -> bool {
    fetch_tags(registry, image).is_ok_and(|tags| tags.iter().any(|t| t == tag))
}

/// Bail unless `tag` is published for `<registry>/<image>`. Used to fail
/// fast — before any `kubectl set image` / `docker pull` — when an image
/// is missing the requested tag (which would otherwise wedge a deployment
/// in `ImagePullBackOff`). Distinguishes a lookup error (network / auth)
/// from an honestly-absent tag.
pub fn ensure_tag_published(registry: &str, image: &str, tag: &str) -> Result<()> {
    let tags = fetch_tags(registry, image)
        .with_context(|| format!("check {registry}/{image}:{tag} is published"))?;
    if tags.iter().any(|t| t == tag) {
        Ok(())
    } else {
        bail!(
            "{registry}/{image}:{tag} is not published — publish it via the daily deploy \
             (or pick a tag that exists) first."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_release_tag_accepts_yy_m_d_and_optional_h() {
        // Canonical no-leading-zeros shape.
        assert!(is_release_tag("26.6.23"));
        assert!(is_release_tag("26.6.5")); // single-digit day
        assert!(is_release_tag("0.1.9")); // every component single-digit
        assert!(is_release_tag("26.6.25.14")); // ad-hoc same-day .H suffix
        assert!(is_release_tag("26.6.25.4")); // single-digit hour
        assert!(is_release_tag("26.6.25.0"));
        // Non-releases and malformed shapes stay rejected.
        assert!(!is_release_tag("latest"));
        assert!(!is_release_tag("ci-6a5f96a"));
        assert!(!is_release_tag("2026.6.23")); // four-digit year
        assert!(!is_release_tag("26.6")); // too few groups
        assert!(!is_release_tag("26.6.25.14.30")); // too many groups
        assert!(!is_release_tag("26..6")); // empty group
    }

    #[test]
    fn pick_latest_release_tag_takes_the_newest_and_ignores_non_releases() {
        let tags = vec![
            "latest".to_string(),
            "26.6.10".to_string(),
            "ci-deadbeef".to_string(),
            "26.6.23".to_string(),
            "26.5.31".to_string(),
        ];
        assert_eq!(pick_latest_release_tag(&tags), Some("26.6.23".to_string()));
        // An ad-hoc `.H` release sorts after the bare same-day tag.
        assert_eq!(
            pick_latest_release_tag(&[
                "26.6.25".to_string(),
                "26.6.25.14".to_string(),
                "26.6.10".to_string(),
            ]),
            Some("26.6.25.14".to_string())
        );
        // Regression: numeric, not lexical, ordering. A plain string `max`
        // would pick `26.6.5` over `26.6.30` ("5" > "3") and `26.6.x` over
        // `26.10.x` ("6" > "1") — both chronologically wrong.
        assert_eq!(
            pick_latest_release_tag(&[
                "26.6.5".to_string(),
                "26.6.30".to_string(),
                "26.10.5".to_string(),
            ]),
            Some("26.10.5".to_string())
        );
        // A later month wins even though its day is smaller.
        assert_eq!(
            pick_latest_release_tag(&["26.6.30".to_string(), "26.7.1".to_string()]),
            Some("26.7.1".to_string())
        );
        assert_eq!(
            pick_latest_release_tag(&["latest".to_string(), "ci-x".to_string()]),
            None
        );
    }

    #[test]
    fn validate_release_tag_rejects_non_release() {
        assert!(validate_release_tag("26.6.23").is_ok());
        assert!(validate_release_tag("latest").is_err());
        assert!(validate_release_tag("ci-abc").is_err());
    }

    #[test]
    fn image_ref_composes_the_published_path() {
        let reg = DEFAULT_REGISTRY;
        assert_eq!(
            image_ref(reg, "navigator-web", "26.6.23"),
            "ghcr.io/neon-law-foundation/navigator-web:26.6.23"
        );
    }

    /// The default namespace is the Foundation's GHCR org, and it is a
    /// `<host>/<namespace>` pair rather than a bare host.
    ///
    /// `fetch_tags` splits on the first `/` to build both the token scope and
    /// the tags URL, so a value with no slash would produce a token request
    /// for the wrong repository and a `tags/list` against a path that does not
    /// exist — two confusing 404s rather than one clear error.
    #[test]
    fn the_default_registry_is_a_host_and_a_namespace() {
        assert_eq!(DEFAULT_REGISTRY, "ghcr.io/neon-law-foundation");
        let (host, namespace) = DEFAULT_REGISTRY
            .split_once('/')
            .expect("the registry names a host and a namespace");
        assert_eq!(host, "ghcr.io");
        assert_eq!(namespace, "neon-law-foundation");
    }

    /// A deployment that names no registry falls back to the default rather
    /// than composing an empty prefix.
    ///
    /// An empty value here would yield `/navigator-web:26.6.23`, which Docker
    /// reads as an implicit-Hub reference and would pull a stranger's image.
    /// That is the failure the fallback exists to prevent, so a blank is
    /// treated as unset rather than honoured.
    #[test]
    fn a_blank_registry_falls_back_to_the_default() {
        for blank in [None, Some(""), Some("   ")] {
            assert_eq!(super::super::ship::images_registry(blank), DEFAULT_REGISTRY);
        }
    }

    /// A fork overrides the one variable and every image follows.
    #[test]
    fn a_fork_overrides_the_whole_namespace() {
        assert_eq!(
            super::super::ship::images_registry(Some("ghcr.io/acme")),
            "ghcr.io/acme"
        );
    }
}
