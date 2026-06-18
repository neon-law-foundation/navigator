//! Polite, rate-limited HTTP fetching of NRS chapter pages.
//!
//! Good-citizen rules (the source publishes no `Crawl-delay`, and
//! `/NRS/` is not disallowed by its `robots.txt` — checked 2026-06-06):
//! a descriptive [`crate::USER_AGENT`], a request timeout, and a caller-
//! controlled pause between chapters so we fetch sequentially and never
//! hammer the site.
//!
//! The legislature serves windows-1252 (MS-Word-filtered HTML), so the
//! body is decoded with `encoding_rs` honoring the `Content-Type`
//! charset, defaulting to windows-1252. [`Fetcher`] implements the
//! [`ChapterSource`] seam so [`crate::sync`] can be driven from a fixture
//! in tests without touching the network.

use std::time::Duration;

/// The result of asking the source for one chapter.
#[derive(Debug, Clone)]
pub enum FetchOutcome {
    /// The chapter page, decoded to UTF-8.
    Page(String),
    /// HTTP 404 — the chapter does not exist (a gap in the numbering,
    /// e.g. a reserved probate chapter). A soft skip, not a failure.
    NotFound,
}

/// Errors fetching a chapter page. A `Status`/`Request`/`Body` error is a
/// real failure (counts toward the run's failure threshold); a 404 is
/// not an error — it's [`FetchOutcome::NotFound`].
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// The reqwest client could not be built.
    #[error("building HTTP client: {0}")]
    Client(#[source] reqwest::Error),
    /// The request itself failed (DNS, connect, timeout).
    #[error("requesting {url}: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    /// A non-success, non-404 status.
    #[error("unexpected HTTP {status} for {url}")]
    Status { url: String, status: u16 },
    /// The body could not be read.
    #[error("reading body of {url}: {source}")]
    Body {
        url: String,
        #[source]
        source: reqwest::Error,
    },
}

/// Where chapter HTML comes from. Implemented by [`Fetcher`] over the
/// network and by a fixture stub in tests.
pub trait ChapterSource {
    /// Fetch one chapter page by URL.
    fn fetch(
        &self,
        url: &str,
    ) -> impl std::future::Future<Output = Result<FetchOutcome, FetchError>> + Send;

    /// Polite inter-chapter pause. Defaults to a no-op (tests run at full
    /// speed); [`Fetcher`] overrides it with a real sleep.
    fn pause(&self) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }
}

/// A real HTTP fetcher with a polite inter-request delay.
pub struct Fetcher {
    client: reqwest::Client,
    delay: Duration,
}

impl Fetcher {
    /// Build a fetcher that pauses `delay` between chapters.
    ///
    /// # Errors
    ///
    /// Returns [`FetchError::Client`] if the HTTP client cannot be built.
    pub fn new(delay: Duration) -> Result<Self, FetchError> {
        let client = reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(FetchError::Client)?;
        Ok(Self { client, delay })
    }
}

impl ChapterSource for Fetcher {
    async fn pause(&self) {
        tokio::time::sleep(self.delay).await;
    }

    async fn fetch(&self, url: &str) -> Result<FetchOutcome, FetchError> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|source| FetchError::Request {
                url: url.to_string(),
                source,
            })?;

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(FetchOutcome::NotFound);
        }
        if !status.is_success() {
            return Err(FetchError::Status {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }

        let charset = content_type_charset(
            resp.headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
        );
        let bytes = resp.bytes().await.map_err(|source| FetchError::Body {
            url: url.to_string(),
            source,
        })?;
        Ok(FetchOutcome::Page(decode(&bytes, charset.as_deref())))
    }
}

/// Pull the `charset=` parameter out of a `Content-Type` header value.
fn content_type_charset(content_type: Option<&str>) -> Option<String> {
    let ct = content_type?;
    ct.split(';')
        .filter_map(|part| part.trim().strip_prefix("charset="))
        .map(|c| c.trim().trim_matches('"').to_ascii_lowercase())
        .next()
}

/// Decode page bytes to UTF-8 using the named charset, defaulting to
/// windows-1252 (what the legislature serves). Unknown labels also fall
/// back to windows-1252.
#[must_use]
pub fn decode(bytes: &[u8], charset: Option<&str>) -> String {
    let encoding = charset
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::WINDOWS_1252);
    encoding.decode(bytes).0.into_owned()
}

#[cfg(test)]
mod tests {
    use super::{content_type_charset, decode};

    #[test]
    fn extracts_charset_from_content_type() {
        assert_eq!(
            content_type_charset(Some("text/html; charset=windows-1252")).as_deref(),
            Some("windows-1252")
        );
        assert_eq!(content_type_charset(Some("text/html")), None);
        assert_eq!(content_type_charset(None), None);
    }

    #[test]
    fn decodes_windows_1252_curly_punctuation() {
        // 0x93/0x94 are windows-1252 curly double quotes; 0x92 is the
        // right single quote the source uses for defined terms.
        let bytes = [0x93, b'C', b'l', b'a', b'i', b'm', 0x94, 0x92, b's'];
        assert_eq!(decode(&bytes, Some("windows-1252")), "“Claim”’s");
        // default (no charset) is windows-1252 too
        assert_eq!(decode(&bytes, None), "“Claim”’s");
    }
}
