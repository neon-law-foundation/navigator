//! Per-deployment publisher identity for Project client-portal bundles.
//!
//! A Project repository's CI publishes its built `portal/dist/` to this
//! deployment's private `<deployment>-applications` bucket, keyless, through
//! Workload Identity Federation. This module provisions the Google half of that
//! trust:
//!
//! 1. A `navigator-app-publisher` service account, one per deployment.
//! 2. `roles/storage.objectCreator` on the applications bucket and **nothing
//!    else** — create, never delete, and no `buckets.get`. A compromised publish
//!    can add an object; it cannot remove a live bundle, overwrite by deleting,
//!    or read another lane's documents.
//! 3. A GHE OIDC Workload Identity provider pinned to the applications
//!    organization on `main`, issued by the enterprise subdomain
//!    [`GITHUB_OIDC_ISSUER`](super::artifact_registry::GITHUB_OIDC_ISSUER) — not
//!    `githubusercontent.com`, which mints no token this tenant would trust.
//! 4. An impersonation binding pinned to the one `<org>/<repo>` allowed to mint
//!    the publisher's token, so a sibling app repository on the same enterprise
//!    cannot publish as it.
//!
//! The consumer half — the composite Action a Project repository runs — is
//! `.github/actions/application-publish`; the provider resource and the service
//! account email are published to each Project repository as GHE repository
//! *variables* (public identifiers; the trust lives in the binding here). See
//! `docs/project-repositories.md`.
//!
//! Everything is idempotent on the pipeline convention: creates POST
//! unconditionally and treat HTTP 409 as success, and the impersonation binding
//! is get-merge-set. The provider mirrors the marketing deploy identity in
//! `marketing.rs`; the impersonation reuses
//! [`ensure_wif_impersonation`](super::artifact_registry::ensure_wif_impersonation),
//! whose exclusive mode revokes a principal a repository rename left behind.

use serde_json::{json, Value};

use super::artifact_registry::{ensure_wif_impersonation, project_number, GITHUB_OIDC_ISSUER};
use super::client::{GcpClient, GcpService};
use super::error::{SetupError, SetupResult};
use super::lro;

/// Account id (local part of the SA email) of the per-deployment publisher.
pub const APP_PUBLISHER_ACCOUNT_ID: &str = "navigator-app-publisher";
/// Workload Identity pool the Project repositories federate through. Distinct
/// from the registry's `github` pool, which environment projects never create.
pub const APP_PUBLISHER_WIF_POOL_ID: &str = "app-publisher";
/// The provider id the consumer Action expects in the resource it is passed.
///
/// **The `ghe-oidc` spelling stays, and this is not stale narration.** It is a
/// live resource id, and a provider id is not patchable: renaming it here would
/// ask for a *second* provider under the same pool rather than renaming the
/// first, while `create_lro_or_conflict` read the 409 from the existing one as
/// success. The rename would report success and converge nothing — and
/// `.github/actions/application-publish/action.yml` names this id in the
/// resource it documents, so the two would disagree.
pub const APP_PUBLISHER_WIF_PROVIDER_ID: &str = "ghe-oidc";
/// The one role the publisher holds on the applications bucket: create, never
/// delete, and no `buckets.get`.
const PUBLISHER_ROLE: &str = "roles/storage.objectCreator";

/// The publisher service account email in `project_id`.
#[must_use]
pub fn service_account_email(project_id: &str) -> String {
    format!("{APP_PUBLISHER_ACCOUNT_ID}@{project_id}.iam.gserviceaccount.com")
}

/// The full provider resource the deployment publishes as
/// `NAVIGATOR_APP_PUBLISHER_WIF_PROVIDER`.
#[must_use]
pub fn wif_provider_resource(project_number: &str) -> String {
    format!(
        "projects/{project_number}/locations/global/workloadIdentityPools/\
         {APP_PUBLISHER_WIF_POOL_ID}/providers/{APP_PUBLISHER_WIF_PROVIDER_ID}"
    )
}

/// The attribute condition guarding token exchange: the applications org, on
/// `main` only. Pinned to `repository_owner` so every app repository in the org
/// can reach the impersonation gate, and to `refs/heads/main` so no other ref
/// mints a token.
#[must_use]
pub fn wif_attribute_condition(org: &str) -> String {
    format!("assertion.repository_owner == '{org}' && assertion.ref == 'refs/heads/main'")
}

