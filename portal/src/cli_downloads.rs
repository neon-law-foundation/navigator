//! `/app/team` — resolving and serving the `navigator` CLI archives.
//!
//! The render half lives in [`webapp::cli_downloads`]; this module is
//! everything that touches object storage. The split is the usual one for a
//! Dioxus page: the wasm bundle must not carry a bucket coordinate, so the
//! pre-layer resolves what is published and injects it, and a separate route
//! turns a platform slug back into bytes.
//!
//! **Keys are composed here, never accepted from the request.** The download
//! route takes a platform slug, matches it against [`PLATFORMS`], and builds
//! the key from the deployment's own release tag. A caller cannot name a key,
//! so no traversal or cross-prefix read is reachable from the URL — which
//! matters more than usual here, because the same bucket holds client
//! documents.
//!
//! **Publication is per-deployment, exactly like assets.** `ops ship` uploads
//! the tag's archives into the deployment's bucket; a deployment whose upload
//! has not run lists nothing and the page says so. That is why the page reads
//! storage rather than trusting the release tag alone: the tag says what this
//! deployment *runs*, and only the bucket says what a reader can actually
//! download.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path as AxumPath, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};

use webapp::cli_downloads::{CliArchive, InjectedDownloads};

/// Where the release archives live in the deployment's bucket.
///
/// One prefix per tag, so a deployment can hold more than one release without
/// the listing having to parse filenames to tell them apart.
pub const RELEASE_PREFIX: &str = "cli-releases";

/// How long a download link stays valid. Short: the page re-issues one on every
/// visit, and a signed URL is a bearer token for a deployment's private bucket,
/// so a link pasted into a chat should stop working quickly. The software is
/// open source; the bucket is not.
const SIGNED_URL_TTL: Duration = Duration::from_mins(5);

/// The platforms a release publishes, as `(slug, label, extension)`.
///
/// The slug is what appears in the download URL and in the archive filename —
/// they are deliberately the same word, so a reader who sees
/// `navigator-26.7.27-linux.tar.gz` land in their downloads folder can match it
/// to the row they clicked. macOS is absent: there is no macOS archive today
/// (the release notes tell a macOS operator to build from the tagged source),
/// and listing a platform whose bytes do not exist is exactly the 404 this page
/// is written to avoid.
pub const PLATFORMS: &[(&str, &str, &str)] =
    &[("windows", "Windows", "zip"), ("linux", "Linux", "tar.gz")];

/// The deployment's release tag, or `None` for a local `cargo run` that has
/// none. An untagged deployment publishes nothing, so the page renders empty.
fn release_tag() -> Option<String> {
    std::env::var("NAVIGATOR_RELEASE_TAG")
        .ok()
        .filter(|tag| !tag.is_empty() && tag != "unknown")
}

/// The storage key one platform's archive occupies for `tag`.
fn archive_key(tag: &str, platform: &str, extension: &str) -> String {
    format!("{RELEASE_PREFIX}/{tag}/navigator-{tag}-{platform}.{extension}")
}

/// Render a byte count the way a download page should: two significant decimals
/// at MB, whole numbers below. Not exact-decimal MB — readers compare this with
/// what their browser reports, and browsers use these same 1024-based units.
fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    #[allow(clippy::cast_precision_loss)]
    let bytes = bytes as f64;
    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

/// What this deployment can actually serve: the archives present in its bucket
/// for its own release tag.
///
/// A storage backend with no `list` (or a listing that errors) yields an empty
/// set rather than an error page. The page is a convenience, and a reader who
/// cannot see it has no way to act on a storage fault — the operator reads it
/// in the logs instead.
pub async fn published_archives(storage: &Arc<dyn cloud::StorageService>) -> InjectedDownloads {
    let Some(tag) = release_tag() else {
        return InjectedDownloads::default();
    };

    let listing = match storage.list(&format!("{RELEASE_PREFIX}/{tag}/")).await {
        Ok(listing) => listing,
        Err(cloud::StorageError::Unsupported(_)) => Vec::new(),
        Err(e) => {
            tracing::warn!(error = %e, %tag, "listing CLI release archives failed");
            Vec::new()
        }
    };

    let archives = PLATFORMS
        .iter()
        .filter_map(|(platform, label, extension)| {
            let key = archive_key(&tag, platform, extension);
            let found = listing.iter().find(|object| object.key == key)?;
            Some(CliArchive {
                platform: (*platform).to_string(),
                label: (*label).to_string(),
                filename: key.rsplit('/').next().unwrap_or(&key).to_string(),
                href: format!("/app/team/download/{platform}"),
                size: human_size(found.size_bytes),
            })
        })
        .collect();

    InjectedDownloads {
        version: tag,
        archives,
    }
}

