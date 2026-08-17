//! `store::source_pages` — page renderings of a stored PDF asset, with
//! the relied-on passage pinned to a normalised rectangle (#893).
//!
//! [`crate::assets`] holds the bytes; [`pdf::passage`] does the geometry.
//! This module joins them: fetch an asset, locate a quote in it, lift the
//! pages it falls on, and persist each page rendering through
//! [`cloud::StorageService`] — never a local path, never into the
//! repository.
//!
//! # Provenance is the storage key
//!
//! A rendering is keyed by the **SHA-256 of the exact asset bytes it was
//! cut from**, which is the revision identity `assets` already carries:
//!
//! ```text
//! source-pages/<sha256>/p<page-index>.pdf
//! ```
//!
//! Superseding a document produces different bytes, therefore a
//! different digest, therefore a different key. There is no way for a
//! superseded document to keep serving a stale highlight, because the
//! stale rendering is not reachable from the new revision. The returned
//! [`SourcePageRender`] carries the digest so a caller records what it
//! pinned rather than assuming.
//!
//! Because the key is content-addressed, writing is idempotent: the same
//! asset and page re-render to the same key, so a second request reuses
//! the stored object rather than uploading it again.
//!
//! # What it refuses
//!
//! An asset that is not a PDF, and every failure [`pdf::passage`] refuses
//! to guess at — an unfound quote, an out-of-range occurrence, an
//! unmeasurable font. None of them produce a rendering.

use std::sync::Arc;

use cloud::StorageService;
use pdf::{NormalisedRect, PassageError};
use uuid::Uuid;

use crate::surreal::SurrealDb;

