//! The global external HTTPS load balancer that fronts a public GCS bucket.
//!
//! A bucket alone cannot serve a custom domain over TLS. `storage.googleapis.com`
//! has its own certificate; `www.example.com` needs one issued for that name,
//! and GCS has no way to hold it. So a static site on a real hostname is
//! always a bucket *plus* a load balancer, and this module is that half.
//!
//! Four resources per hostname, in dependency order:
//!
//! 1. **Global address** — the anycast IPv4 the DNS `A` record points at.
//! 2. **Backend bucket** — teaches the load balancer to read from GCS, with
//!    Cloud CDN in front.
//! 3. **URL map** — sends every path to the backend bucket. There is no
//!    routing to express: the site is one origin.
//! 4. **Target HTTPS proxy** — binds a Certificate Manager map to the URL map.
//! 5. **Global forwarding rule** — binds the address and port 443 to the proxy.
//!
//! Plus a second, smaller chain on port 80 whose only job is a 301 to HTTPS.
//!
//! The certificate itself is **not** here. It lives in
//! [`super::certificate_manager`], which validates by DNS record rather than by
//! having a CA call this load balancer — so it can issue before the hostname is
//! cut over, and so neither Cloud CDN nor the port-80 redirect sits in the
//! validation path. The proxy references the map, so a renewal never touches
//! any resource in this module.
//!
//! ## Caching, and why nothing here invalidates
//!
//! CDN cache mode is `USE_ORIGIN_HEADERS`: the edge obeys whatever
//! `Cache-Control` the object carries. That makes the deploy step, not this
//! module, the thing that controls staleness — hashed assets ship
//! `immutable`, HTML ships `no-cache`. A deploy therefore never needs a cache
//! invalidation, which is why the deployer service account is not granted
//! `compute.urlMaps.invalidateCache`. Rewriting an object's bytes under an
//! unchanged name is the one case that would need one, and the hashed-asset
//! layout means that does not happen.
//!
//! ## Idempotency
//!
//! Every `insert` treats HTTP **409 Conflict** as success, the same contract
//! the rest of `gcp` uses. A re-run after a partial failure converges.

use serde_json::{json, Value};

use super::client::{GcpClient, GcpService};
use super::error::{SetupError, SetupResult};
use super::lro;

/// Outcome of an idempotent create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureOutcome {
    Created,
    AlreadyExists,
}

/// `EXTERNAL_MANAGED` is the global external Application Load Balancer, the
/// scheme Cloud CDN and backend buckets require. The older `EXTERNAL` scheme
/// is classic and is not what this provisions.
const LOAD_BALANCING_SCHEME: &str = "EXTERNAL_MANAGED";

/// A fully qualified Compute resource reference, which the API wants for every
/// cross-resource field (`urlMap`, `sslCertificates`, `target`, ...).
fn self_link(project_id: &str, collection: &str, name: &str) -> String {
    format!(
        "https://www.googleapis.com/compute/v1/projects/{project_id}/global/{collection}/{name}"
    )
}

/// POST a Compute global-collection insert, waiting on the LRO and treating
/// 409 as an already-provisioned resource.
async fn insert_global(
    client: &GcpClient,
    project_id: &str,
    collection: &str,
    body: &Value,
    operation: &str,
) -> SetupResult<EnsureOutcome> {
    let path = format!("/compute/v1/projects/{project_id}/global/{collection}");
    let response = client.post_json(GcpService::Compute, &path, body).await?;
    let status = response.status_u16();
    match status {
        200..=299 => {
            let op: Value =
                serde_json::from_str(&response.into_text()).map_err(|source| SetupError::Json {
                    what: "compute insert response",
                    source,
                })?;
            lro::wait(
                client,
                GcpService::Compute,
                &op,
                &format!("/compute/v1/projects/{project_id}/global/operations/{{name}}"),
            )
            .await?;
            Ok(EnsureOutcome::Created)
        }
        409 => Ok(EnsureOutcome::AlreadyExists),
        other => Err(SetupError::BadStatus {
            operation: operation.to_string(),
            status: other,
            body: response.into_text(),
        }),
    }
}

