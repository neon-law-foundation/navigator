#![allow(clippy::doc_markdown)] // DNS prose is dense with acronyms (DKIM, DMARC, SPF, CNAME, DNSimple, SendGrid).
//! DNS provider abstraction + DNSimple implementation.
//!
//! `devx dns setup` provisions the records SendGrid needs to send +
//! receive mail for our domain. The provider sits behind the
//! [`DnsProvider`] trait so future cutovers (Cloud DNS, Route53)
//! drop in without touching the orchestration layer.
//!
//! ## What we ensure
//!
//! Per-domain, idempotent (existing matching records → no-op,
//! drifted records → patched in place, missing → created):
//!
//! | Type | Name      | Content                                   | Why                            |
//! |------|-----------|-------------------------------------------|--------------------------------|
//! | MX   | (root)    | `mx.sendgrid.net` priority 10             | Inbound Parse delivery         |
//! | TXT  | (root)    | `v=spf1 include:sendgrid.net ~all`        | SPF (envelope-from check)      |
//! | TXT  | `_dmarc`  | `v=DMARC1; p=none; rua=mailto:…`          | DMARC reporting                |
//! | CNAME| `s1._domainkey` (when provided) | DKIM signer 1            | DKIM signature verification    |
//! | CNAME| `s2._domainkey` (when provided) | DKIM signer 2            | DKIM signature verification    |
//!
//! DKIM targets come from SendGrid's Domain Authentication wizard
//! (`/v3/whitelabel/domains`). The operator runs that once, copies
//! the two CNAME targets, and passes them via `--dkim-target1` /
//! `--dkim-target2` on the next `devx dns setup`. We do NOT call the
//! SendGrid whitelabel API from here — that's a separate
//! provisioning step kept outside the DNS provider seam.
//!
//! ## Auth
//!
//! DNSimple v2 uses an account-scoped Personal Access Token as
//! `Authorization: Bearer …`. Two env vars:
//!
//! - `DNSIMPLE_API_TOKEN` — the bearer token
//! - `DNSIMPLE_ACCOUNT_ID` — numeric account ID (lookup once via
//!   `GET /v2/accounts`)
//!
//! Tests stand up a `wiremock` server and point a constructor
//! override at it — no real HTTP, no real account.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// DNS record types we manipulate. We don't need the full enumeration —
/// just the four that drive mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    Mx,
    Txt,
    Cname,
}

impl RecordType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mx => "MX",
            Self::Txt => "TXT",
            Self::Cname => "CNAME",
        }
    }
}

/// A desired DNS record. `name` is the relative-to-zone label
/// (`""` for root, `"_dmarc"` for the DMARC TXT). `priority` is
/// honored only for MX.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredRecord {
    pub record_type: RecordType,
    pub name: String,
    pub content: String,
    pub priority: Option<u32>,
    pub ttl: u32,
}

/// What `ensure_record` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureOutcome {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug, Error)]
pub enum DnsError {
    #[error("http error: {0}")]
    Http(String),
    #[error("unexpected status {status} from {url}: {body}")]
    Status {
        status: u16,
        url: String,
        body: String,
    },
    #[error("missing env var: {0}")]
    MissingEnv(&'static str),
}

/// Pluggable DNS backend. The two methods cover the entire surface
/// `ensure_record` needs: list (for idempotency) and write.
#[async_trait]
pub trait DnsProvider: Send + Sync {
    /// List every record currently in the zone. The caller filters.
    async fn list_records(&self, zone: &str) -> Result<Vec<ExistingRecord>, DnsError>;

    /// Create a record. Returns the new record's provider-assigned id.
    async fn create_record(&self, zone: &str, record: &DesiredRecord) -> Result<u64, DnsError>;

    /// Update an existing record's content (and, for MX, priority).
    async fn update_record(
        &self,
        zone: &str,
        record_id: u64,
        record: &DesiredRecord,
    ) -> Result<(), DnsError>;
}

/// A record as returned by the provider. Carries enough to decide
/// "same as desired" or "drifted, needs update".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingRecord {
    pub id: u64,
    pub record_type: RecordType,
    pub name: String,
    pub content: String,
    pub priority: Option<u32>,
}