/// Errors from [`render_passage`].
#[derive(Debug, thiserror::Error)]
pub enum SourcePageError {
    #[error("storage: {0}")]
    Storage(#[from] cloud::StorageError),
    #[error("database: {0}")]
    Db(#[from] crate::assets::AssetError),
    #[error("no asset {0}")]
    NoAsset(Uuid),
    /// Only a PDF has pages to render and a text layer to locate in.
    #[error("asset {asset_id} is `{content_type}`, not a PDF")]
    NotAPdf {
        asset_id: Uuid,
        content_type: String,
    },
    #[error(transparent)]
    Passage(#[from] PassageError),
}

/// One page of a source document, rendered and stored, with the passage
/// marked on it.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcePageRegion {
    /// Zero-based page index within the source asset.
    pub page_index: usize,
    /// Where the page rendering was written. Stable for a given asset
    /// revision and page.
    pub storage_key: String,
    /// The passage's region on that page, as page fractions.
    pub rect: NormalisedRect,
}

/// A stored page rendering set for one occurrence of one quote.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcePageRender {
    /// The asset the rendering was cut from.
    pub asset_id: Uuid,
    /// The revision: the SHA-256 of the exact bytes rendered. A
    /// superseded document has a different digest and therefore
    /// different keys.
    pub asset_sha256_hex: String,
    /// Which occurrence of the quote was pinned, 1-based.
    pub ordinal: usize,
    /// How many times the quote appears in the asset.
    pub occurrences: usize,
    /// One region per line the passage covers; a passage broken across a
    /// page break carries regions with differing
    /// [`SourcePageRegion::page_index`] and therefore differing
    /// [`SourcePageRegion::storage_key`].
    pub regions: Vec<SourcePageRegion>,
}

impl SourcePageRender {
    /// The distinct page renderings this passage needs, in page order.
    #[must_use]
    pub fn storage_keys(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for region in &self.regions {
            if !out.contains(&region.storage_key) {
                out.push(region.storage_key.clone());
            }
        }
        out
    }
}

/// The key a page rendering of `sha256_hex`'s page `page_index` is
/// stored at. Content-addressed, so it pins the revision rather than the
/// asset row.
#[must_use]
pub fn storage_key(sha256_hex: &str, page_index: usize) -> String {
    format!("source-pages/{sha256_hex}/p{page_index}.pdf")
}

/// Locate the `ordinal`-th (1-based) occurrence of `quote` in the PDF
/// asset `asset_id`, render every page it falls on, store those
/// renderings, and return the normalised rects tied to the revision.
///
/// # Errors
/// [`SourcePageError::NoAsset`] for an unknown id,
/// [`SourcePageError::NotAPdf`] for an asset with no pages,
/// [`SourcePageError::Passage`] for anything [`pdf::locate`] refuses to
/// guess at, and [`SourcePageError::Storage`] /
/// [`SourcePageError::Db`] on an infrastructure failure.
pub async fn render_passage(
    db: &SurrealDb,
    storage: &Arc<dyn StorageService>,
    asset_id: Uuid,
    quote: &str,
    ordinal: usize,
) -> Result<SourcePageRender, SourcePageError> {
    let row = crate::assets::find_by_id(db, asset_id)
        .await?
        .ok_or(SourcePageError::NoAsset(asset_id))?;
    if row.content_type != "application/pdf" {
        return Err(SourcePageError::NotAPdf {
            asset_id,
            content_type: row.content_type,
        });
    }
    let bytes = storage.get(&row.storage_key).await?.bytes;

    // Locate first: a quote that cannot be pinned must not leave a
    // rendering behind in storage.
    let found = pdf::locate(&bytes, quote, ordinal)?;

    let mut regions = Vec::with_capacity(found.rects.len());
    for hit in &found.rects {
        let key = storage_key(&row.sha256_hex, hit.page_index);
        if !storage.exists(&key).await? {
            let page = pdf::page_render(&bytes, hit.page_index)?;
            storage.put(&key, &page, "application/pdf").await?;
        }
        regions.push(SourcePageRegion {
            page_index: hit.page_index,
            storage_key: key,
            rect: hit.rect,
        });
    }

    Ok(SourcePageRender {
        asset_id,
        asset_sha256_hex: row.sha256_hex,
        ordinal: found.ordinal,
        occurrences: found.occurrences,
        regions,
    })
}

#[cfg(test)]
mod tests {
    use super::{render_passage, storage_key, SourcePageError};
    use crate::assets::ingest_content;
    use crate::surreal::SurrealDb;
    use cloud::{FsStorage, StorageService};
    use std::sync::Arc;
    use uuid::Uuid;

    async fn fixtures() -> (SurrealDb, Arc<dyn StorageService>, tempfile::TempDir) {
        let db = crate::surreal::test_support::mem().await;
        let tmp = tempfile::tempdir().unwrap();
        let storage: Arc<dyn StorageService> =
            Arc::new(FsStorage::new(tmp.path().to_path_buf()).await.unwrap());
        (db, storage, tmp)
    }

    /// A synthetic two-page source document. No vendored bytes.
    fn source_pdf() -> Vec<u8> {
        pdf::render(
            "The witness testified to the meeting.\n\n#pagebreak()\n\nThe court struck the answer.",
        )
        .expect("render the synthetic source")
    }

    #[tokio::test]
    async fn a_located_passage_stores_its_page_rendering_under_the_revision_digest() {
        let (db, storage, _tmp) = fixtures().await;
        let bytes = source_pdf();
        let asset_id = ingest_content(&db, &storage, &bytes, "application/pdf")
            .await
            .unwrap();

        let found = render_passage(&db, &storage, asset_id, "testified to the meeting", 1)
            .await
            .unwrap();

        assert_eq!(found.ordinal, 1);
        assert_eq!(found.occurrences, 1);
        assert_eq!(found.regions.len(), 1);
        let region = &found.regions[0];
        assert_eq!(region.page_index, 0);
        assert_eq!(
            region.storage_key,
            storage_key(&found.asset_sha256_hex, 0),
            "the key is the revision digest, not the asset row id",
        );
        assert!(!found.asset_sha256_hex.is_empty());

        // The rendering is really there, is a PDF, and holds exactly the
        // page the rect was measured against.
        let stored = storage.get(&region.storage_key).await.unwrap().bytes;
        assert!(stored.starts_with(b"%PDF-"));
        assert_eq!(pdf::page_count(&stored).unwrap(), 1);
        assert!(pdf::locate(&stored, "testified to the meeting", 1).is_ok());
        assert!(region.rect.width > 0.0 && region.rect.height > 0.0);
    }