/// Reserve the global anycast IPv4 the hostname's `A` record will point at.
///
/// Reserved rather than ephemeral because the address outlives the forwarding
/// rule: an operator can rebuild the load balancer without the DNS record
/// going stale, and without a second certificate issuance wait.
pub async fn ensure_global_address(
    client: &GcpClient,
    project_id: &str,
    name: &str,
) -> SetupResult<EnsureOutcome> {
    let body = json!({
        "name": name,
        "ipVersion": "IPV4",
        "addressType": "EXTERNAL",
        "description": "Static marketing site frontend",
    });
    insert_global(
        client,
        project_id,
        "addresses",
        &body,
        &format!("reserve global address {name}"),
    )
    .await
}

/// Read a reserved global address back. Returns `None` in dry-run, and on a
/// 404 for an address that has not been created yet.
pub async fn global_address_ip(
    client: &GcpClient,
    project_id: &str,
    name: &str,
) -> SetupResult<Option<String>> {
    let path = format!("/compute/v1/projects/{project_id}/global/addresses/{name}");
    let response = client.get(GcpService::Compute, &path).await?;
    let status = response.status_u16();
    match status {
        200..=299 => {
            let body: Value =
                serde_json::from_str(&response.into_text()).map_err(|source| SetupError::Json {
                    what: "global address lookup",
                    source,
                })?;
            Ok(body
                .get("address")
                .and_then(Value::as_str)
                .map(str::to_string))
        }
        404 => Ok(None),
        other => Err(SetupError::BadStatus {
            operation: format!("read global address {name}"),
            status: other,
            body: response.into_text(),
        }),
    }
}

/// Point a CDN-enabled backend bucket at `bucket_name`.
///
/// `USE_ORIGIN_HEADERS` is deliberate — see the module docs. It makes the
/// object's own `Cache-Control` authoritative, so the deploy controls
/// staleness and no invalidation is ever required.
pub async fn ensure_backend_bucket(
    client: &GcpClient,
    project_id: &str,
    name: &str,
    bucket_name: &str,
) -> SetupResult<EnsureOutcome> {
    let body = json!({
        "name": name,
        "bucketName": bucket_name,
        "enableCdn": true,
        "cdnPolicy": { "cacheMode": "USE_ORIGIN_HEADERS" },
        "description": "Static marketing site origin",
    });
    insert_global(
        client,
        project_id,
        "backendBuckets",
        &body,
        &format!("create backend bucket {name}"),
    )
    .await
}

/// Send every path to the backend bucket. A static site is one origin, so
/// there is no host rule or path matcher to express.
pub async fn ensure_url_map(
    client: &GcpClient,
    project_id: &str,
    name: &str,
    backend_bucket_name: &str,
) -> SetupResult<EnsureOutcome> {
    let body = json!({
        "name": name,
        "defaultService": self_link(project_id, "backendBuckets", backend_bucket_name),
    });
    insert_global(
        client,
        project_id,
        "urlMaps",
        &body,
        &format!("create url map {name}"),
    )
    .await
}

/// A URL map that serves no content and only 301s to HTTPS.
///
/// `MOVED_PERMANENTLY_DEFAULT` is a 301, and `stripQuery: false` keeps the
/// query string across the redirect so campaign parameters survive.
pub async fn ensure_redirect_url_map(
    client: &GcpClient,
    project_id: &str,
    name: &str,
) -> SetupResult<EnsureOutcome> {
    let body = json!({
        "name": name,
        "defaultUrlRedirect": {
            "httpsRedirect": true,
            "redirectResponseCode": "MOVED_PERMANENTLY_DEFAULT",
            "stripQuery": false,
        },
    });
    insert_global(
        client,
        project_id,
        "urlMaps",
        &body,
        &format!("create redirect url map {name}"),
    )
    .await
}

