//! Same-origin GitHub star-count endpoint for the public footer CTA.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const CACHE_TTL: Duration = Duration::from_hours(1);
const GITHUB_API_BASE: &str = "https://api.github.com";
const USER_AGENT: &str = concat!("neon-law-navigator/", env!("CARGO_PKG_VERSION"));

static CACHE: OnceLock<Mutex<Option<CachedStars>>> = OnceLock::new();

#[derive(Debug, Clone)]
struct CachedStars {
    repo: String,
    count: u64,
    fetched_at: Instant,
}

#[derive(Debug, Deserialize)]
struct GitHubRepo {
    stargazers_count: u64,
}

#[derive(Debug, Serialize)]
struct StarCount {
    stargazers_count: u64,
}

#[derive(Debug, thiserror::Error)]
enum GitHubStarsError {
    #[error("foundation GitHub URL is disabled or unsupported")]
    UnsupportedRepoUrl,
    #[error("GitHub API request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("GitHub API returned {0}")]
    Upstream(StatusCode),
}

pub async fn handler() -> impl IntoResponse {
    match star_count().await {
        Ok(count) => (
            [(header::CACHE_CONTROL, "public, max-age=3600")],
            Json(StarCount {
                stargazers_count: count,
            }),
        )
            .into_response(),
        Err(GitHubStarsError::UnsupportedRepoUrl) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::warn!(error = %err, "github star count unavailable");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

async fn star_count() -> Result<u64, GitHubStarsError> {
    let repo = repo_path_from_url(
        views::brand::foundation_github_url().ok_or(GitHubStarsError::UnsupportedRepoUrl)?,
    )
    .ok_or(GitHubStarsError::UnsupportedRepoUrl)?;
    let now = Instant::now();

    {
        let guard = cache().lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.repo == repo && now.duration_since(cached.fetched_at) < CACHE_TTL {
                return Ok(cached.count);
            }
        }
    }

    let count = fetch_stars_from(GITHUB_API_BASE, &repo).await?;
    let mut guard = cache().lock().await;
    *guard = Some(CachedStars {
        repo,
        count,
        fetched_at: Instant::now(),
    });
    Ok(count)
}

fn cache() -> &'static Mutex<Option<CachedStars>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

async fn fetch_stars_from(api_base: &str, repo: &str) -> Result<u64, GitHubStarsError> {
    let url = format!("{}/repos/{}", api_base.trim_end_matches('/'), repo);
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?
        .get(url)
        .header(header::ACCEPT, "application/vnd.github+json")
        .header(header::USER_AGENT, USER_AGENT)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(GitHubStarsError::Upstream(response.status()));
    }
    Ok(response.json::<GitHubRepo>().await?.stargazers_count)
}

fn repo_path_from_url(url: &str) -> Option<String> {
    let path = url
        .trim()
        .strip_prefix("https://github.com/")
        .or_else(|| url.trim().strip_prefix("http://github.com/"))?
        .trim_end_matches('/');
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let raw_repo = parts.next()?;
    let repo = raw_repo.strip_suffix(".git").unwrap_or(raw_repo);
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    if [owner, repo].iter().all(|part| {
        part.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    }) {
        Some(format!("{owner}/{repo}"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{fetch_stars_from, repo_path_from_url};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn repo_path_from_url_accepts_github_repo_root() {
        assert_eq!(
            repo_path_from_url("https://github.com/neon-law-foundation/Navigator"),
            Some("neon-law-foundation/Navigator".to_string())
        );
    }

    #[test]
    fn repo_path_from_url_rejects_non_github_urls() {
        assert_eq!(repo_path_from_url("https://example.com/owner/repo"), None);
    }

    #[test]
    fn repo_path_from_url_rejects_nested_github_paths() {
        assert_eq!(
            repo_path_from_url("https://github.com/owner/repo/tree/main"),
            None
        );
    }

    #[tokio::test]
    async fn fetch_stars_from_reads_stargazer_count() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo"))
            .and(header("accept", "application/vnd.github+json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "stargazers_count": 42
            })))
            .mount(&server)
            .await;

        let count = fetch_stars_from(&server.uri(), "owner/repo").await.unwrap();
        assert_eq!(count, 42);
    }
}