/// Idempotent upsert: find an existing record matching (type, name),
/// then compare content/priority and patch only if drifted.
pub async fn ensure_record(
    provider: &dyn DnsProvider,
    zone: &str,
    desired: &DesiredRecord,
) -> Result<EnsureOutcome, DnsError> {
    let records = provider.list_records(zone).await?;
    let existing = records
        .into_iter()
        .find(|r| r.record_type == desired.record_type && r.name == desired.name);
    match existing {
        None => {
            provider.create_record(zone, desired).await?;
            Ok(EnsureOutcome::Created)
        }
        Some(existing)
            if existing.content == desired.content && existing.priority == desired.priority =>
        {
            Ok(EnsureOutcome::Unchanged)
        }
        Some(existing) => {
            provider.update_record(zone, existing.id, desired).await?;
            Ok(EnsureOutcome::Updated)
        }
    }
}

/// Build the canonical set of mail records for `zone`.
///
/// `dkim_targets` is the pair of CNAME targets SendGrid hands back
/// from the Domain Authentication wizard. Empty → DKIM is skipped.
#[must_use]
pub fn mail_records(zone: &str, dkim_targets: &[String]) -> Vec<DesiredRecord> {
    let mut out = vec![
        DesiredRecord {
            record_type: RecordType::Mx,
            name: String::new(),
            content: "mx.sendgrid.net".into(),
            priority: Some(10),
            ttl: 3600,
        },
        DesiredRecord {
            record_type: RecordType::Txt,
            name: String::new(),
            content: "v=spf1 include:sendgrid.net ~all".into(),
            priority: None,
            ttl: 3600,
        },
        DesiredRecord {
            record_type: RecordType::Txt,
            name: "_dmarc".into(),
            content: format!("v=DMARC1; p=none; rua=mailto:postmaster@{zone}"),
            priority: None,
            ttl: 3600,
        },
    ];
    for (idx, target) in dkim_targets.iter().enumerate() {
        out.push(DesiredRecord {
            record_type: RecordType::Cname,
            name: format!("s{}._domainkey", idx + 1),
            content: target.clone(),
            priority: None,
            ttl: 3600,
        });
    }
    out
}

/// Outcome of [`run_mail_setup`]: one line per desired record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureReport {
    pub record_type: RecordType,
    pub name: String,
    pub outcome: EnsureOutcome,
}

/// Ensure every record from [`mail_records`] exists in the zone.
pub async fn run_mail_setup(
    provider: &dyn DnsProvider,
    zone: &str,
    dkim_targets: &[String],
) -> Result<Vec<EnsureReport>, DnsError> {
    let mut report = Vec::new();
    for record in mail_records(zone, dkim_targets) {
        let outcome = ensure_record(provider, zone, &record).await?;
        report.push(EnsureReport {
            record_type: record.record_type,
            name: record.name,
            outcome,
        });
    }
    Ok(report)
}

// --- DNSimple implementation --------------------------------------

/// DNSimple v2 base URL. Override in tests via
/// [`DnsimpleProvider::with_base_url`].
pub const DNSIMPLE_BASE_URL: &str = "https://api.dnsimple.com";

/// Production HTTP client for DNSimple v2.
#[derive(Clone)]
pub struct DnsimpleProvider {
    http: reqwest::Client,
    api_token: String,
    account_id: String,
    base_url: String,
    dry_run: bool,
    recorded: Arc<Mutex<Vec<RecordedCall>>>,
}

/// Recorded request (dry-run audit log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedCall {
    pub method: &'static str,
    pub url: String,
    pub body: Option<String>,
}