/// Bind a Certificate Manager map to the URL map.
///
/// The proxy references the **map**, not a certificate, so a certificate can be
/// renewed or replaced underneath it without touching the load balancer. The
/// reference must be the fully qualified `//certificatemanager.googleapis.com/…`
/// form — a bare resource path is accepted and then serves no certificate.
pub async fn ensure_target_https_proxy(
    client: &GcpClient,
    project_id: &str,
    name: &str,
    url_map_name: &str,
    certificate_map: &str,
) -> SetupResult<EnsureOutcome> {
    let body = json!({
        "name": name,
        "urlMap": self_link(project_id, "urlMaps", url_map_name),
        "certificateMap": certificate_map,
    });
    insert_global(
        client,
        project_id,
        "targetHttpsProxies",
        &body,
        &format!("create target https proxy {name}"),
    )
    .await
}

/// Point an existing proxy at `certificate_map`.
///
/// `insert` is a no-op once the proxy exists, so a proxy created before the
/// move to Certificate Manager would keep its old `sslCertificates` forever
/// without this. Idempotent: setting the map it already has is accepted.
pub async fn set_proxy_certificate_map(
    client: &GcpClient,
    project_id: &str,
    name: &str,
    certificate_map: &str,
) -> SetupResult<()> {
    let path = format!(
        "/compute/v1/projects/{project_id}/global/targetHttpsProxies/{name}/setCertificateMap"
    );
    let body = json!({ "certificateMap": certificate_map });
    let response = client.post_json(GcpService::Compute, &path, &body).await?;
    match response.status_u16() {
        200..=299 => {
            let op: Value =
                serde_json::from_str(&response.into_text()).map_err(|source| SetupError::Json {
                    what: "set certificate map response",
                    source,
                })?;
            lro::wait(
                client,
                GcpService::Compute,
                &op,
                &format!("/compute/v1/projects/{project_id}/global/operations/{{name}}"),
            )
            .await?;
            Ok(())
        }
        other => Err(SetupError::BadStatus {
            operation: format!("set certificate map on proxy {name}"),
            status: other,
            body: response.into_text(),
        }),
    }
}

/// The port-80 proxy, which only ever reaches the redirect URL map.
pub async fn ensure_target_http_proxy(
    client: &GcpClient,
    project_id: &str,
    name: &str,
    url_map_name: &str,
) -> SetupResult<EnsureOutcome> {
    let body = json!({
        "name": name,
        "urlMap": self_link(project_id, "urlMaps", url_map_name),
    });
    insert_global(
        client,
        project_id,
        "targetHttpProxies",
        &body,
        &format!("create target http proxy {name}"),
    )
    .await
}

