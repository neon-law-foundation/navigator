//! Shared container-registry helpers.
//!
//! CI (`deploy.yml`) builds and pushes every navigator image once, to
//! `ghcr.io/<owner>` tagged with an immutable release name. Three callers
//! resolve and verify those tags and must do it identically, so the logic lives
//! here once:
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

/// One parsed immutable release tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReleaseTag {
    year: u32,
    month: u32,
    day: u32,
    variant: ReleaseVariant,
}

/// Same-day release variants, in their compatibility order.
///
/// The legacy `.H` form predates `-hotfix.N`. Keeping it between the base and
/// the current hotfix form makes the ordering deterministic if a registry
/// contains both conventions for one date: base < `.H` < `-hotfix.N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseVariant {
    Base,
    Legacy(u32),
    Hotfix(u32),
}

/// True when `tag` is one of the immutable release shapes: `YY.M.D`, the
/// legacy `YY.M.D.H`, or the current `YY.M.D-hotfix.N`.
///
/// Each component carries **no leading zeros** (the firm-wide version
/// convention: June is `6`). Date and legacy components are one or two digits;
/// the hotfix number is any `u32`, written unpadded so it remains valid semver.
#[must_use]
pub fn is_release_tag(tag: &str) -> bool {
    parse_release_tag(tag).is_some()
}

/// Reject a `--tag` that is not an immutable release tag — rolling `latest`,
/// `buildcache`, or `ci-<sha>` onto a workload is exactly the unauditable
/// deploy we forbid.
pub fn validate_release_tag(tag: &str) -> Result<()> {
    if is_release_tag(tag) {
        Ok(())
    } else {
        bail!(
            "--tag must be an immutable YY.M.D, YY.M.D.H, or YY.M.D-hotfix.N release tag (for example 26.8.19, 26.8.19.4, or 26.8.19-hotfix.14), got `{tag}`"
        );
    }
}

/// The newest immutable release tag in `tags`.
///
/// Comparison is numeric, never lexical. Dates order by `(year, month, day)`;
/// variants on the same date order base < legacy `.H` < `-hotfix.N`, and
/// numbers within each variant order numerically. Non-release tags are ignored.
#[must_use]
pub fn pick_latest_release_tag(tags: &[String]) -> Option<String> {
    tags.iter()
        .filter(|t| is_release_tag(t))
        .max_by_key(|t| release_sort_key(t))
        .cloned()
}

/// Parse a release tag into a numerically comparable key.
fn release_sort_key(tag: &str) -> (u32, u32, u32, u8, u32) {
    let parsed = parse_release_tag(tag).expect("caller filtered with is_release_tag");
    let (variant, number) = match parsed.variant {
        ReleaseVariant::Base => (0, 0),
        ReleaseVariant::Legacy(number) => (1, number),
        ReleaseVariant::Hotfix(number) => (2, number),
    };
    (parsed.year, parsed.month, parsed.day, variant, number)
}

fn parse_release_tag(tag: &str) -> Option<ReleaseTag> {
    let (base, variant) = if let Some((base, number)) = tag.split_once("-hotfix.") {
        if base.contains('-') || number.contains('.') {
            return None;
        }
        (
            base,
            ReleaseVariant::Hotfix(number_component(number, None)?),
        )
    } else {
        (tag, ReleaseVariant::Base)
    };

    let mut groups = base.split('.');
    let year = short_component(groups.next()?)?;
    let month = short_component(groups.next()?)?;
    let day = short_component(groups.next()?)?;
    let trailing = groups.next();
    if groups.next().is_some() {
        return None;
    }

    let variant = match (variant, trailing) {
        (ReleaseVariant::Base, None) => ReleaseVariant::Base,
        (ReleaseVariant::Base, Some(number)) => ReleaseVariant::Legacy(short_component(number)?),
        (ReleaseVariant::Hotfix(number), None) => ReleaseVariant::Hotfix(number),
        (ReleaseVariant::Hotfix(_), Some(_)) => return None,
        (ReleaseVariant::Legacy(_), _) => unreachable!("parser never constructs legacy early"),
    };

    Some(ReleaseTag {
        year,
        month,
        day,
        variant,
    })
}

fn short_component(component: &str) -> Option<u32> {
    number_component(component, Some(2))
}

fn number_component(component: &str, max_len: Option<usize>) -> Option<u32> {
    if component.is_empty()
        || max_len.is_some_and(|max| component.len() > max)
        || (component.len() > 1 && component.starts_with('0'))
        || !component.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    component.parse().ok()
}

/// Resolve the latest published immutable release tag for `<registry>/<image>`.
/// Errors when the package has no release tag yet (e.g. the daily deploy
/// has never run for this fork).
pub fn resolve_latest_tag(registry: &str, image: &str) -> Result<String> {
    let tags = fetch_tags(registry, image)?;
    pick_latest_release_tag(&tags).ok_or_else(|| {
        anyhow::anyhow!(
            "no YY.M.D, YY.M.D.H, or YY.M.D-hotfix.N release tag on {registry}/{image} — has a release published one yet?"
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
    fn is_release_tag_accepts_every_immutable_release_form() {
        // Canonical no-leading-zeros shape.
        assert!(is_release_tag("26.6.23"));
        assert!(is_release_tag("26.6.5")); // single-digit day
        assert!(is_release_tag("0.1.9")); // every component single-digit
        assert!(is_release_tag("26.6.25.14")); // ad-hoc same-day .H suffix
        assert!(is_release_tag("26.6.25.4")); // single-digit hour
        assert!(is_release_tag("26.6.25.0"));
        assert!(is_release_tag("26.8.19-hotfix.14"));
        assert!(is_release_tag("26.8.19-hotfix.0"));
        assert!(is_release_tag("26.8.19-hotfix.214"));
        // Non-releases and malformed shapes stay rejected.
        assert!(!is_release_tag("latest"));
        assert!(!is_release_tag("buildcache"));
        assert!(!is_release_tag("ci-6a5f96a"));
        assert!(!is_release_tag("2026.6.23")); // four-digit year
        assert!(!is_release_tag("26.6")); // too few groups
        assert!(!is_release_tag("26.6.25.14.30")); // too many groups
        assert!(!is_release_tag("26..6")); // empty group
        assert!(!is_release_tag("26.8.19-hotfix")); // missing number
        assert!(!is_release_tag("26.8.19-hotfix.")); // empty number
        assert!(!is_release_tag("26.8.19-hotfix.x")); // nonnumeric number
        assert!(!is_release_tag("26.8.19-hotfix.14.1")); // too many groups
        assert!(!is_release_tag("26.8.19-hotfix.014")); // invalid semver number
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
        // Same-day forms have one explicit compatibility order: the base is
        // first, then the legacy `.H` form, then the current `-hotfix.N` form.
        assert_eq!(
            pick_latest_release_tag(&[
                "26.8.19-hotfix.9".to_string(),
                "26.8.19".to_string(),
                "26.8.19.99".to_string(),
                "26.8.19-hotfix.14".to_string(),
            ]),
            Some("26.8.19-hotfix.14".to_string())
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
        assert!(validate_release_tag("26.8.19-hotfix.14").is_ok());
        assert!(validate_release_tag("latest").is_err());
        assert!(validate_release_tag("buildcache").is_err());
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