/// The impersonation principal set, pinned to one `<org>/<repo>` so a sibling
/// app repository cannot mint the publisher's token even though the provider
/// trusts the whole organization.
#[must_use]
pub fn wif_principal_set(project_number: &str, org: &str, repo: &str) -> String {
    format!(
        "principalSet://iam.googleapis.com/projects/{project_number}/locations/global/\
         workloadIdentityPools/{APP_PUBLISHER_WIF_POOL_ID}/attribute.repository/{org}/{repo}"
    )
}

/// Provision the publisher identity that publishes to `applications_bucket`.
///
/// `org`/`repo` name the one Project repository allowed to publish. The
/// applications bucket must already exist — [`ensure_object_creator`] binds the
/// publisher on it — which is why `run` calls this after the buckets stage.
pub async fn ensure(
    client: &GcpClient,
    project_id: &str,
    org: &str,
    repo: &str,
    applications_bucket: &str,
) -> SetupResult<()> {
    let sa = service_account_email(project_id);
    ensure_publisher_account(client, project_id).await?;
    ensure_object_creator(client, applications_bucket, &sa).await?;
    ensure_wif_pool(client, project_id).await?;
    ensure_wif_provider(client, project_id, org).await?;
    let number = project_number(client, project_id).await?;
    ensure_wif_impersonation(
        client,
        project_id,
        &sa,
        &wif_principal_set(&number, org, repo),
    )
    .await?;

    // The two public identifiers the Project repository sets as GHE repository
    // variables. Printed so an operator can copy them straight into the
    // repository's Actions variables — the trust is enforced by the binding
    // above, so nothing here is a secret. See docs/project-repositories.md.
    eprintln!(
        "gcp setup [{project_id}] set GHE variable \
         NAVIGATOR_APP_PUBLISHER_WIF_PROVIDER={}",
        wif_provider_resource(&number)
    );
    eprintln!(
        "gcp setup [{project_id}] set GHE variable NAVIGATOR_APP_PUBLISHER_SERVICE_ACCOUNT={sa}"
    );
    Ok(())
}

/// Idempotently create the publisher service account. `serviceAccounts.create`
/// returns the finished account rather than a long-running operation, so — like
/// the marketing deployer — this must not be routed through an LRO wait.
async fn ensure_publisher_account(client: &GcpClient, project_id: &str) -> SetupResult<()> {
    let path = format!("/v1/projects/{project_id}/serviceAccounts");
    let body = json!({
        "accountId": APP_PUBLISHER_ACCOUNT_ID,
        "serviceAccount": {
            "displayName": "Navigator application publisher",
            "description": "Publishes Project portal bundles to the applications bucket",
        },
    });
    let resp = client.post_json(GcpService::Iam, &path, &body).await?;
    match resp.status_u16() {
        200..=299 | 409 => Ok(()),
        other => Err(SetupError::BadStatus {
            operation: format!("create service account {APP_PUBLISHER_ACCOUNT_ID}"),
            status: other,
            body: resp.into_text(),
        }),
    }
}

/// Grant the publisher [`PUBLISHER_ROLE`] on the applications bucket alone, via
/// the bucket's own IAM policy rather than a project role — so the identity can
/// create objects in the one bucket and touch nothing else in the project.
///
/// Get-merge-put against the live policy: it adds the binding only when absent,
/// preserving any binding added by hand and making no write on a converged
/// re-run.
async fn ensure_object_creator(
    client: &GcpClient,
    bucket: &str,
    publisher_email: &str,
) -> SetupResult<()> {
    let member = format!("serviceAccount:{publisher_email}");
    let path = format!("/storage/v1/b/{bucket}/iam");

    let response = client.get(GcpService::Storage, &path).await?;
    let status = response.status_u16();
    if !(200..=299).contains(&status) {
        return Err(SetupError::BadStatus {
            operation: format!("read IAM policy for bucket {bucket}"),
            status,
            body: response.into_text(),
        });
    }
    let mut policy: Value =
        serde_json::from_str(&response.into_text()).map_err(|source| SetupError::Json {
            what: "bucket IAM policy",
            source,
        })?;

    let mut bindings = policy
        .get("bindings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let already_bound = bindings.iter().any(|binding| {
        binding.get("role").and_then(Value::as_str) == Some(PUBLISHER_ROLE)
            && binding
                .get("members")
                .and_then(Value::as_array)
                .is_some_and(|members| members.iter().any(|m| m.as_str() == Some(&member)))
    });
    if already_bound {
        return Ok(());
    }
    bindings.push(json!({ "role": PUBLISHER_ROLE, "members": [member] }));
    policy["bindings"] = Value::Array(bindings);

    let resp = client.put_json(GcpService::Storage, &path, &policy).await?;
    match resp.status_u16() {
        200..=299 => Ok(()),
        other => Err(SetupError::BadStatus {
            operation: format!("grant publisher role on bucket {bucket}"),
            status: other,
            body: resp.into_text(),
        }),
    }
}