/// Bind the reserved address and a port to a proxy.
///
/// `port` is `"443"` for the HTTPS chain and `"80"` for the redirect chain;
/// `target_collection` selects which proxy kind the rule points at.
pub async fn ensure_global_forwarding_rule(
    client: &GcpClient,
    project_id: &str,
    name: &str,
    address_name: &str,
    target_collection: &str,
    target_name: &str,
    port: &str,
) -> SetupResult<EnsureOutcome> {
    let body = json!({
        "name": name,
        "IPAddress": self_link(project_id, "addresses", address_name),
        "IPProtocol": "TCP",
        "portRange": port,
        "target": self_link(project_id, target_collection, target_name),
        "loadBalancingScheme": LOAD_BALANCING_SCHEME,
    });
    insert_global(
        client,
        project_id,
        "forwardingRules",
        &body,
        &format!("create forwarding rule {name}"),
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::StaticToken;
    use super::*;

    fn client_for(server: &MockServer) -> GcpClient {
        GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Compute, server.uri())
    }

    async fn mount_insert(server: &MockServer, collection: &str, expected: Value) {
        Mock::given(method("POST"))
            .and(path(format!("/compute/v1/projects/p/global/{collection}")))
            .and(body_partial_json(expected))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "DONE" })))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn the_backend_bucket_defers_caching_to_the_object_headers() {
        let server = MockServer::start().await;
        mount_insert(
            &server,
            "backendBuckets",
            json!({
                "name": "foundation-backend",
                "bucketName": "neon-law-marketing-foundation",
                "enableCdn": true,
                "cdnPolicy": { "cacheMode": "USE_ORIGIN_HEADERS" },
            }),
        )
        .await;

        ensure_backend_bucket(
            &client_for(&server),
            "p",
            "foundation-backend",
            "neon-law-marketing-foundation",
        )
        .await
        .unwrap();
    }

    /// The proxy must reference the Certificate Manager map, not a classic
    /// `sslCertificates` resource. Sending `sslCertificates` here would be
    /// accepted and then never serve the DNS-validated certificate.
    #[tokio::test]
    async fn the_proxy_references_the_certificate_map() {
        let server = MockServer::start().await;
        let reference = "//certificatemanager.googleapis.com/projects/p/locations/global/certificateMaps/foundation-map";
        mount_insert(
            &server,
            "targetHttpsProxies",
            json!({
                "name": "foundation-https-proxy",
                "certificateMap": reference,
            }),
        )
        .await;

        ensure_target_https_proxy(
            &client_for(&server),
            "p",
            "foundation-https-proxy",
            "foundation-urlmap",
            reference,
        )
        .await
        .unwrap();
    }

    /// A 301 that drops the query string silently breaks every campaign link,
    /// so `stripQuery` is asserted rather than left to the API default.
    #[tokio::test]
    async fn the_http_redirect_is_a_301_that_keeps_the_query_string() {
        let server = MockServer::start().await;
        mount_insert(
            &server,
            "urlMaps",
            json!({
                "name": "foundation-redirect",
                "defaultUrlRedirect": {
                    "httpsRedirect": true,
                    "redirectResponseCode": "MOVED_PERMANENTLY_DEFAULT",
                    "stripQuery": false,
                },
            }),
        )
        .await;

        ensure_redirect_url_map(&client_for(&server), "p", "foundation-redirect")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn the_forwarding_rule_uses_the_application_load_balancer_scheme() {
        let server = MockServer::start().await;
        mount_insert(
            &server,
            "forwardingRules",
            json!({
                "name": "foundation-https",
                "portRange": "443",
                "loadBalancingScheme": "EXTERNAL_MANAGED",
                "IPProtocol": "TCP",
            }),
        )
        .await;

        ensure_global_forwarding_rule(
            &client_for(&server),
            "p",
            "foundation-https",
            "foundation-ip",
            "targetHttpsProxies",
            "foundation-https-proxy",
            "443",
        )
        .await
        .unwrap();
    }

    /// Re-running the command against a provisioned project must converge
    /// rather than fail, which is the whole idempotency contract.
    #[tokio::test]
    async fn an_existing_resource_is_success_not_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;

        let outcome = ensure_global_address(&client_for(&server), "p", "foundation-ip")
            .await
            .unwrap();
        assert_eq!(outcome, EnsureOutcome::AlreadyExists);
    }

    #[tokio::test]
    async fn a_real_failure_still_surfaces() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_string("quota"))
            .mount(&server)
            .await;

        let err = ensure_global_address(&client_for(&server), "p", "foundation-ip")
            .await
            .expect_err("403 is not an already-exists");
        assert!(err.to_string().contains("foundation-ip"), "{err}");
    }

    #[tokio::test]
    async fn a_missing_address_reads_as_absent_rather_than_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let ip = global_address_ip(&client_for(&server), "p", "foundation-ip")
            .await
            .unwrap();
        assert_eq!(ip, None);
    }

    #[tokio::test]
    async fn a_reserved_address_reports_the_ip_for_the_dns_record() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/compute/v1/projects/p/global/addresses/foundation-ip",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "address": "34.107.1.2" })),
            )
            .mount(&server)
            .await;

        let ip = global_address_ip(&client_for(&server), "p", "foundation-ip")
            .await
            .unwrap();
        assert_eq!(ip.as_deref(), Some("34.107.1.2"));
    }
}