    #[tokio::test]
    async fn a_second_request_reuses_the_stored_rendering() {
        // Content-addressed keys make the write idempotent: the same
        // revision and page never churn a second object.
        let (db, storage, _tmp) = fixtures().await;
        let asset_id = ingest_content(&db, &storage, &source_pdf(), "application/pdf")
            .await
            .unwrap();

        let first = render_passage(&db, &storage, asset_id, "The court struck the answer", 1)
            .await
            .unwrap();

        // Stamp the stored object. If the second call re-rendered and
        // re-uploaded, the stamp is gone.
        let key = first.regions[0].storage_key.clone();
        storage
            .put(&key, b"already rendered", "application/pdf")
            .await
            .unwrap();

        let second = render_passage(&db, &storage, asset_id, "The court struck the answer", 1)
            .await
            .unwrap();
        assert_eq!(first, second, "the same revision renders to the same keys");
        assert_eq!(
            storage.get(&key).await.unwrap().bytes,
            b"already rendered",
            "an existing rendering for this revision was overwritten instead of reused",
        );
    }

    #[tokio::test]
    async fn a_passage_across_a_page_break_stores_both_pages() {
        let (db, storage, _tmp) = fixtures().await;
        let asset_id = ingest_content(&db, &storage, &source_pdf(), "application/pdf")
            .await
            .unwrap();

        let found = render_passage(
            &db,
            &storage,
            asset_id,
            "testified to the meeting. The court struck",
            1,
        )
        .await
        .unwrap();

        assert_eq!(found.regions.len(), 2, "one region per page, never merged");
        assert_eq!(found.regions[0].page_index, 0);
        assert_eq!(found.regions[1].page_index, 1);
        assert_eq!(found.storage_keys().len(), 2);
        for region in &found.regions {
            let stored = storage.get(&region.storage_key).await.unwrap().bytes;
            assert_eq!(pdf::page_count(&stored).unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn an_unfound_quote_leaves_nothing_in_storage() {
        // Fail-closed: a quote that cannot be pinned must not leave a
        // rendering behind that a caller could mistake for evidence.
        let (db, storage, tmp) = fixtures().await;
        let asset_id = ingest_content(&db, &storage, &source_pdf(), "application/pdf")
            .await
            .unwrap();

        let err = render_passage(&db, &storage, asset_id, "the witness recanted", 1)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                SourcePageError::Passage(pdf::PassageError::QuoteNotFound { .. })
            ),
            "expected QuoteNotFound, got {err:?}",
        );
        assert!(
            !tmp.path().join("source-pages").exists(),
            "a refused passage wrote a rendering anyway",
        );
    }

    #[tokio::test]
    async fn a_non_pdf_asset_is_refused() {
        let (db, storage, _tmp) = fixtures().await;
        let asset_id = ingest_content(&db, &storage, b"# not a pdf", "text/markdown")
            .await
            .unwrap();
        let err = render_passage(&db, &storage, asset_id, "not a pdf", 1)
            .await
            .unwrap_err();
        assert!(
            matches!(err, SourcePageError::NotAPdf { .. }),
            "expected NotAPdf, got {err:?}",
        );
    }

    #[tokio::test]
    async fn an_unknown_asset_is_refused() {
        let (db, storage, _tmp) = fixtures().await;
        let missing = Uuid::now_v7();
        assert!(matches!(
            render_passage(&db, &storage, missing, "anything", 1).await,
            Err(SourcePageError::NoAsset(_)),
        ));
    }
}
