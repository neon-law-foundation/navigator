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
const GITHUB_REPO_PATH: &str = "neon-law-foundation/navigator";
const USER_AGENT: &str = concat!("neon-law-navigator/", env!("CARGO_PKG_VERSION"));

static CACHE: OnceLock<Mutex<Option<CachedStars>>> = OnceLock::new();

#[derive(Debug, Clone)]
struct CachedStars {
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
        Err(err) => {
            tracing::warn!(error = %err, "github star count unavailable");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

async fn star_count() -> Result<u64, GitHubStarsError> {
    let now = Instant::now();

    {
        let guard = cache().lock().await;
        if let Some(cached) = guard.as_ref() {
            if now.duration_since(cached.fetched_at) < CACHE_TTL {
                return Ok(cached.count);
            }
        }
    }

    let count = fetch_stars_from(GITHUB_API_BASE).await?;
    let mut guard = cache().lock().await;
    *guard = Some(CachedStars {
        count,
        fetched_at: Instant::now(),
    });
    Ok(count)
}

fn cache() -> &'static Mutex<Option<CachedStars>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

async fn fetch_stars_from(api_base: &str) -> Result<u64, GitHubStarsError> {
    let url = format!(
        "{}/repos/{GITHUB_REPO_PATH}",
        api_base.trim_end_matches('/')
    );
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

#[cfg(test)]
mod tests {
    use super::fetch_stars_from;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetch_stars_from_reads_fixed_repo_stargazer_count() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/neon-law-foundation/navigator"))
            .and(header("accept", "application/vnd.github+json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "stargazers_count": 42
            })))
            .mount(&server)
            .await;

        let count = fetch_stars_from(&server.uri()).await.unwrap();
        assert_eq!(count, 42);
    }
}
