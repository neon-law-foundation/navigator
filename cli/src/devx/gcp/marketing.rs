//! Provision the static marketing sites — and nothing else.
//!
//! `neon-law-marketing` holds the published brand sites. Each one is a React build
//! that ships as files: no server, no database, no cluster, no request-time
//! rendering. So this command provisions exactly two things per site, a public
//! bucket and the load balancer that puts a certificate in front of it, plus
//! one identity per site so its repository can deploy without a stored key.
//!
//! That is a genuinely different shape from [`super::run`], which builds an
//! environment (private document storage, GKE), and from
//! [`super::hub`], which builds a registry. Pointing either of those at
//! `neon-law-marketing` would put client documents in a project whose buckets are
//! anonymously readable, so [`super::tenants`] refuses the mismatch before the
//! first GCP call.
//!
//! ## The GHE issuer
//!
//! The deploy identity is federated, not a downloaded key. The subtlety is the
//! issuer: these repositories live on GitHub Enterprise Cloud with data
//! residency, which mints Actions OIDC tokens from the enterprise's own
//! subdomain — [`GHE_OIDC_ISSUER`] — and *not* from
//! `token.actions.githubusercontent.com`. A provider configured with the wrong
//! issuer rejects every token with a signature failure.
//!
//! One enterprise issues from one host, so this is the same issuer
//! [`super::artifact_registry`] trusts for `neon-law-foundation/navigator`, and
//! [`GHE_OIDC_ISSUER`] aliases that constant rather than repeating the literal.
//! What still differs is the *provider*: each lives in its own project and
//! narrows to its own repositories.
//!
//! Because the issuer already lives on a per-enterprise subdomain, it is
//! inherently scoped to this enterprise: no other tenant can mint a token
//! against it. The provider narrows it further to the `marketing` owner, and
//! the impersonation binding narrows it again to one specific repository, so a
//! token from a different repository in the same org cannot deploy this site.

use super::artifact_registry::{ensure_wif_impersonation, ensure_wif_pool, WIF_POOL_ID};
use super::client::{GcpClient, GcpService};
use super::error::SetupResult;
use super::tenants::{self, TenantRole};
use super::{buckets, certificate_manager, load_balancer, lro, services, DEFAULT_REGION};

use serde_json::{json, Value};

/// The APIs the marketing project needs and nothing more. Container, Secret
/// Manager, and IAP are deliberately absent: enabling them here would
/// advertise a capability a static site must not grow.
pub const REQUIRED_SERVICES: &[&str] = &[
    "certificatemanager.googleapis.com",
    "compute.googleapis.com",
    "iam.googleapis.com",
    "iamcredentials.googleapis.com",
    "storage.googleapis.com",
    "sts.googleapis.com",
];

/// The Actions OIDC issuer for this GitHub Enterprise Cloud tenant.
///
/// Data residency puts the issuer on the enterprise subdomain rather than on
/// the shared `token.actions.githubusercontent.com`. Verify with
/// `curl https://token.actions.githubusercontent.com/.well-known/openid-configuration`
/// before changing it — a wrong issuer fails closed, at token exchange, with
/// an error that does not name this constant.
///
/// Aliased rather than repeated: the tenant has exactly one issuer, and two
/// copies of that literal can drift apart while both still compile.
pub const GHE_OIDC_ISSUER: &str = super::artifact_registry::GITHUB_OIDC_ISSUER;

/// The GitHub organization on the enterprise that owns both marketing sites.
pub const GHE_REPOSITORY_OWNER: &str = "marketing";

/// The Workload Identity provider for the enterprise's Actions. Distinct from
/// [`super::artifact_registry::WIF_PROVIDER_ID`] because it admits a different
/// set of repositories; sharing an id would let one overwrite the other.
pub const GHE_WIF_PROVIDER_ID: &str = "ghe-oidc";

