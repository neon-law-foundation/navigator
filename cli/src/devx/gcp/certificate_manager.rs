//! Google-managed certificates validated by DNS, not by the load balancer.
//!
//! The classic `compute.sslCertificates` MANAGED type asks a CA to *contact the
//! load balancer* over port 80 to prove domain control. That has two
//! consequences this workspace cannot live with:
//!
//! 1. **The certificate cannot exist before the cutover.** Validation needs DNS
//!    already pointing at the load balancer, so moving a hostname that serves a
//!    live site means the old site goes dark while the new certificate issues.
//! 2. **Cloud CDN and the HTTP-to-HTTPS redirect sit in the validation path.**
//!    Google's own troubleshooting guidance names both as reasons validation
//!    fails, and recommends this module's approach instead.
//!
//! Certificate Manager's **DNS authorization** removes both. Google hands back
//! a `CNAME` to publish in the zone; the CA reads that record instead of
//! calling the load balancer. The record can be added while the hostname still
//! resolves to the old site, so the certificate reaches `ACTIVE` *before* the
//! `A` record moves and the cutover carries no TLS gap. Renewal stays automatic
//! for as long as the authorization record remains published.
//!
//! ## The resource chain
//!
//! ```text
//! DnsAuthorization ──> Certificate ──> CertificateMapEntry ──> CertificateMap
//!   (per hostname)      (per hostname)   (hostname -> cert)      (per site)
//!                                                                    │
//!                                            TargetHttpsProxy <──────┘
//! ```
//!
//! The proxy references the **map**, not a certificate, so a certificate can be
//! replaced underneath it without touching the load balancer.
//!
//! ## Idempotency
//!
//! Every create treats HTTP **409 Conflict** as success, the same contract the
//! rest of `gcp` uses. A re-run after a partial failure converges, and a re-run
//! while a certificate is still `PROVISIONING` is safe.

use serde_json::{json, Value};

use super::client::{GcpClient, GcpService};
use super::error::{SetupError, SetupResult};
use super::lro;

/// The `CNAME` an operator publishes to prove control of a hostname.
///
/// Google generates the name and value; neither is guessable, and the record
/// must stay published for renewal to keep working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsAuthorizationRecord {
    pub name: String,
    pub record_type: String,
    pub data: String,
}

/// POST a Certificate Manager create, waiting on the LRO and treating 409 as an
/// already-provisioned resource.
async fn create_or_conflict(
    client: &GcpClient,
    path: &str,
    body: &Value,
    operation: &str,
) -> SetupResult<()> {
    let response = client
        .post_json(GcpService::CertificateManager, path, body)
        .await?;
    match response.status_u16() {
        200..=299 => {
            let op: Value =
                serde_json::from_str(&response.into_text()).map_err(|source| SetupError::Json {
                    what: "certificate manager create response",
                    source,
                })?;
            lro::wait(client, GcpService::CertificateManager, &op, "/v1/{name}").await?;
            Ok(())
        }
        409 => Ok(()),
        other => Err(SetupError::BadStatus {
            operation: operation.to_string(),
            status: other,
            body: response.into_text(),
        }),
    }
}

/// Create the DNS authorization that proves control of `domain`.
pub async fn ensure_dns_authorization(
    client: &GcpClient,
    project_id: &str,
    id: &str,
    domain: &str,
) -> SetupResult<()> {
    let path = format!(
        "/v1/projects/{project_id}/locations/global/dnsAuthorizations?dnsAuthorizationId={id}"
    );
    let body = json!({
        "domain": domain,
        "description": "Static marketing site certificate authorization",
    });
    create_or_conflict(
        client,
        &path,
        &body,
        &format!("create DNS authorization {id} for {domain}"),
    )
    .await
}

/// Read back the `CNAME` an operator must publish. `None` in dry-run, and while
/// the authorization has not been created yet.
pub async fn dns_authorization_record(
    client: &GcpClient,
    project_id: &str,
    id: &str,
) -> SetupResult<Option<DnsAuthorizationRecord>> {
    let path = format!("/v1/projects/{project_id}/locations/global/dnsAuthorizations/{id}");
    let response = client.get(GcpService::CertificateManager, &path).await?;
    match response.status_u16() {
        200..=299 => {
            let body: Value =
                serde_json::from_str(&response.into_text()).map_err(|source| SetupError::Json {
                    what: "dns authorization lookup",
                    source,
                })?;
            let record = body.get("dnsResourceRecord");
            Ok(record.and_then(|record| {
                Some(DnsAuthorizationRecord {
                    name: record.get("name")?.as_str()?.to_string(),
                    record_type: record
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("CNAME")
                        .to_string(),
                    data: record.get("data")?.as_str()?.to_string(),
                })
            }))
        }
        404 => Ok(None),
        other => Err(SetupError::BadStatus {
            operation: format!("read DNS authorization {id}"),
            status: other,
            body: response.into_text(),
        }),
    }
}

/// Create the managed certificate for `domain`, validated through
/// `authorization_id` rather than by contacting the load balancer.
pub async fn ensure_certificate(
    client: &GcpClient,
    project_id: &str,
    id: &str,
    domain: &str,
    authorization_id: &str,
) -> SetupResult<()> {
    let path =
        format!("/v1/projects/{project_id}/locations/global/certificates?certificateId={id}");
    let body = json!({
        "managed": {
            "domains": [domain],
            "dnsAuthorizations": [format!(
                "projects/{project_id}/locations/global/dnsAuthorizations/{authorization_id}"
            )],
        },
        "description": "Static marketing site certificate",
    });
    create_or_conflict(
        client,
        &path,
        &body,
        &format!("create certificate {id} for {domain}"),
    )
    .await
}