impl DnsimpleProvider {
    /// Production constructor. Bearer token + numeric account id.
    #[must_use]
    pub fn new(api_token: impl Into<String>, account_id: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_token: api_token.into(),
            account_id: account_id.into(),
            base_url: DNSIMPLE_BASE_URL.into(),
            dry_run: false,
            recorded: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Load from `DNSIMPLE_API_TOKEN` + `DNSIMPLE_ACCOUNT_ID`.
    pub fn from_env() -> Result<Self, DnsError> {
        let api_token = std::env::var("DNSIMPLE_API_TOKEN")
            .map_err(|_| DnsError::MissingEnv("DNSIMPLE_API_TOKEN"))?;
        let account_id = std::env::var("DNSIMPLE_ACCOUNT_ID")
            .map_err(|_| DnsError::MissingEnv("DNSIMPLE_ACCOUNT_ID"))?;
        Ok(Self::new(api_token, account_id))
    }

    /// Override the base URL — tests only.
    #[cfg(test)]
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Switch into dry-run mode. List remains live (we need the real
    /// state to decide create/update), but `create_record` /
    /// `update_record` are recorded instead of sent.
    #[must_use]
    pub fn with_dry_run(mut self) -> Self {
        self.dry_run = true;
        self
    }

    /// Snapshot of dry-run records.
    #[must_use]
    pub fn recorded_calls(&self) -> Vec<RecordedCall> {
        self.recorded
            .lock()
            .expect("recorded lock poisoned")
            .clone()
    }

    fn url(&self, path: &str) -> String {
        format!("{}/v2/{}{path}", self.base_url, self.account_id)
    }

    fn record(&self, method: &'static str, url: &str, body: Option<String>) {
        tracing::info!(
            target: "devx::dns::dry_run",
            method = method,
            url = url,
            body = body.as_deref().unwrap_or(""),
            "[dry-run] would call DNSimple",
        );
        self.recorded
            .lock()
            .expect("recorded lock poisoned")
            .push(RecordedCall {
                method,
                url: url.to_string(),
                body,
            });
    }
}

#[derive(Debug, Deserialize)]
struct DnsimpleListResponse {
    data: Vec<DnsimpleRecord>,
}

#[derive(Debug, Deserialize)]
struct DnsimpleSingleResponse {
    data: DnsimpleRecord,
}

#[derive(Debug, Deserialize)]
struct DnsimpleRecord {
    id: u64,
    #[serde(rename = "type")]
    record_type: String,
    name: String,
    content: String,
    #[serde(default)]
    priority: Option<u32>,
}

#[derive(Debug, Serialize)]
struct DnsimpleWriteBody<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    record_type: &'a str,
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<u32>,
    ttl: u32,
}

#[async_trait]
impl DnsProvider for DnsimpleProvider {
    async fn list_records(&self, zone: &str) -> Result<Vec<ExistingRecord>, DnsError> {
        let url = self.url(&format!("/zones/{zone}/records?per_page=100"));
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.api_token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| DnsError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let body_bytes = resp
            .bytes()
            .await
            .map_err(|e| DnsError::Http(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(DnsError::Status {
                status,
                url,
                body: String::from_utf8_lossy(&body_bytes).into_owned(),
            });
        }
        let parsed: DnsimpleListResponse = serde_json::from_slice(&body_bytes)
            .map_err(|e| DnsError::Http(format!("decode list response: {e}")))?;
        Ok(parsed
            .data
            .into_iter()
            .filter_map(|r| {
                let rt = match r.record_type.as_str() {
                    "MX" => RecordType::Mx,
                    "TXT" => RecordType::Txt,
                    "CNAME" => RecordType::Cname,
                    _ => return None,
                };
                Some(ExistingRecord {
                    id: r.id,
                    record_type: rt,
                    name: r.name,
                    content: r.content,
                    priority: r.priority,
                })
            })
            .collect())
    }

    async fn create_record(&self, zone: &str, record: &DesiredRecord) -> Result<u64, DnsError> {
        let url = self.url(&format!("/zones/{zone}/records"));
        let body = DnsimpleWriteBody {
            name: &record.name,
            record_type: record.record_type.as_str(),
            content: &record.content,
            priority: record.priority,
            ttl: record.ttl,
        };
        let body_json = serde_json::to_string(&body)
            .map_err(|e| DnsError::Http(format!("serialize body: {e}")))?;
        if self.dry_run {
            self.record("POST", &url, Some(body_json));
            return Ok(0);
        }
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_token)
            .header("Accept", "application/json")
            .body(body_json)
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| DnsError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let body_bytes = resp
            .bytes()
            .await
            .map_err(|e| DnsError::Http(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(DnsError::Status {
                status,
                url,
                body: String::from_utf8_lossy(&body_bytes).into_owned(),
            });
        }
        let parsed: DnsimpleSingleResponse = serde_json::from_slice(&body_bytes)
            .map_err(|e| DnsError::Http(format!("decode create response: {e}")))?;
        Ok(parsed.data.id)
    }

    async fn update_record(
        &self,
        zone: &str,
        record_id: u64,
        record: &DesiredRecord,
    ) -> Result<(), DnsError> {
        let url = self.url(&format!("/zones/{zone}/records/{record_id}"));
        let body = DnsimpleWriteBody {
            name: &record.name,
            record_type: record.record_type.as_str(),
            content: &record.content,
            priority: record.priority,
            ttl: record.ttl,
        };
        let body_json = serde_json::to_string(&body)
            .map_err(|e| DnsError::Http(format!("serialize body: {e}")))?;
        if self.dry_run {
            self.record("PATCH", &url, Some(body_json));
            return Ok(());
        }
        let resp = self
            .http
            .patch(&url)
            .bearer_auth(&self.api_token)
            .header("Accept", "application/json")
            .body(body_json)
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| DnsError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let body = resp.text().await.unwrap_or_default();
            return Err(DnsError::Status { status, url, body });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn record_type_as_str_maps_to_dns_names() {
        assert_eq!(RecordType::Mx.as_str(), "MX");
        assert_eq!(RecordType::Txt.as_str(), "TXT");
        assert_eq!(RecordType::Cname.as_str(), "CNAME");
    }

    #[test]
    fn mail_records_emits_mx_spf_dmarc_without_dkim_when_targets_empty() {
        let recs = mail_records("example.com", &[]);
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].record_type, RecordType::Mx);
        assert_eq!(recs[0].content, "mx.sendgrid.net");
        assert_eq!(recs[0].priority, Some(10));
        assert_eq!(recs[1].record_type, RecordType::Txt);
        assert!(recs[1].content.starts_with("v=spf1"));
        assert_eq!(recs[2].name, "_dmarc");
        assert!(recs[2].content.contains("postmaster@example.com"));
    }

