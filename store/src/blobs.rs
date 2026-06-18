//! Content-addressed blob ingest, independent of the `documents` lane.
//!
//! [`crate::documents::ingest_bytes`] couples a blob to a matter-scoped
//! `documents` row. Template bodies (and any other non-document
//! artifact) want only the blob half: sha-dedup the bytes, write them
//! to object storage at `blobs/<sha>`, and insert/reuse a `blobs` row.
//! This module is that lower-level seam.

use std::sync::Arc;

use cloud::StorageService;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entity::blob;
use crate::Db;

/// Errors from [`ingest`].
#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("storage: {0}")]
    Storage(#[from] cloud::StorageError),
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Ingest `bytes` as a content-addressed blob: dedup by SHA-256, write
/// to storage at `blobs/<sha>` when new, insert/reuse the `blobs` row,
/// and return its id. Idempotent — re-ingesting identical bytes reuses
/// the existing blob.
pub async fn ingest(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    bytes: &[u8],
    content_type: &str,
) -> Result<Uuid, BlobError> {
    let sha_hex = sha256_hex(bytes);
    if let Some(existing) = blob::Entity::find()
        .filter(blob::Column::Sha256Hex.eq(sha_hex.clone()))
        .one(db)
        .await?
    {
        return Ok(existing.id);
    }
    let storage_key = format!("blobs/{sha_hex}");
    storage.put(&storage_key, bytes, content_type).await?;
    let byte_size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
    let row = blob::ActiveModel {
        storage_key: ActiveValue::Set(storage_key),
        content_type: ActiveValue::Set(content_type.to_string()),
        byte_size: ActiveValue::Set(byte_size),
        sha256_hex: ActiveValue::Set(sha_hex),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

/// Fetch a blob's bytes from storage by blob id.
pub async fn fetch(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    blob_id: Uuid,
) -> Result<Vec<u8>, BlobError> {
    let row = blob::Entity::find_by_id(blob_id)
        .one(db)
        .await?
        .ok_or_else(|| BlobError::Storage(cloud::StorageError::NotFound(blob_id.to_string())))?;
    Ok(storage.get(&row.storage_key).await?.bytes)
}
