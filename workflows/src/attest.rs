//! `onchain__record_attestation` step dispatch — record an on-chain
//! attorney attestation (the Neon Law Node product).
//!
//! Mirrors [`crate::compliance`]: the caller threads an [`OnChainPayload`]
//! through the signal `value`, and the worker (the `workflows-service`
//! `NotationService` in prod, the in-process [`crate::DispatchingRuntime`]
//! in dev/tests) records the attestation when a transition lands on
//! `onchain__record_attestation`.
//!
//! ## The chain is isolated behind a trait
//!
//! Solana lives *only* behind the [`Attestor`] trait, the same way GCS
//! lives only behind `cloud::StorageService`. The generic workflow layer
//! knows the provider-neutral `onchain__` prefix; selecting Solana (or a
//! second chain later) is a new `impl Attestor`, never a workflow edit.
//! [`NullAttestor`] is the KIND / no-chain default — it records *no*
//! transaction, so the local row stays `pending` and the workflow can
//! never claim an on-chain record that does not exist.
//!
//! ## The local row is the system of record
//!
//! Whatever the attestor returns, the side effect always writes one
//! `attestations` row ([`store::attestations`]) inside the caller's
//! `ctx.run`. The row carries the SHA-256 of the attested document plus
//! identifiers (wallets, tx signature) — never client content, the same
//! trust boundary telemetry observes. A real [`RecordedTx`] flips the row
//! to `recorded`; its absence leaves it `pending`.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Env var selecting the on-chain backend. `null` / unset → no chain
/// (the [`NullAttestor`]); `solana` is reserved for the not-yet-shipped
/// `SolanaAttestor`.
pub const ONCHAIN_BACKEND_ENV: &str = "NAVIGATOR_ONCHAIN_BACKEND";

/// What to attest, threaded as the JSON `value` of the signal that lands
/// on `onchain__record_attestation`. The `storage_key` points at the
/// already-persisted attestation document (e.g. the signed retainer PDF);
/// the dispatch hashes those bytes — the hash, not the document, is what
/// goes on-chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnChainPayload {
    /// `cloud::StorageService` key of the document to hash and attest.
    pub storage_key: String,
    /// Firm wallet public key to bind in the attestation, when known.
    #[serde(default)]
    pub firm_wallet: Option<String>,
    /// Client wallet public key to bind in the attestation, when known.
    #[serde(default)]
    pub client_wallet: Option<String>,
}

/// One attestation to record on-chain: identifiers and a hash, never
/// content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationRequest {
    pub notation_id: Uuid,
    /// Lowercase hex SHA-256 of the attested document bytes.
    pub sha256: String,
    pub firm_wallet: Option<String>,
    pub client_wallet: Option<String>,
}

/// A confirmed on-chain record — what a real [`Attestor`] returns once a
/// transaction lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedTx {
    /// Chain transaction signature.
    pub signature: String,
    /// Program Derived Address holding the attestation account.
    pub pda: String,
    /// RFC 3339 timestamp the transaction confirmed.
    pub recorded_at: String,
}

/// Errors from recording an attestation.
#[derive(Debug, thiserror::Error)]
pub enum AttestError {
    #[error("read attestation document `{key}`: {source}")]
    Storage {
        key: String,
        #[source]
        source: cloud::StorageError,
    },
    #[error("on-chain record: {0}")]
    Chain(String),
    #[error("unknown on-chain backend `{0}` (expected `null` or `solana`)")]
    UnknownBackend(String),
    #[error("the `solana` backend is not shipped yet — set {ONCHAIN_BACKEND_ENV}=null")]
    SolanaNotImplemented,
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
}

/// The chain seam. One method: record an attestation and return the
/// transaction, or `None` when the backend records nothing (the
/// [`NullAttestor`]). Implemented by `SolanaAttestor` in prod (not yet
/// shipped) and [`NullAttestor`] in KIND / tests.
#[async_trait]
pub trait Attestor: Send + Sync {
    /// Provider label persisted on the row (`solana`, `null`).
    fn chain(&self) -> &'static str;

    /// Record the attestation on-chain. `Ok(None)` means the backend
    /// recorded nothing (no chain configured); `Ok(Some(tx))` is a
    /// confirmed transaction.
    async fn record(&self, req: &AttestationRequest) -> Result<Option<RecordedTx>, AttestError>;
}

/// The no-chain default: records nothing, so the local `attestations`
/// row stays `pending`. Used in KIND and tests, and any deploy that has
/// not configured a real chain backend — the honest "not shipped yet"
/// behavior the Node product page promises.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullAttestor;

#[async_trait]
impl Attestor for NullAttestor {
    fn chain(&self) -> &'static str {
        "null"
    }

    async fn record(&self, _req: &AttestationRequest) -> Result<Option<RecordedTx>, AttestError> {
        Ok(None)
    }
}