/// What a site's deployer needs on its own bucket, and nothing more. Both are
/// bound on the bucket rather than the project, so neither conveys anything
/// about the sibling site or the ability to change this bucket's own policy.
///
/// `objectAdmin` alone is not enough. It covers objects, but `gcloud storage
/// rsync` also reads the destination bucket's metadata, and a sync whose
/// destination is the bucket root fails with `storage.buckets.get denied`
/// while the same sync into a prefix succeeds — so the gap only shows up on
/// one of the two upload passes. `legacyBucketReader` supplies `buckets.get`
/// and object listing without granting any write on the bucket itself.
const DEPLOYER_ROLES: &[&str] = &[
    "roles/storage.objectAdmin",
    "roles/storage.legacyBucketReader",
];

/// One published marketing site.
///
/// The names are derived from `slug` rather than stored separately so a new
/// site cannot be half-registered — every resource for a site moves together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketingSite {
    /// Short identifier; the stem of every resource name for this site.
    pub slug: &'static str,
    /// The hostname the managed certificate covers and DNS will point here.
    pub domain: &'static str,
    /// `owner/repo` on the enterprise, trusted to deploy this site alone.
    pub github_repo: &'static str,
}

impl MarketingSite {
    /// The published bucket. Bucket names are globally unique across all of
    /// GCS, so this is project-prefixed rather than bare.
    #[must_use]
    pub fn bucket(&self) -> String {
        format!("neon-law-marketing-{}", self.slug)
    }

    /// Deployer service-account id. Google caps the local part at 30
    /// characters; the longest current slug leaves ample room.
    #[must_use]
    pub fn deployer_account_id(&self) -> String {
        format!("{}-deployer", self.slug)
    }

    #[must_use]
    pub fn deployer_email(&self, project_id: &str) -> String {
        format!(
            "{}@{project_id}.iam.gserviceaccount.com",
            self.deployer_account_id()
        )
    }

    #[must_use]
    pub fn address_name(&self) -> String {
        format!("{}-ip", self.slug)
    }

    #[must_use]
    pub fn backend_bucket_name(&self) -> String {
        format!("{}-backend", self.slug)
    }

    #[must_use]
    pub fn certificate_name(&self) -> String {
        format!("{}-cert", self.slug)
    }

    /// The DNS authorization that proves control of [`Self::domain`]. Its
    /// generated `CNAME` must stay published for renewal to keep working.
    #[must_use]
    pub fn dns_authorization_name(&self) -> String {
        format!("{}-auth", self.slug)
    }

    #[must_use]
    pub fn certificate_map_name(&self) -> String {
        format!("{}-map", self.slug)
    }

    #[must_use]
    pub fn certificate_map_entry_name(&self) -> String {
        format!("{}-entry", self.slug)
    }

    #[must_use]
    pub fn url_map_name(&self) -> String {
        format!("{}-urlmap", self.slug)
    }

    #[must_use]
    pub fn redirect_url_map_name(&self) -> String {
        format!("{}-redirect", self.slug)
    }

    #[must_use]
    pub fn https_proxy_name(&self) -> String {
        format!("{}-https-proxy", self.slug)
    }

    #[must_use]
    pub fn http_proxy_name(&self) -> String {
        format!("{}-http-proxy", self.slug)
    }

    #[must_use]
    pub fn https_rule_name(&self) -> String {
        format!("{}-https", self.slug)
    }

    #[must_use]
    pub fn http_rule_name(&self) -> String {
        format!("{}-http", self.slug)
    }

    /// The federated principal for this site's repository.
    ///
    /// Scoped to `attribute.repository`, not to the pool or the owner, so a
    /// workflow in the sibling marketing repository cannot impersonate this
    /// site's deployer and publish over it.
    #[must_use]
    pub fn wif_principal(&self, project_number: &str) -> String {
        format!(
            "principalSet://iam.googleapis.com/projects/{project_number}/locations/global/\
             workloadIdentityPools/{WIF_POOL_ID}/attribute.repository/{}",
            self.github_repo
        )
    }