/// Report a certificate's provisioning state — `ACTIVE`, `PROVISIONING`, or a
/// failure reason. `None` in dry-run and before the certificate exists.
pub async fn certificate_state(
    client: &GcpClient,
    project_id: &str,
    id: &str,
) -> SetupResult<Option<String>> {
    let path = format!("/v1/projects/{project_id}/locations/global/certificates/{id}");
    let response = client.get(GcpService::CertificateManager, &path).await?;
    match response.status_u16() {
        200..=299 => {
            let body: Value =
                serde_json::from_str(&response.into_text()).map_err(|source| SetupError::Json {
                    what: "certificate lookup",
                    source,
                })?;
            Ok(body
                .get("managed")
                .and_then(|managed| managed.get("state"))
                .and_then(Value::as_str)
                .map(str::to_string))
        }
        404 => Ok(None),
        other => Err(SetupError::BadStatus {
            operation: format!("read certificate {id}"),
            status: other,
            body: response.into_text(),
        }),
    }
}

/// Create the certificate map the target HTTPS proxy points at.
pub async fn ensure_certificate_map(
    client: &GcpClient,
    project_id: &str,
    id: &str,
) -> SetupResult<()> {
    let path =
        format!("/v1/projects/{project_id}/locations/global/certificateMaps?certificateMapId={id}");
    let body = json!({ "description": "Static marketing site certificate map" });
    create_or_conflict(
        client,
        &path,
        &body,
        &format!("create certificate map {id}"),
    )
    .await
}

/// Bind `hostname` to `certificate_id` inside the map.
pub async fn ensure_certificate_map_entry(
    client: &GcpClient,
    project_id: &str,
    map_id: &str,
    entry_id: &str,
    hostname: &str,
    certificate_id: &str,
) -> SetupResult<()> {
    let path = format!(
        "/v1/projects/{project_id}/locations/global/certificateMaps/{map_id}/\
         certificateMapEntries?certificateMapEntryId={entry_id}"
    );
    let body = json!({
        "hostname": hostname,
        "certificates": [format!(
            "projects/{project_id}/locations/global/certificates/{certificate_id}"
        )],
    });
    create_or_conflict(
        client,
        &path,
        &body,
        &format!("create certificate map entry {entry_id} for {hostname}"),
    )
    .await
}

/// The fully qualified map reference a target HTTPS proxy expects.
#[must_use]
pub fn map_reference(project_id: &str, map_id: &str) -> String {
    format!(
        "//certificatemanager.googleapis.com/projects/{project_id}/locations/global/\
         certificateMaps/{map_id}"
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::StaticToken;
    use super::*;

    fn client_for(server: &MockServer) -> GcpClient {
        GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::CertificateManager, server.uri())
    }

    /// The whole point of this module: the certificate names the DNS
    /// authorization, so the CA reads a record instead of calling the load
    /// balancer — which is what lets it issue before the cutover.
    #[tokio::test]
    async fn a_certificate_is_validated_through_its_dns_authorization() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/p/locations/global/certificates"))
            .and(query_param("certificateId", "foundation-cert"))
            .and(body_partial_json(json!({
                "managed": {
                    "domains": ["www.foundation.com"],
                    "dnsAuthorizations": [
                        "projects/p/locations/global/dnsAuthorizations/foundation-auth"
                    ],
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "done": true })))
            .expect(1)
            .mount(&server)
            .await;

        ensure_certificate(
            &client_for(&server),
            "p",
            "foundation-cert",
            "www.foundation.com",
            "foundation-auth",
        )
        .await
        .unwrap();
    }

    /// The operator cannot publish a record they were never shown, so the
    /// generated `CNAME` has to survive the round trip.
    #[tokio::test]
    async fn the_authorization_reports_the_cname_the_operator_must_publish() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/v1/projects/p/locations/global/dnsAuthorizations/foundation-auth",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "dnsResourceRecord": {
                    "name": "_acme-challenge.www.foundation.com.",
                    "type": "CNAME",
                    "data": "abc123.authorize.certificatemanager.goog.",
                }
            })))
            .mount(&server)
            .await;

        let record = dns_authorization_record(&client_for(&server), "p", "foundation-auth")
            .await
            .unwrap()
            .expect("the record is what makes the cutover gapless");
        assert_eq!(record.name, "_acme-challenge.www.foundation.com.");
        assert_eq!(record.record_type, "CNAME");
        assert_eq!(record.data, "abc123.authorize.certificatemanager.goog.");
    }

    #[tokio::test]
    async fn a_missing_authorization_reads_as_absent_rather_than_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let record = dns_authorization_record(&client_for(&server), "p", "nope")
            .await
            .unwrap();
        assert_eq!(record, None);
    }

    #[tokio::test]
    async fn certificate_state_surfaces_provisioning_and_active() {
        for state in ["PROVISIONING", "ACTIVE"] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "managed": { "state": state }
                })))
                .mount(&server)
                .await;

            let read = certificate_state(&client_for(&server), "p", "c")
                .await
                .unwrap();
            assert_eq!(read.as_deref(), Some(state));
        }
    }

    #[tokio::test]
    async fn an_existing_resource_is_success_not_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;

        ensure_certificate_map(&client_for(&server), "p", "foundation-map")
            .await
            .unwrap();
    }

    /// The proxy takes a `//certificatemanager.googleapis.com/...` reference,
    /// not a bare resource path. Getting this wrong is accepted by the API and
    /// silently serves no certificate.
    #[test]
    fn the_map_reference_is_fully_qualified() {
        let reference = map_reference("neon-law-marketing", "foundation-map");
        assert!(
            reference.starts_with("//certificatemanager.googleapis.com/projects/"),
            "{reference}",
        );
        assert!(
            reference.ends_with("/certificateMaps/foundation-map"),
            "{reference}"
        );
    }
}