/// Select the on-chain backend from the environment.
///
/// `null` / unset → [`NullAttestor`]. `solana` is reserved for the
/// not-yet-shipped `SolanaAttestor` and errors loudly so a production
/// deploy can never *silently* no-op an attestation it believes is going
/// on-chain. Any other value is an [`AttestError::UnknownBackend`].
///
/// # Errors
///
/// Returns an error for the `solana` backend (not implemented) or an
/// unrecognized backend value.
pub fn attestor_from_env() -> Result<Arc<dyn Attestor>, AttestError> {
    let backend = std::env::var(ONCHAIN_BACKEND_ENV).unwrap_or_default();
    match backend.trim().to_ascii_lowercase().as_str() {
        "" | "null" | "none" => Ok(Arc::new(NullAttestor)),
        "solana" => Err(AttestError::SolanaNotImplemented),
        other => Err(AttestError::UnknownBackend(other.to_string())),
    }
}

/// Lowercase hex SHA-256 of `bytes` — the on-chain hash of the attested
/// document.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Record an attestation: hash the document at `payload.storage_key`,
/// call the [`Attestor`], and write the durable `attestations` row.
///
/// The single side effect of the `onchain__record_attestation` step;
/// callers wrap it in `ctx.run` (worker) or call it inline
/// (`DispatchingRuntime`). The row is written **whatever the chain
/// returns** — `pending` with no tx from [`NullAttestor`], `recorded`
/// with a tx from a real backend — so the local database is always the
/// system of record.
///
/// # Errors
///
/// Fails if the document can't be read, the chain write errors, or the
/// database write fails.
pub async fn dispatch_onchain_record(
    storage: &dyn cloud::StorageService,
    attestor: &dyn Attestor,
    db: &store::Db,
    notation_id: Uuid,
    payload: &OnChainPayload,
) -> Result<(), AttestError> {
    let object =
        storage
            .get(&payload.storage_key)
            .await
            .map_err(|source| AttestError::Storage {
                key: payload.storage_key.clone(),
                source,
            })?;
    let sha256 = sha256_hex(&object.bytes);

    let req = AttestationRequest {
        notation_id,
        sha256: sha256.clone(),
        firm_wallet: payload.firm_wallet.clone(),
        client_wallet: payload.client_wallet.clone(),
    };
    let tx = attestor.record(&req).await?;

    let status = if tx.is_some() {
        store::attestations::STATUS_RECORDED
    } else {
        store::attestations::STATUS_PENDING
    };
    store::attestations::record(
        db,
        &store::attestations::NewAttestation {
            notation_id,
            chain: attestor.chain(),
            sha256: &sha256,
            status,
            pda: tx.as_ref().map(|t| t.pda.as_str()),
            tx_signature: tx.as_ref().map(|t| t.signature.as_str()),
            firm_wallet: payload.firm_wallet.as_deref(),
            client_wallet: payload.client_wallet.as_deref(),
            recorded_at: tx.as_ref().map(|t| t.recorded_at.as_str()),
        },
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        attestor_from_env, sha256_hex, AttestError, AttestationRequest, Attestor, NullAttestor,
        ONCHAIN_BACKEND_ENV,
    };
    use uuid::Uuid;

    #[test]
    fn sha256_hex_is_lowercase_hex_of_the_bytes() {
        // sha256("hello world")
        assert_eq!(
            sha256_hex(b"hello world"),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[tokio::test]
    async fn null_attestor_records_no_transaction() {
        let req = AttestationRequest {
            notation_id: Uuid::from_u128(1),
            sha256: "abc".into(),
            firm_wallet: None,
            client_wallet: None,
        };
        let tx = NullAttestor.record(&req).await.unwrap();
        assert!(tx.is_none(), "the null backend records nothing");
        assert_eq!(NullAttestor.chain(), "null");
    }

    #[test]
    fn attestor_from_env_defaults_to_null_and_rejects_solana() {
        // This test owns the process env var; set/clear it explicitly.
        std::env::remove_var(ONCHAIN_BACKEND_ENV);
        assert_eq!(attestor_from_env().unwrap().chain(), "null");

        std::env::set_var(ONCHAIN_BACKEND_ENV, "null");
        assert_eq!(attestor_from_env().unwrap().chain(), "null");

        // Solana is reserved but not shipped — it must error, never
        // silently no-op an attestation a deployer believes is on-chain.
        // (Match on the whole Result: `Arc<dyn Attestor>` isn't Debug, so
        // `unwrap_err` is unavailable here.)
        std::env::set_var(ONCHAIN_BACKEND_ENV, "solana");
        assert!(matches!(
            attestor_from_env(),
            Err(AttestError::SolanaNotImplemented)
        ));

        std::env::set_var(ONCHAIN_BACKEND_ENV, "dogecoin");
        assert!(matches!(
            attestor_from_env(),
            Err(AttestError::UnknownBackend(_))
        ));
        std::env::remove_var(ONCHAIN_BACKEND_ENV);
    }
}