/// Idempotently create the app-publisher Workload Identity pool.
async fn ensure_wif_pool(client: &GcpClient, project_id: &str) -> SetupResult<()> {
    let path = format!(
        "/v1/projects/{project_id}/locations/global/workloadIdentityPools\
         ?workloadIdentityPoolId={APP_PUBLISHER_WIF_POOL_ID}"
    );
    let body = json!({ "displayName": "Navigator application publisher" });
    create_lro_or_conflict(client, &path, &body, "create app-publisher WIF pool").await
}

/// Idempotently create the GHE OIDC provider under the app-publisher pool,
/// pinned to the applications organization on `main`.
async fn ensure_wif_provider(client: &GcpClient, project_id: &str, org: &str) -> SetupResult<()> {
    let path = format!(
        "/v1/projects/{project_id}/locations/global/workloadIdentityPools/\
         {APP_PUBLISHER_WIF_POOL_ID}/providers\
         ?workloadIdentityPoolProviderId={APP_PUBLISHER_WIF_PROVIDER_ID}"
    );
    // The "GitHub Enterprise OIDC" display name is stale — these repositories
    // are on github.com — and it is deliberately left. `create_lro_or_conflict`
    // POSTs and reads a 409 as done; there is no PATCH path here, so a rename
    // would apply to providers created *after* it and to none of the ones that
    // exist. Correcting it means adding convergence first, which is a change to
    // live infrastructure rather than to a name (ENG-284 category 2).
    let body = json!({
        "displayName": "GitHub Enterprise OIDC",
        "oidc": { "issuerUri": GITHUB_OIDC_ISSUER },
        "attributeMapping": {
            "google.subject": "assertion.sub",
            "attribute.repository": "assertion.repository",
            "attribute.repository_owner": "assertion.repository_owner"
        },
        "attributeCondition": wif_attribute_condition(org)
    });
    create_lro_or_conflict(client, &path, &body, "create app-publisher WIF provider").await
}