    /// The provider resource name a workflow passes to
    /// `google-github-actions/auth`.
    #[must_use]
    pub fn wif_provider_resource(project_number: &str) -> String {
        format!(
            "projects/{project_number}/locations/global/workloadIdentityPools/{WIF_POOL_ID}/\
             providers/{GHE_WIF_PROVIDER_ID}"
        )
    }
}

/// The published sites.
///
/// `neonlaw.org` is the Foundation's marketing hostname and is deliberately
/// not `neonlaw.com`, which the `neon-production` deployment serves.
pub const SITES: &[MarketingSite] = &[MarketingSite {
    slug: "foundation",
    domain: "www.neonlaw.org",
    github_repo: "marketing/neon-law-foundation",
}];

/// Per-run overrides. Kept minimal on purpose: the site list is a property of
/// the workspace, not something an operator retypes at the command line.
#[derive(Debug, Clone)]
pub struct MarketingSetupConfig {
    /// Bucket and load-balancer location. Default: `us-west4`.
    pub region: String,
}

impl Default for MarketingSetupConfig {
    fn default() -> Self {
        Self {
            region: DEFAULT_REGION.to_string(),
        }
    }
}

/// What one site ended up with, for the operator-facing summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteOutcome {
    pub slug: &'static str,
    pub domain: &'static str,
    pub bucket: String,
    pub deployer: String,
    /// The provider resource the site's workflow passes to
    /// `google-github-actions/auth`. Project-scoped, so every site shares it.
    pub wif_provider: String,
    /// The address the site's DNS `A` record must point at. `None` in dry-run.
    pub ip: Option<String>,
    /// The `CNAME` proving control of the hostname. It must be published
    /// *before* the `A` record moves — that ordering is what makes the cutover
    /// carry no TLS gap — and must stay published for renewal.
    pub dns_authorization: Option<certificate_manager::DnsAuthorizationRecord>,
    /// `PROVISIONING` until the authorization record resolves, then `ACTIVE`.
    pub certificate_state: Option<String>,
}

/// Provision every site in [`SITES`]. Steps, per site, in dependency order:
///
/// 1. The public website bucket, and its anonymous read grant.
/// 2. The deployer service account, and `objectAdmin` on that bucket only.
/// 3. The load balancer: address, backend bucket, certificate, URL map,
///    HTTPS proxy, forwarding rule, and the port-80 redirect chain.
/// 4. `roles/iam.workloadIdentityUser` for the site's federated GitHub
///    principal on its own deployer.
///
/// The shared Workload Identity pool and GHE provider are created once, before
/// the per-site loop, because every site federates through them.
pub async fn run(
    client: &GcpClient,
    project_id: &str,
    config: &MarketingSetupConfig,
) -> SetupResult<Vec<SiteOutcome>> {
    tenants::validate_target(TenantRole::Marketing, project_id)?;

    services::enable(client, project_id, REQUIRED_SERVICES).await?;

    ensure_wif_pool(client, project_id).await?;
    ensure_ghe_wif_provider(client, project_id).await?;
    let project_number = super::artifact_registry::project_number(client, project_id).await?;

    let mut outcomes = Vec::with_capacity(SITES.len());
    for site in SITES {
        outcomes.push(provision_site(client, project_id, &project_number, site, config).await?);
    }
    Ok(outcomes)
}

async fn provision_site(
    client: &GcpClient,
    project_id: &str,
    project_number: &str,
    site: &MarketingSite,
    config: &MarketingSetupConfig,
) -> SetupResult<SiteOutcome> {
    let bucket = site.bucket();
    buckets::ensure_website_bucket(client, project_id, &bucket, &config.region).await?;
    buckets::ensure_public_read(client, &bucket).await?;

    let deployer = site.deployer_email(project_id);
    ensure_deployer_account(client, project_id, site).await?;
    ensure_bucket_deployer_roles(client, &bucket, &deployer).await?;

    ensure_certificate_chain(client, project_id, site).await?;
    ensure_load_balancer(client, project_id, site).await?;

    ensure_wif_impersonation(
        client,
        project_id,
        &deployer,
        &site.wif_principal(project_number),
    )
    .await?;

    let ip = load_balancer::global_address_ip(client, project_id, &site.address_name()).await?;
    let dns_authorization = certificate_manager::dns_authorization_record(
        client,
        project_id,
        &site.dns_authorization_name(),
    )
    .await?;
    let certificate_state =
        certificate_manager::certificate_state(client, project_id, &site.certificate_name())
            .await?;

    Ok(SiteOutcome {
        slug: site.slug,
        domain: site.domain,
        bucket,
        deployer,
        wif_provider: MarketingSite::wif_provider_resource(project_number),
        ip,
        dns_authorization,
        certificate_state,
    })
}