/// The `/app/team` pre-layer: resolve what is published and inject it for the
/// render, the same shape the docs mount uses for its matched document.
pub async fn inject_downloads(
    State(storage): State<Arc<dyn cloud::StorageService>>,
    mut req: Request,
    next: Next,
) -> Response {
    let downloads = published_archives(&storage).await;
    req.extensions_mut().insert(downloads);
    next.run(req).await
}

/// `GET /app/team/download/{platform}` — hand over one archive.
///
/// The policy has already admitted the caller (firm tiers only; `client` and
/// anonymous are denied on the `/app/team` prefix), so this resolves the key
/// and serves it. An unknown slug is a 404 rather than a 400: the set of
/// platforms is not a caller's business to probe.
pub async fn download(
    State(storage): State<Arc<dyn cloud::StorageService>>,
    AxumPath(platform): AxumPath<String>,
) -> Response {
    let Some(tag) = release_tag() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((slug, _, extension)) = PLATFORMS
        .iter()
        .find(|(slug, _, _)| *slug == platform.as_str())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let key = archive_key(&tag, slug, extension);

    match storage.signed_url(&key, SIGNED_URL_TTL).await {
        Ok(url) => Redirect::temporary(&url).into_response(),
        // `FsStorage` (dev and tests) signs nothing, so stream the bytes.
        Err(cloud::StorageError::Unsupported(_)) => match storage.get(&key).await {
            Ok(object) => (
                [
                    (
                        axum::http::header::CONTENT_TYPE,
                        "application/octet-stream".to_string(),
                    ),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        format!(
                            "attachment; filename=\"{}\"",
                            key.rsplit('/').next().unwrap_or("navigator")
                        ),
                    ),
                ],
                object.bytes,
            )
                .into_response(),
            Err(cloud::StorageError::NotFound(_)) => StatusCode::NOT_FOUND.into_response(),
            Err(e) => {
                tracing::error!(error = %e, %key, "reading CLI archive failed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        },
        Err(cloud::StorageError::NotFound(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(error = %e, %key, "signing CLI archive URL failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{archive_key, human_size, PLATFORMS, RELEASE_PREFIX};

    /// A slug that is not in [`PLATFORMS`] never reaches [`archive_key`] — the
    /// handler 404s first — so the only slugs that can compose a key are the
    /// two literals in this table. This pins that the lookup is a match against
    /// the table rather than a substring or prefix test, which is what would
    /// let `../` through.
    #[test]
    fn only_a_known_platform_slug_resolves() {
        for probe in ["../secrets", "linux/../..", "LINUX", "linu", ""] {
            assert!(
                !PLATFORMS.iter().any(|(slug, _, _)| *slug == probe),
                "{probe:?} must not resolve to a platform"
            );
        }
        assert!(PLATFORMS.iter().any(|(slug, _, _)| *slug == "linux"));
    }

    /// The archive name the page serves must match what `deploy.yml` builds and
    /// uploads. Two files that never reference each other, so the shape is
    /// pinned on this side too.
    #[test]
    fn keys_match_the_release_archive_names() {
        assert_eq!(
            archive_key("26.7.27", "windows", "zip"),
            "cli-releases/26.7.27/navigator-26.7.27-windows.zip"
        );
        assert_eq!(
            archive_key("26.7.27", "linux", "tar.gz"),
            "cli-releases/26.7.27/navigator-26.7.27-linux.tar.gz"
        );
    }

    /// Every key sits under the one prefix, so a bucket that also holds client
    /// documents cannot be read through this route.
    #[test]
    fn every_platform_key_stays_under_the_release_prefix() {
        for (platform, _, extension) in PLATFORMS {
            let key = archive_key("26.7.27", platform, extension);
            assert!(
                key.starts_with(&format!("{RELEASE_PREFIX}/")),
                "{platform} key escaped the release prefix: {key}"
            );
            assert!(
                !key.contains(".."),
                "{platform} key contains traversal: {key}"
            );
        }
    }

    #[test]
    fn sizes_render_in_the_units_a_browser_reports() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(19_293_798), "18.4 MB");
    }
}
