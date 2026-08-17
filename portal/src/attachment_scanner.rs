//! Malware-scanning boundary for inbound email attachments.
//!
//! Production uses [`ClamdAttachmentScanner`] over ClamAV's cluster-private
//! `INSTREAM` protocol. Tests inject [`FakeAttachmentScanner`] so ordinary
//! `cargo test` runs never depend on a sidecar.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// SendGrid accepts complete messages smaller than 30 MB. Keeping the scanner
/// stream at the same ceiling prevents a file from passing one boundary only
/// to be rejected by the next.
pub const DEFAULT_MAX_STREAM_BYTES: usize = 30_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanVerdict {
    Clean,
    Found { signature: String },
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ScanError {
    #[error("attachment exceeds the scanner stream limit of {limit} bytes")]
    SizeLimit { limit: usize },
    #[error("attachment scanner is unavailable: {0}")]
    Unavailable(String),
    #[error("attachment scanner timed out")]
    Timeout,
    #[error("attachment scanner returned a malformed reply: {0}")]
    MalformedReply(String),
}

#[async_trait]
pub trait AttachmentScanner: Send + Sync {
    async fn scan(&self, bytes: &[u8]) -> Result<ScanVerdict, ScanError>;
}

/// TCP `clamd` adapter. Its address must remain private: the clamd TCP
/// protocol has no authentication or transport encryption.
#[derive(Debug, Clone)]
pub struct ClamdAttachmentScanner {
    addr: String,
    timeout: Duration,
    max_stream_bytes: usize,
}

impl ClamdAttachmentScanner {
    #[must_use]
    pub fn new(addr: impl Into<String>) -> Self {
        Self {
            addr: addr.into(),
            timeout: Duration::from_secs(30),
            max_stream_bytes: DEFAULT_MAX_STREAM_BYTES,
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_max_stream_bytes(mut self, max_stream_bytes: usize) -> Self {
        self.max_stream_bytes = max_stream_bytes;
        self
    }

    /// Check protocol-level readiness. A TCP listener alone is not enough:
    /// `clamd` only accepts scans after its signature database is loaded.
    pub async fn ping(&self) -> Result<(), ScanError> {
        let result = timeout(self.timeout, async {
            let mut stream = TcpStream::connect(&self.addr)
                .await
                .map_err(|error| ScanError::Unavailable(error.to_string()))?;
            stream
                .write_all(b"zPING\0")
                .await
                .map_err(|error| ScanError::Unavailable(error.to_string()))?;
            let mut reply = [0_u8; 64];
            let read = stream
                .read(&mut reply)
                .await
                .map_err(|error| ScanError::Unavailable(error.to_string()))?;
            if reply[..read].windows(4).any(|part| part == b"PONG") {
                Ok(())
            } else {
                Err(ScanError::MalformedReply(
                    String::from_utf8_lossy(&reply[..read]).into_owned(),
                ))
            }
        })
        .await;
        result.map_err(|_| ScanError::Timeout)?
    }

    async fn scan_inner(&self, bytes: &[u8]) -> Result<ScanVerdict, ScanError> {
        if bytes.len() > self.max_stream_bytes {
            return Err(ScanError::SizeLimit {
                limit: self.max_stream_bytes,
            });
        }
        let mut stream = TcpStream::connect(&self.addr)
            .await
            .map_err(|error| ScanError::Unavailable(error.to_string()))?;
        stream
            .write_all(b"zINSTREAM\0")
            .await
            .map_err(|error| ScanError::Unavailable(error.to_string()))?;
        for chunk in bytes.chunks(64 * 1024) {
            let len = u32::try_from(chunk.len())
                .map_err(|_| ScanError::MalformedReply("stream chunk is too large".into()))?;
            stream
                .write_all(&len.to_be_bytes())
                .await
                .map_err(|error| ScanError::Unavailable(error.to_string()))?;
            stream
                .write_all(chunk)
                .await
                .map_err(|error| ScanError::Unavailable(error.to_string()))?;
        }
        stream
            .write_all(&0_u32.to_be_bytes())
            .await
            .map_err(|error| ScanError::Unavailable(error.to_string()))?;
        stream
            .flush()
            .await
            .map_err(|error| ScanError::Unavailable(error.to_string()))?;

        let mut reply = Vec::with_capacity(128);
        stream
            .take(4096)
            .read_to_end(&mut reply)
            .await
            .map_err(|error| ScanError::Unavailable(error.to_string()))?;
        parse_clamd_reply(&reply)
    }
}

#[async_trait]
impl AttachmentScanner for ClamdAttachmentScanner {
    async fn scan(&self, bytes: &[u8]) -> Result<ScanVerdict, ScanError> {
        timeout(self.timeout, self.scan_inner(bytes))
            .await
            .map_err(|_| ScanError::Timeout)?
    }
}

fn parse_clamd_reply(reply: &[u8]) -> Result<ScanVerdict, ScanError> {
    let reply = String::from_utf8_lossy(reply);
    let reply = reply.trim_matches(['\0', '\r', '\n', ' ']);
    let result = reply
        .strip_prefix("stream: ")
        .ok_or_else(|| ScanError::MalformedReply(reply.to_owned()))?;
    if result == "OK" {
        return Ok(ScanVerdict::Clean);
    }
    if let Some(signature) = result.strip_suffix(" FOUND") {
        if !signature.is_empty() {
            return Ok(ScanVerdict::Found {
                signature: signature.to_owned(),
            });
        }
    }
    Err(ScanError::MalformedReply(reply.to_owned()))
}

/// Deterministic scanner used at the Tier-1 trait boundary.
#[derive(Clone)]
pub struct FakeAttachmentScanner {
    result: Result<ScanVerdict, ScanError>,
    calls: Arc<AtomicUsize>,
}

impl FakeAttachmentScanner {
    #[must_use]
    pub fn new(result: Result<ScanVerdict, ScanError>) -> Self {
        Self {
            result,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[must_use]
    pub fn clean() -> Self {
        Self::new(Ok(ScanVerdict::Clean))
    }

    #[must_use]
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl AttachmentScanner for FakeAttachmentScanner {
    async fn scan(&self, _bytes: &[u8]) -> Result<ScanVerdict, ScanError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.result.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn parses_clean_found_and_malformed_replies() {
        assert_eq!(
            parse_clamd_reply(b"stream: OK\0").unwrap(),
            ScanVerdict::Clean
        );
        assert_eq!(
            parse_clamd_reply(b"stream: Eicar-Signature FOUND\0").unwrap(),
            ScanVerdict::Found {
                signature: "Eicar-Signature".into()
            }
        );
        assert!(matches!(
            parse_clamd_reply(b"not clamd"),
            Err(ScanError::MalformedReply(_))
        ));
        assert!(matches!(
            parse_clamd_reply(b"stream: INSTREAM size limit exceeded. ERROR\0"),
            Err(ScanError::MalformedReply(_))
        ));
    }

    #[tokio::test]
    async fn rejects_oversized_stream_before_connecting() {
        let scanner = ClamdAttachmentScanner::new("127.0.0.1:1").with_max_stream_bytes(3);
        assert_eq!(
            scanner.scan(b"four").await.unwrap_err(),
            ScanError::SizeLimit { limit: 3 }
        );
    }

    #[tokio::test]
    async fn fake_counts_each_scan_exactly_once() {
        let scanner = FakeAttachmentScanner::clean();
        scanner.scan(b"a").await.unwrap();
        scanner.scan(b"b").await.unwrap();
        assert_eq!(scanner.calls(), 2);
    }

    #[tokio::test]
    async fn clamd_adapter_streams_bytes_and_maps_found() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut command = [0_u8; 10];
            socket.read_exact(&mut command).await.unwrap();
            assert_eq!(&command, b"zINSTREAM\0");
            let len = socket.read_u32().await.unwrap();
            let mut bytes = vec![0; usize::try_from(len).unwrap()];
            socket.read_exact(&mut bytes).await.unwrap();
            assert_eq!(bytes, b"EICAR test fixture");
            assert_eq!(socket.read_u32().await.unwrap(), 0);
            socket
                .write_all(b"stream: Eicar-Signature FOUND\0")
                .await
                .unwrap();
        });
        let scanner = ClamdAttachmentScanner::new(addr.to_string());
        assert_eq!(
            scanner.scan(b"EICAR test fixture").await.unwrap(),
            ScanVerdict::Found {
                signature: "Eicar-Signature".into()
            }
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn clamd_adapter_bounds_an_unresponsive_scanner() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        let scanner =
            ClamdAttachmentScanner::new(addr.to_string()).with_timeout(Duration::from_millis(5));
        assert_eq!(
            scanner.scan(b"document").await.unwrap_err(),
            ScanError::Timeout
        );
        server.abort();
    }
}