/// The certificate chain: a DNS authorization, the certificate it validates,
/// and the map the load balancer's proxy points at.
///
/// Separated from [`ensure_load_balancer`] because it is the half that does not
/// depend on DNS pointing here yet — that is the entire reason this workspace
/// uses Certificate Manager rather than a classic managed certificate.
async fn ensure_certificate_chain(
    client: &GcpClient,
    project_id: &str,
    site: &MarketingSite,
) -> SetupResult<()> {
    certificate_manager::ensure_dns_authorization(
        client,
        project_id,
        &site.dns_authorization_name(),
        site.domain,
    )
    .await?;
    certificate_manager::ensure_certificate(
        client,
        project_id,
        &site.certificate_name(),
        site.domain,
        &site.dns_authorization_name(),
    )
    .await?;
    certificate_manager::ensure_certificate_map(client, project_id, &site.certificate_map_name())
        .await?;
    certificate_manager::ensure_certificate_map_entry(
        client,
        project_id,
        &site.certificate_map_name(),
        &site.certificate_map_entry_name(),
        site.domain,
        &site.certificate_name(),
    )
    .await
}

/// The five-resource HTTPS chain plus the port-80 redirect.
async fn ensure_load_balancer(
    client: &GcpClient,
    project_id: &str,
    site: &MarketingSite,
) -> SetupResult<()> {
    let certificate_map =
        certificate_manager::map_reference(project_id, &site.certificate_map_name());

    load_balancer::ensure_global_address(client, project_id, &site.address_name()).await?;
    load_balancer::ensure_backend_bucket(
        client,
        project_id,
        &site.backend_bucket_name(),
        &site.bucket(),
    )
    .await?;
    load_balancer::ensure_url_map(
        client,
        project_id,
        &site.url_map_name(),
        &site.backend_bucket_name(),
    )
    .await?;
    load_balancer::ensure_target_https_proxy(
        client,
        project_id,
        &site.https_proxy_name(),
        &site.url_map_name(),
        &certificate_map,
    )
    .await?;
    // `insert` is a no-op on a proxy that already exists, so a proxy created
    // before the move to Certificate Manager would keep its old certificate
    // reference without this.
    load_balancer::set_proxy_certificate_map(
        client,
        project_id,
        &site.https_proxy_name(),
        &certificate_map,
    )
    .await?;
    load_balancer::ensure_global_forwarding_rule(
        client,
        project_id,
        &site.https_rule_name(),
        &site.address_name(),
        "targetHttpsProxies",
        &site.https_proxy_name(),
        "443",
    )
    .await?;

    load_balancer::ensure_redirect_url_map(client, project_id, &site.redirect_url_map_name())
        .await?;
    load_balancer::ensure_target_http_proxy(
        client,
        project_id,
        &site.http_proxy_name(),
        &site.redirect_url_map_name(),
    )
    .await?;
    load_balancer::ensure_global_forwarding_rule(
        client,
        project_id,
        &site.http_rule_name(),
        &site.address_name(),
        "targetHttpProxies",
        &site.http_proxy_name(),
        "80",
    )
    .await?;
    Ok(())
}