/// POST an IAM create, waiting on any long-running operation and treating 409 as
/// already done.
async fn create_lro_or_conflict(
    client: &GcpClient,
    path: &str,
    body: &Value,
    operation: &'static str,
) -> SetupResult<()> {
    let resp = client.post_json(GcpService::Iam, path, body).await?;
    match resp.status_u16() {
        200..=299 => {
            let op: Value =
                serde_json::from_str(&resp.into_text()).map_err(|source| SetupError::Json {
                    what: "create operation",
                    source,
                })?;
            lro::wait(client, GcpService::Iam, &op, "/v1/{name}").await?;
            Ok(())
        }
        409 => Ok(()),
        other => Err(SetupError::BadStatus {
            operation: operation.to_string(),
            status: other,
            body: resp.into_text(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::{GcpService, StaticToken};
    use super::*;

    fn offline_dry_run_client() -> GcpClient {
        GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Iam, "http://127.0.0.1:1")
            .with_base_url(GcpService::Storage, "http://127.0.0.1:1")
            .with_base_url(GcpService::CloudResourceManager, "http://127.0.0.1:1")
            .with_dry_run()
    }

    #[test]
    fn service_account_email_is_project_scoped() {
        assert_eq!(
            service_account_email("neon-law-stg"),
            "navigator-app-publisher@neon-law-stg.iam.gserviceaccount.com"
        );
    }

    #[test]
    fn condition_pins_the_org_and_main_only() {
        let condition = wif_attribute_condition("neon-law");
        assert!(condition.contains("assertion.repository_owner == 'neon-law'"));
        assert!(condition.contains("assertion.ref == 'refs/heads/main'"));
    }

    #[test]
    fn principal_set_pins_one_repository() {
        let principal = wif_principal_set("123456789012", "neon-law", "acme");
        assert!(principal.starts_with("principalSet://iam.googleapis.com/projects/123456789012/"));
        assert!(principal
            .ends_with("workloadIdentityPools/app-publisher/attribute.repository/neon-law/acme"));
    }

    #[test]
    fn provider_resource_names_the_ghe_oidc_provider() {
        assert_eq!(
            wif_provider_resource("123456789012"),
            "projects/123456789012/locations/global/workloadIdentityPools/app-publisher/providers/ghe-oidc"
        );
    }

    #[tokio::test]
    async fn dry_run_records_the_full_publisher_provisioning() {
        let client = offline_dry_run_client();
        ensure(
            &client,
            "neon-law-stg",
            "neon-law",
            "acme",
            "neon-law-stg-applications",
        )
        .await
        .unwrap();
        let calls = client.recorded_calls();
        // SA create + bucket IAM get + bucket IAM put + WIF pool + WIF provider
        // + impersonation get + impersonation set = 7 (project number is
        // short-circuited in dry-run, so no CRM lookup).
        assert_eq!(calls.len(), 7, "unexpected dry-run calls: {calls:?}");
        let joined = calls
            .iter()
            .map(|c| format!("{} {}", c.url, c.body.as_deref().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("navigator-app-publisher"));
        assert!(joined.contains("workloadIdentityPools?workloadIdentityPoolId=app-publisher"));
        assert!(joined.contains("providers?workloadIdentityPoolProviderId=ghe-oidc"));
        assert!(joined.contains(GITHUB_OIDC_ISSUER));
        assert!(joined.contains("assertion.repository_owner == 'neon-law'"));
        // The publisher may create objects, never delete them or read metadata.
        assert!(joined.contains("roles/storage.objectCreator"));
        assert!(!joined.contains("roles/storage.objectAdmin"));
    }

    #[tokio::test]
    async fn object_creator_binding_is_added_when_absent_and_skipped_when_present() {
        // Absent: get returns an empty policy → put fires with objectCreator.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/storage/v1/b/proj-applications/iam"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/storage/v1/b/proj-applications/iam"))
            .and(body_partial_json(json!({
                "bindings": [{
                    "role": "roles/storage.objectCreator",
                    "members": ["serviceAccount:navigator-app-publisher@proj.iam.gserviceaccount.com"]
                }]
            })))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Storage, server.uri());
        ensure_object_creator(
            &client,
            "proj-applications",
            "navigator-app-publisher@proj.iam.gserviceaccount.com",
        )
        .await
        .unwrap();

        // Present: get already carries the binding, so no PUT mock is mounted —
        // a write would panic the test.
        let server2 = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/storage/v1/b/proj-applications/iam"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "bindings": [{
                    "role": "roles/storage.objectCreator",
                    "members": ["serviceAccount:navigator-app-publisher@proj.iam.gserviceaccount.com"]
                }]
            })))
            .mount(&server2)
            .await;
        let client2 = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Storage, server2.uri());
        ensure_object_creator(
            &client2,
            "proj-applications",
            "navigator-app-publisher@proj.iam.gserviceaccount.com",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn wif_provider_posts_tenant_issuer_and_owner_pinned_condition() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/v1/projects/p/locations/global/workloadIdentityPools/app-publisher/providers",
            ))
            .and(query_param("workloadIdentityPoolProviderId", "ghe-oidc"))
            .and(body_partial_json(json!({
                "oidc": { "issuerUri": "https://token.actions.githubusercontent.com" },
                "attributeCondition":
                    "assertion.repository_owner == 'neon-law' && assertion.ref == 'refs/heads/main'"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "done": true })))
            .expect(1)
            .mount(&server)
            .await;
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Iam, server.uri());
        ensure_wif_provider(&client, "p", "neon-law").await.unwrap();
    }
}