    #[test]
    fn mail_records_adds_two_dkim_cnames_when_targets_provided() {
        let recs = mail_records(
            "example.com",
            &[
                "s1.target.sendgrid.net".into(),
                "s2.target.sendgrid.net".into(),
            ],
        );
        assert_eq!(recs.len(), 5);
        assert_eq!(recs[3].record_type, RecordType::Cname);
        assert_eq!(recs[3].name, "s1._domainkey");
        assert_eq!(recs[3].content, "s1.target.sendgrid.net");
        assert_eq!(recs[4].name, "s2._domainkey");
        assert_eq!(recs[4].content, "s2.target.sendgrid.net");
    }

    /// Programmable fake that records calls and returns canned data.
    #[derive(Default)]
    struct FakeProvider {
        existing: Vec<ExistingRecord>,
        creates: Arc<Mutex<Vec<DesiredRecord>>>,
        updates: Arc<Mutex<Vec<(u64, DesiredRecord)>>>,
    }

    #[async_trait]
    impl DnsProvider for FakeProvider {
        async fn list_records(&self, _zone: &str) -> Result<Vec<ExistingRecord>, DnsError> {
            Ok(self.existing.clone())
        }
        async fn create_record(
            &self,
            _zone: &str,
            record: &DesiredRecord,
        ) -> Result<u64, DnsError> {
            self.creates.lock().unwrap().push(record.clone());
            Ok(42)
        }
        async fn update_record(
            &self,
            _zone: &str,
            id: u64,
            record: &DesiredRecord,
        ) -> Result<(), DnsError> {
            self.updates.lock().unwrap().push((id, record.clone()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn ensure_record_creates_when_missing() {
        let fake = FakeProvider::default();
        let creates = fake.creates.clone();
        let desired = DesiredRecord {
            record_type: RecordType::Mx,
            name: String::new(),
            content: "mx.sendgrid.net".into(),
            priority: Some(10),
            ttl: 3600,
        };
        let outcome = ensure_record(&fake, "example.com", &desired).await.unwrap();
        assert_eq!(outcome, EnsureOutcome::Created);
        assert_eq!(creates.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn ensure_record_unchanged_when_existing_matches() {
        let fake = FakeProvider {
            existing: vec![ExistingRecord {
                id: 7,
                record_type: RecordType::Mx,
                name: String::new(),
                content: "mx.sendgrid.net".into(),
                priority: Some(10),
            }],
            ..Default::default()
        };
        let desired = DesiredRecord {
            record_type: RecordType::Mx,
            name: String::new(),
            content: "mx.sendgrid.net".into(),
            priority: Some(10),
            ttl: 3600,
        };
        let outcome = ensure_record(&fake, "example.com", &desired).await.unwrap();
        assert_eq!(outcome, EnsureOutcome::Unchanged);
    }

    #[tokio::test]
    async fn ensure_record_updates_when_content_drifted() {
        let fake = FakeProvider {
            existing: vec![ExistingRecord {
                id: 7,
                record_type: RecordType::Txt,
                name: String::new(),
                content: "v=spf1 -all".into(), // drifted
                priority: None,
            }],
            ..Default::default()
        };
        let updates = fake.updates.clone();
        let desired = DesiredRecord {
            record_type: RecordType::Txt,
            name: String::new(),
            content: "v=spf1 include:sendgrid.net ~all".into(),
            priority: None,
            ttl: 3600,
        };
        let outcome = ensure_record(&fake, "example.com", &desired).await.unwrap();
        assert_eq!(outcome, EnsureOutcome::Updated);
        let updates = updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 7);
    }

    #[tokio::test]
    async fn run_mail_setup_creates_all_three_when_zone_empty() {
        let fake = FakeProvider::default();
        let creates = fake.creates.clone();
        let report = run_mail_setup(&fake, "example.com", &[]).await.unwrap();
        assert_eq!(report.len(), 3);
        assert!(report.iter().all(|r| r.outcome == EnsureOutcome::Created));
        assert_eq!(creates.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn dnsimple_list_records_decodes_v2_response() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "data": [
                {"id": 1, "type": "MX", "name": "", "content": "mx.sendgrid.net", "priority": 10},
                {"id": 2, "type": "TXT", "name": "", "content": "v=spf1 include:sendgrid.net ~all"},
                {"id": 3, "type": "A", "name": "www", "content": "1.2.3.4"} // filtered out
            ]
        });
        Mock::given(method("GET"))
            .and(path("/v2/123/zones/example.com/records"))
            .and(header("authorization", "Bearer T"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;

        let provider = DnsimpleProvider::new("T", "123").with_base_url(server.uri());
        let records = provider.list_records("example.com").await.unwrap();
        // The A record is filtered out because we only manipulate MX/TXT/CNAME.
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].record_type, RecordType::Mx);
        assert_eq!(records[0].priority, Some(10));
        assert_eq!(records[1].record_type, RecordType::Txt);
    }

    #[tokio::test]
    async fn dnsimple_create_record_posts_v2_body_and_returns_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/123/zones/example.com/records"))
            .and(header("authorization", "Bearer T"))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"data": {"id": 99, "type": "MX", "name": "", "content": "mx.sendgrid.net", "priority": 10}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let provider = DnsimpleProvider::new("T", "123").with_base_url(server.uri());
        let desired = DesiredRecord {
            record_type: RecordType::Mx,
            name: String::new(),
            content: "mx.sendgrid.net".into(),
            priority: Some(10),
            ttl: 3600,
        };
        let id = provider
            .create_record("example.com", &desired)
            .await
            .unwrap();
        assert_eq!(id, 99);
    }

    #[tokio::test]
    async fn dnsimple_dry_run_skips_create_traffic_and_records_call() {
        // Point at unreachable to prove no real HTTP happens.
        let provider = DnsimpleProvider::new("T", "123")
            .with_base_url("http://127.0.0.1:1")
            .with_dry_run();
        let desired = DesiredRecord {
            record_type: RecordType::Txt,
            name: "_dmarc".into(),
            content: "v=DMARC1; p=none".into(),
            priority: None,
            ttl: 3600,
        };
        let id = provider
            .create_record("example.com", &desired)
            .await
            .unwrap();
        assert_eq!(id, 0); // dry-run synthetic id
        let calls = provider.recorded_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "POST");
        assert!(calls[0].url.contains("/v2/123/zones/example.com/records"));
        assert!(calls[0].body.as_deref().unwrap().contains("_dmarc"));
    }

    #[tokio::test]
    async fn dnsimple_list_returns_status_error_on_4xx() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
            .mount(&server)
            .await;
        let provider = DnsimpleProvider::new("T", "123").with_base_url(server.uri());
        let err = provider.list_records("example.com").await.unwrap_err();
        match err {
            DnsError::Status { status, body, .. } => {
                assert_eq!(status, 403);
                assert!(body.contains("Forbidden"));
            }
            _ => panic!("expected Status, got {err:?}"),
        }
    }
}