/// Idempotently create the GHE OIDC provider under the shared pool.
///
/// The attribute condition pins it to the `marketing` owner, so no other
/// organization on this enterprise can mint a token the project will trust.
pub async fn ensure_ghe_wif_provider(client: &GcpClient, project_id: &str) -> SetupResult<()> {
    let path = format!(
        "/v1/projects/{project_id}/locations/global/workloadIdentityPools/{WIF_POOL_ID}/\
         providers?workloadIdentityPoolProviderId={GHE_WIF_PROVIDER_ID}"
    );
    let body = json!({
        "displayName": "GitHub Enterprise OIDC",
        "oidc": { "issuerUri": GHE_OIDC_ISSUER },
        "attributeMapping": {
            "google.subject": "assertion.sub",
            "attribute.repository": "assertion.repository",
            "attribute.repository_owner": "assertion.repository_owner"
        },
        "attributeCondition": format!("assertion.repository_owner == '{GHE_REPOSITORY_OWNER}'")
    });
    create_or_conflict(
        client,
        GcpService::Iam,
        &path,
        &body,
        "create GHE WIF provider",
    )
    .await
}

/// Idempotently create the site's deployer service account.
///
/// `serviceAccounts.create` returns the finished `ServiceAccount`, not a
/// long-running operation, so this must *not* be routed through
/// [`create_or_conflict`]. Doing so treats the returned resource as an
/// incomplete LRO and polls its `name`, which races IAM's own propagation and
/// fails the whole run with a 404 for the account that was just created.
async fn ensure_deployer_account(
    client: &GcpClient,
    project_id: &str,
    site: &MarketingSite,
) -> SetupResult<()> {
    let path = format!("/v1/projects/{project_id}/serviceAccounts");
    let body = json!({
        "accountId": site.deployer_account_id(),
        "serviceAccount": {
            "displayName": format!("{} deploy identity", site.domain),
            "description": format!("Publishes {} from {}", site.domain, site.github_repo),
        },
    });
    let resp = client.post_json(GcpService::Iam, &path, &body).await?;
    match resp.status_u16() {
        200..=299 | 409 => Ok(()),
        other => Err(super::error::SetupError::BadStatus {
            operation: format!(
                "create deployer service account {}",
                site.deployer_account_id()
            ),
            status: other,
            body: resp.into_text(),
        }),
    }
}

/// Grant the deployer [`DEPLOYER_ROLES`] on its own bucket, via the bucket's
/// IAM policy rather than a project-level role — so the identity can write to
/// the site it owns and to nothing else in the project.
///
/// Reads the live policy once and writes once, adding only the roles that are
/// missing, so it preserves the anonymous read grant and any binding added by
/// hand, and makes no call at all on a converged re-run.
async fn ensure_bucket_deployer_roles(
    client: &GcpClient,
    bucket: &str,
    deployer_email: &str,
) -> SetupResult<()> {
    let member = format!("serviceAccount:{deployer_email}");
    let path = format!("/storage/v1/b/{bucket}/iam");

    let response = client.get(GcpService::Storage, &path).await?;
    let status = response.status_u16();
    if !(200..=299).contains(&status) {
        return Err(super::error::SetupError::BadStatus {
            operation: format!("read IAM policy for bucket {bucket}"),
            status,
            body: response.into_text(),
        });
    }
    let mut policy: Value = serde_json::from_str(&response.into_text()).map_err(|source| {
        super::error::SetupError::Json {
            what: "bucket IAM policy",
            source,
        }
    })?;

    let mut bindings = policy
        .get("bindings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut changed = false;
    for role in DEPLOYER_ROLES {
        let already_bound = bindings.iter().any(|binding| {
            binding.get("role").and_then(Value::as_str) == Some(*role)
                && binding
                    .get("members")
                    .and_then(Value::as_array)
                    .is_some_and(|members| members.iter().any(|m| m.as_str() == Some(&member)))
        });
        if !already_bound {
            bindings.push(json!({ "role": role, "members": [member] }));
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }

    policy["bindings"] = Value::Array(bindings);

    let resp = client.put_json(GcpService::Storage, &path, &policy).await?;
    match resp.status_u16() {
        200..=299 => Ok(()),
        other => Err(super::error::SetupError::BadStatus {
            operation: format!("grant deployer roles on bucket {bucket}"),
            status: other,
            body: resp.into_text(),
        }),
    }
}

/// POST an IAM create, waiting on any LRO and treating 409 as already done.
async fn create_or_conflict(
    client: &GcpClient,
    service: GcpService,
    path: &str,
    body: &Value,
    operation: &'static str,
) -> SetupResult<()> {
    let resp = client.post_json(service, path, body).await?;
    match resp.status_u16() {
        200..=299 => {
            let op: Value = serde_json::from_str(&resp.into_text()).map_err(|source| {
                super::error::SetupError::Json {
                    what: "create operation",
                    source,
                }
            })?;
            lro::wait(client, service, &op, "/v1/{name}").await?;
            Ok(())
        }
        409 => Ok(()),
        other => Err(super::error::SetupError::BadStatus {
            operation: operation.to_string(),
            status: other,
            body: resp.into_text(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::{GcpService, StaticToken};
    use super::*;

    fn offline_dry_run_client() -> GcpClient {
        GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ServiceUsage, "http://127.0.0.1:1")
            .with_base_url(GcpService::Iam, "http://127.0.0.1:1")
            .with_base_url(GcpService::Storage, "http://127.0.0.1:1")
            .with_base_url(GcpService::Compute, "http://127.0.0.1:1")
            .with_base_url(GcpService::CloudResourceManager, "http://127.0.0.1:1")
            .with_base_url(GcpService::Container, "http://127.0.0.1:1")
            .with_dry_run()
    }

    #[test]
    fn marketing_enables_no_environment_capability() {
        for forbidden in [
            "container.googleapis.com",
            "secretmanager.googleapis.com",
            "anthosconfigmanagement.googleapis.com",
            "iap.googleapis.com",
        ] {
            assert!(
                !REQUIRED_SERVICES.contains(&forbidden),
                "a static site is not an environment; it must not enable {forbidden}",
            );
        }
    }

    /// The single most breakable fact in this module. GitHub Enterprise Cloud
    /// with data residency does not use the public issuer, and a provider
    /// carrying the wrong one fails at token exchange with an error that never
    /// names the constant.
    /// The issuer is the standard public one.
    ///
    /// This asserted the opposite while Navigator sat on a data-residency
    /// enterprise, which minted its own Actions tokens under the tenant host: a
    /// provider pinned to the wrong issuer is accepted at create time, reports
    /// `ACTIVE`, and then fails every token exchange with an error that never
    /// names the constant. The repository is public now, so the standard issuer
    /// is the correct one — and the same failure mode applies in reverse.
    #[test]
    fn the_oidc_issuer_is_the_public_github_one() {
        assert_eq!(
            GHE_OIDC_ISSUER,
            "https://token.actions.githubusercontent.com"
        );
        assert_eq!(
            GHE_OIDC_ISSUER,
            super::super::artifact_registry::GITHUB_OIDC_ISSUER,
            "one enterprise mints from one host; keep the constant single-sourced",
        );
    }

    #[test]
    fn the_ghe_provider_does_not_collide_with_the_github_com_provider() {
        assert_ne!(
            GHE_WIF_PROVIDER_ID,
            super::super::artifact_registry::WIF_PROVIDER_ID,
            "two providers trusting different issuers cannot share an id",
        );
    }

    /// Uniqueness across the registry, plus a registry that cannot silently
    /// empty. A pairwise comparison asserts nothing while `SITES` holds a
    /// single entry, so the emptiness check is what keeps this honest: a
    /// `SITES` that lost its last site would leave every test in this module
    /// passing over nothing, and `ops gcp marketing setup` would report a
    /// clean run having provisioned no site at all.
    #[test]
    fn every_site_has_a_distinct_slug_bucket_and_repository() {
        assert!(
            !SITES.is_empty(),
            "the marketing provisioner needs at least one site to provision",
        );

        let slugs: BTreeSet<&str> = SITES.iter().map(|site| site.slug).collect();
        let buckets: BTreeSet<String> = SITES.iter().map(MarketingSite::bucket).collect();
        let repositories: BTreeSet<&str> = SITES.iter().map(|site| site.github_repo).collect();
        let domains: BTreeSet<&str> = SITES.iter().map(|site| site.domain).collect();

        assert_eq!(slugs.len(), SITES.len(), "two sites share a slug");
        assert_eq!(buckets.len(), SITES.len(), "two sites share a bucket");
        assert_eq!(
            repositories.len(),
            SITES.len(),
            "two sites share a repository",
        );
        assert_eq!(domains.len(), SITES.len(), "two sites share a hostname");
    }

    /// Google rejects a service-account id outside 6..=30 characters, and the
    /// failure arrives partway through provisioning rather than at parse time.
    #[test]
    fn every_deployer_account_id_is_a_legal_length() {
        for site in SITES {
            let id = site.deployer_account_id();
            assert!(
                (6..=30).contains(&id.len()),
                "{id} is {} characters; Google allows 6 to 30",
                id.len(),
            );
        }
    }

    /// The Foundation's marketing hostname is `.org`. `neonlaw.com` is served
    /// by the `neon-production` deployment, and pointing a marketing
    /// certificate at it would collide with that deployment's own.
    #[test]
    fn the_foundation_site_is_the_org_domain_not_the_com() {
        let foundation = SITES.iter().find(|s| s.slug == "foundation").unwrap();
        assert_eq!(foundation.domain, "www.neonlaw.org");
    }

    /// Scoping to `attribute.repository` is what stops one marketing
    /// repository from publishing over the other's bucket. A principal scoped
    /// only to the pool or the owner would let either deploy both.
    #[test]
    fn each_principal_is_scoped_to_one_repository() {
        for site in SITES {
            let principal = site.wif_principal("93609063550");
            assert!(
                principal.ends_with(&format!("attribute.repository/{}", site.github_repo)),
                "{principal}",
            );
            for other in SITES {
                if other.slug != site.slug {
                    assert!(!principal.contains(other.github_repo), "{principal}");
                }
            }
        }
    }

    #[tokio::test]
    async fn the_marketing_command_refuses_an_environment_before_any_call() {
        let client = offline_dry_run_client();
        let err = run(&client, "neon-law", &MarketingSetupConfig::default())
            .await
            .expect_err("neon-law runs the application, not the marketing sites");

        assert!(err.to_string().contains("neon-law"), "{err}");
        assert!(
            client.recorded_calls().is_empty(),
            "the tenant guard must precede every GCP call, got {:?}",
            client.recorded_calls(),
        );
    }

    /// The guarantee that gives this command its name: a marketing run creates
    /// no database, no cluster, and no private document storage.
    #[tokio::test]
    async fn a_dry_run_touches_no_environment_resource() {
        let client = offline_dry_run_client();
        run(
            &client,
            "neon-law-marketing",
            &MarketingSetupConfig::default(),
        )
        .await
        .unwrap();

        for call in client.recorded_calls() {
            for forbidden in ["/instances", "/clusters", "-documents", "/global/networks"] {
                assert!(
                    !call.url.contains(forbidden),
                    "marketing setup must not touch `{forbidden}`: {call:?}",
                );
            }
            assert_ne!(call.method, "SHELL", "marketing shells out to nothing");
        }
    }

    #[tokio::test]
    async fn a_dry_run_provisions_both_sites() {
        let client = offline_dry_run_client();
        let outcomes = run(
            &client,
            "neon-law-marketing",
            &MarketingSetupConfig::default(),
        )
        .await
        .unwrap();

        assert_eq!(outcomes.len(), SITES.len());
        let urls: Vec<String> = client
            .recorded_calls()
            .iter()
            .map(|c| c.url.clone())
            .collect();
        let joined = urls.join("\n");

        for site in SITES {
            assert!(
                joined.contains("b?project=neon-law-marketing") && joined.contains(site.slug),
                "no bucket create recorded for {}",
                site.slug,
            );
            for resource in [
                "global/addresses",
                "global/backendBuckets",
                "global/urlMaps",
                "global/targetHttpsProxies",
                "global/targetHttpProxies",
                "global/forwardingRules",
                // The certificate chain, which is Certificate Manager rather
                // than a `compute` resource precisely so it validates by DNS.
                "global/dnsAuthorizations",
                "global/certificates",
                "global/certificateMaps",
            ] {
                assert!(
                    joined.contains(resource),
                    "no {resource} call recorded: {joined}",
                );
            }
        }

        // The classic per-load-balancer certificate is gone. Creating one here
        // would reintroduce the CA-calls-the-load-balancer validation this
        // command exists to avoid, and it would be silently shadowed by the
        // certificate map the proxy actually reads.
        assert!(
            !joined.contains("global/sslCertificates"),
            "the certificate must come from Certificate Manager, not compute: {joined}",
        );
    }

    /// `objectAdmin` covers objects but not the bucket resource, and `gcloud
    /// storage rsync` reads the destination bucket's metadata. The gap is
    /// invisible on a sync into a prefix and fails only on a sync to the
    /// bucket root, so the first real deploy uploaded the assets and then died
    /// on `storage.buckets.get denied` for the HTML.
    #[test]
    fn a_deployer_can_read_the_bucket_it_writes_to() {
        assert!(
            DEPLOYER_ROLES.contains(&"roles/storage.legacyBucketReader"),
            "rsync to the bucket root needs storage.buckets.get, which \
             objectAdmin does not grant",
        );
        assert!(DEPLOYER_ROLES.contains(&"roles/storage.objectAdmin"));
    }

    /// The deployer publishes one site. Anything that could reach the sibling
    /// bucket, rewrite this bucket's own policy, or act project-wide is more
    /// than publishing needs.
    #[test]
    fn a_deployer_holds_no_project_or_policy_authority() {
        for role in DEPLOYER_ROLES {
            assert!(
                !matches!(
                    *role,
                    "roles/storage.admin" | "roles/owner" | "roles/editor"
                ),
                "{role} exceeds what publishing one bucket requires",
            );
        }
    }

    /// `serviceAccounts.create` answers with the finished `ServiceAccount`,
    /// which carries a `name` but no `done`. Routing it through the LRO helper
    /// reads that as an unfinished operation and polls the account's own
    /// resource path, which races IAM propagation and 404s on the account that
    /// was just created — failing the run after the buckets already exist.
    ///
    /// The mock answers exactly like the real API and refuses any follow-up
    /// GET, so a regression puts the poll back and this test fails.
    #[tokio::test]
    async fn creating_a_deployer_does_not_poll_it_as_an_operation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/neon-law-marketing/serviceAccounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "projects/neon-law-marketing/serviceAccounts/\
                         foundation-deployer@neon-law-marketing.iam.gserviceaccount.com",
                "email": "foundation-deployer@neon-law-marketing.iam.gserviceaccount.com",
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .expect(0)
            .mount(&server)
            .await;

        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Iam, server.uri());
        ensure_deployer_account(&client, "neon-law-marketing", &SITES[0])
            .await
            .unwrap();
    }

    /// A second run finds the account already there. 409 is convergence, not
    /// an error, or re-running the command could never repair a partial run.
    #[tokio::test]
    async fn an_existing_deployer_is_convergence_not_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;

        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Iam, server.uri());
        ensure_deployer_account(&client, "neon-law-marketing", &SITES[0])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn the_provider_trusts_the_enterprise_issuer_and_only_the_marketing_owner() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/v1/projects/neon-law-marketing/locations/global/workloadIdentityPools/github/providers",
            ))
            .and(query_param("workloadIdentityPoolProviderId", "ghe-oidc"))
            .and(body_partial_json(json!({
                "oidc": { "issuerUri": "https://token.actions.githubusercontent.com" },
                "attributeCondition": "assertion.repository_owner == 'marketing'",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "done": true })))
            .expect(1)
            .mount(&server)
            .await;

        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Iam, server.uri());
        ensure_ghe_wif_provider(&client, "neon-law-marketing")
            .await
            .unwrap();
    }
}
