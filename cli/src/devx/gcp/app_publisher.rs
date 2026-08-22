//! Per-deployment publisher identity for Project client-portal bundles.
//!
//! A Project repository's CI publishes its built `portal/dist/` to this
//! deployment's private `<deployment>-applications` bucket, keyless, through
//! Workload Identity Federation. This module provisions the Google half of that
//! trust:
//!
//! 1. A `navigator-app-publisher` service account, one per deployment.
//! 2. A custom role holding exactly `storage.objects.create`,
//!    `storage.objects.update` and `storage.objects.get`, bound on the
//!    applications bucket under an IAM **condition** that confines it to this
//!    Project's own `<code>/portal` prefix. Create *and update*, never delete,
//!    and never another Project's objects.
//! 3. A GitHub OIDC Workload Identity provider pinned to the applications
//!    organization on `main`, issued by
//!    [`GITHUB_OIDC_ISSUER`](super::artifact_registry::GITHUB_OIDC_ISSUER).
//! 4. An impersonation binding pinned to the one `<org>/<repo>` allowed to mint
//!    the publisher's token, so a sibling app repository in the same
//!    organization cannot publish as it.
//!
//! ## Why not `roles/storage.objectCreator`, which this used to grant
//!
//! `objectCreator` is create-only, and the module used to defend that as
//! "create, never delete". The never-delete half is still right and still
//! enforced. The create-*only* half became wrong: the publish `cp`s every object
//! on every run — unconditionally, so that no live asset's age runs out under
//! the bucket's Delete rule — and it stamps `index.html` with custom metadata
//! afterwards. Overwriting an existing object and writing its metadata are
//! `storage.objects.create` and `storage.objects.update`, and a create-only role
//! refuses the second and every republish. A publisher provisioned with
//! `objectCreator` succeeds exactly once and then fails on a permission denial.
//!
//! ## Why a condition, and why the role is custom rather than predefined
//!
//! The bucket is **shared**: every Project's portal lives in it under its own
//! `<code>/portal/` prefix, and the prefix is derived by the Action, not
//! enforced by Google. An unconditioned object-write grant on that bucket
//! therefore lets any Project's CI overwrite every other Project's portal — a
//! privileged client-facing artifact that Navigator serves same-origin. The
//! condition is what makes the derived prefix an enforced one.
//!
//! No predefined role is create-and-update without delete: `objectCreator` is
//! create-only, `objectUser` and `objectAdmin` both carry delete. So the role is
//! custom and holds three permissions. It deliberately does **not** hold
//! `storage.objects.list`: listing is evaluated against the *bucket*, so no
//! object-name condition can scope it, and a grant of it would leak every other
//! Project's object names. The publish does not need it — it uses `cp`, which
//! never lists.
//!
//! A condition lives on a binding, and a binding names one role and one member
//! set, so **a shared publisher account carries exactly one prefix**. One
//! publisher identity per Project is a consequence of this shape, not a
//! preference.
//!
//! The consumer half — the composite Action a Project repository runs — is
//! `.github/actions/application-publish`; the provider resource and the service
//! account email are set on each Project repository as repository *secrets*
//! (public identifiers, but they name the deployment's GCP project in a public
//! log; the trust lives in the binding here). See
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

/// Id of the custom role the publisher holds on the applications bucket.
pub const PUBLISHER_ROLE_ID: &str = "navigatorApplicationsPublisher";

/// The permissions that custom role holds, and the complete set.
///
/// `create` overwrites an object (a GCS overwrite is a new generation, not a
/// delete), `update` writes the custom metadata the publish stamps onto
/// `index.html`, and `get` covers the destination probe gcloud performs before
/// writing. `storage.objects.delete` is absent because the publish never
/// deletes, and `storage.objects.list` is absent because listing is evaluated
/// against the bucket and no object-name condition can scope it.
pub const PUBLISHER_PERMISSIONS: &[&str] = &[
    "storage.objects.create",
    "storage.objects.get",
    "storage.objects.update",
];

/// Roles a publisher may hold from an earlier provisioning round, which `ensure`
/// strips off the bucket policy so the narrowed grant is not additive to a wider
/// one left in place.
///
/// `objectCreator` is what this module granted before; `objectAdmin` is what
/// production was hand-patched to when create-only refused a republish. Leaving
/// either alongside the conditioned binding would keep the hole open, since IAM
/// is a union of grants.
const SUPERSEDED_PUBLISHER_ROLES: &[&str] = &[
    "roles/storage.objectCreator",
    "roles/storage.objectAdmin",
    "roles/storage.objectUser",
];

/// The full resource name of the publisher's custom role in `project_id`.
#[must_use]
pub fn publisher_role_name(project_id: &str) -> String {
    format!("projects/{project_id}/roles/{PUBLISHER_ROLE_ID}")
}

/// The IAM condition confining the publisher to one Project's portal prefix.
///
/// Two clauses, and both are needed. The `startsWith` clause covers every object
/// under the prefix; the equality clause covers the prefix path *itself*, which
/// gcloud probes as though it were an object before writing — without it that
/// probe is denied and the publish fails before uploading anything.
///
/// `code` is the Project code, which is also the repository name: the Action
/// derives the object prefix from `github.event.repository.name`, so the
/// condition and the upload path are derived from the same string.
#[must_use]
pub fn publisher_condition_expression(bucket: &str, code: &str) -> String {
    let prefix = format!("projects/_/buckets/{bucket}/objects/{code}/portal");
    format!("resource.name == \"{prefix}\" || resource.name.startsWith(\"{prefix}/\")")
}

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
/// `org`/`repo` name the one Project repository allowed to publish, and `repo`
/// is also the Project code the bucket condition is scoped to — the Action
/// derives its object prefix from the same repository name, so a rename moves
/// both together or neither. The applications bucket must already exist —
/// [`ensure_publisher_grant`] binds the publisher on it — which is why `run`
/// calls this after the buckets stage.
pub async fn ensure(
    client: &GcpClient,
    project_id: &str,
    org: &str,
    repo: &str,
    applications_bucket: &str,
) -> SetupResult<()> {
    let sa = service_account_email(project_id);
    ensure_publisher_account(client, project_id).await?;
    ensure_publisher_role(client, project_id).await?;
    ensure_publisher_grant(client, project_id, applications_bucket, repo, &sa).await?;
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

    // The two coordinates the Project repository sets as repository *secrets*.
    // Printed so an operator can copy them straight into the repository's
    // Actions secrets. They are public identifiers and the trust is enforced by
    // the binding above, so neither is key material — but both name the
    // deployment's GCP project, and a Project repository's Actions log is
    // public, so they are secrets to keep them out of it rather than because
    // they are sensitive. See docs/project-repositories.md and
    // `.github/actions/application-publish/action.yml`.
    eprintln!(
        "gcp setup [{project_id}] set repository secret \
         NAVIGATOR_APP_PUBLISHER_WIF_PROVIDER={}",
        wif_provider_resource(&number)
    );
    eprintln!(
        "gcp setup [{project_id}] set repository secret \
         NAVIGATOR_APP_PUBLISHER_SERVICE_ACCOUNT={sa}"
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

/// Idempotently create the publisher's custom role, holding exactly
/// [`PUBLISHER_PERMISSIONS`].
///
/// A project-level role definition, bound on the bucket by
/// [`ensure_publisher_grant`] — defining a role grants nothing on its own.
/// `roles.create` returns the finished role rather than a long-running
/// operation, and a 409 means it already exists.
///
/// A 409 is *not* followed by a PATCH. The permission set is the contract this
/// module asserts, and silently widening a role an operator narrowed by hand
/// would be the same class of surprise as the hand-patch this change exists to
/// reconcile. A role whose permissions have drifted is a reconcile decision, not
/// a create-path side effect.
async fn ensure_publisher_role(client: &GcpClient, project_id: &str) -> SetupResult<()> {
    let path = format!("/v1/projects/{project_id}/roles?roleId={PUBLISHER_ROLE_ID}");
    let body = json!({
        "role": {
            "title": "Navigator applications publisher",
            "description": "Create and update objects under one Project's portal prefix; \
                            never delete, never list.",
            "includedPermissions": PUBLISHER_PERMISSIONS,
            "stage": "GA",
        },
    });
    let resp = client.post_json(GcpService::Iam, &path, &body).await?;
    match resp.status_u16() {
        200..=299 | 409 => Ok(()),
        other => Err(SetupError::BadStatus {
            operation: format!("create custom role {PUBLISHER_ROLE_ID}"),
            status: other,
            body: resp.into_text(),
        }),
    }
}

/// Bind the publisher's custom role on the applications bucket, under a
/// condition confining it to `code`'s own portal prefix, and strip any
/// superseded wider grant the same publisher still holds.
///
/// Get-merge-put against the live policy, and it does three things rather than
/// one:
///
/// 1. Removes the publisher from any [`SUPERSEDED_PUBLISHER_ROLES`] binding, and
///    drops a binding left with no members. IAM is a union, so adding the narrow
///    grant beside a wide one narrows nothing — this is what reconciles a
///    deployment hand-patched to `objectAdmin`.
/// 2. Ensures exactly one conditioned binding for the custom role.
/// 3. Sets `version: 3`, without which a conditioned binding is rejected. The
///    fetched `etag` travels back untouched, so a concurrent edit loses rather
///    than being silently overwritten.
///
/// It writes only when something actually changed, so a converged re-run makes
/// no request — the property `navigator ops gcp setup` is expected to have.
async fn ensure_publisher_grant(
    client: &GcpClient,
    project_id: &str,
    bucket: &str,
    code: &str,
    publisher_email: &str,
) -> SetupResult<()> {
    let member = format!("serviceAccount:{publisher_email}");
    let role = publisher_role_name(project_id);
    let expression = publisher_condition_expression(bucket, code);
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

    let bindings = policy
        .get("bindings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut changed = false;
    let mut next: Vec<Value> = Vec::with_capacity(bindings.len() + 1);
    let mut already_bound = false;

    for binding in bindings {
        let binding_role = binding.get("role").and_then(Value::as_str).unwrap_or("");
        let holds_publisher = binding
            .get("members")
            .and_then(Value::as_array)
            .is_some_and(|members| members.iter().any(|m| m.as_str() == Some(&member)));

        if holds_publisher && SUPERSEDED_PUBLISHER_ROLES.contains(&binding_role) {
            // Strip the publisher out; keep any other member of that binding.
            let remaining: Vec<Value> = binding
                .get("members")
                .and_then(Value::as_array)
                .map(|members| {
                    members
                        .iter()
                        .filter(|m| m.as_str() != Some(&member))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            changed = true;
            if !remaining.is_empty() {
                let mut kept = binding.clone();
                kept["members"] = Value::Array(remaining);
                next.push(kept);
            }
            continue;
        }

        if binding_role == role && holds_publisher {
            let condition_matches = binding
                .get("condition")
                .and_then(|c| c.get("expression"))
                .and_then(Value::as_str)
                == Some(expression.as_str());
            if condition_matches {
                already_bound = true;
                next.push(binding);
                continue;
            }
            // Same role and member under a different condition — a stale prefix
            // from a repository rename. Replaced rather than added to, so the
            // old prefix stops being writable.
            changed = true;
            continue;
        }

        next.push(binding);
    }

    if !already_bound {
        next.push(json!({
            "role": role,
            "members": [member],
            "condition": {
                "title": "one Project's portal prefix",
                "description":
                    "Confines the publisher to this Project's own `<code>/portal` prefix in \
                     the shared applications bucket.",
                "expression": expression,
            },
        }));
        changed = true;
    }

    let needs_version = policy.get("version").and_then(Value::as_i64) != Some(3);
    if !changed && !needs_version {
        return Ok(());
    }

    policy["bindings"] = Value::Array(next);
    policy["version"] = json!(3);

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

/// Idempotently create the GitHub OIDC provider under the app-publisher pool,
/// pinned to the applications organization on `main`.
///
/// The provider id is [`APP_PUBLISHER_WIF_PROVIDER_ID`], still spelled
/// `ghe-oidc`. That spelling is a live resource id and not narration — see the
/// constant for why renaming it would converge nothing.
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
        // SA create + custom role create + bucket IAM get + bucket IAM put +
        // WIF pool + WIF provider + impersonation get + impersonation set = 8
        // (project number is short-circuited in dry-run, so no CRM lookup).
        //
        // The custom role is its own call because defining a role and binding it
        // are separate operations: the definition is project-level and grants
        // nothing until the conditioned binding on the bucket references it.
        assert_eq!(calls.len(), 8, "unexpected dry-run calls: {calls:?}");
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
        // The publisher holds the custom create-and-update role, never a
        // predefined role that carries delete.
        assert!(joined.contains(PUBLISHER_ROLE_ID));
        assert!(!joined.contains("roles/storage.objectAdmin"));
        assert!(!joined.contains("roles/storage.objectUser"));
        // And never `objects.list`, which cannot be prefix-scoped.
        assert!(!joined.contains("storage.objects.list"));
    }

    const PUBLISHER: &str = "navigator-app-publisher@proj.iam.gserviceaccount.com";
    const PUBLISHER_MEMBER: &str =
        "serviceAccount:navigator-app-publisher@proj.iam.gserviceaccount.com";

    /// The condition names the prefix itself *and* everything beneath it.
    ///
    /// The equality clause is not redundant. gcloud probes the destination
    /// prefix as though it were an object before writing, and a condition
    /// carrying only `startsWith(".../portal/")` denies that probe — the publish
    /// then fails before uploading anything, with a `403` on the prefix path and
    /// no trailing slash.
    #[test]
    fn the_condition_covers_the_prefix_path_and_its_children_only() {
        let expression = publisher_condition_expression("proj-applications", "sample-litigation");
        assert!(expression.contains(
            "resource.name == \"projects/_/buckets/proj-applications/objects/\
             sample-litigation/portal\""
        ));
        assert!(expression.contains(
            "resource.name.startsWith(\"projects/_/buckets/proj-applications/objects/\
             sample-litigation/portal/\")"
        ));
        // Another Project's prefix is not named at all.
        assert!(!expression.contains("sample-estate"));
    }

    /// The custom role holds create and update, and neither delete nor list.
    #[test]
    fn the_custom_role_is_create_and_update_only() {
        assert!(PUBLISHER_PERMISSIONS.contains(&"storage.objects.create"));
        assert!(PUBLISHER_PERMISSIONS.contains(&"storage.objects.update"));
        assert!(
            !PUBLISHER_PERMISSIONS.contains(&"storage.objects.delete"),
            "the publish never deletes; granting delete would remove the one \
             property the never-delete upload order relies on",
        );
        assert!(
            !PUBLISHER_PERMISSIONS.contains(&"storage.objects.list"),
            "listing is evaluated against the bucket, so no object-name \
             condition can scope it, and it would leak every other Project's \
             object names",
        );
        assert_eq!(
            publisher_role_name("proj"),
            "projects/proj/roles/navigatorApplicationsPublisher",
        );
    }

    /// An empty policy gains one conditioned binding, and `version: 3` with it.
    ///
    /// Without `version: 3` a conditioned binding is rejected outright, so the
    /// two travel together or neither works.
    #[tokio::test]
    async fn the_conditioned_binding_is_added_to_an_empty_policy_at_version_three() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/storage/v1/b/proj-applications/iam"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/storage/v1/b/proj-applications/iam"))
            .and(body_partial_json(json!({
                "version": 3,
                "bindings": [{
                    "role": "projects/proj/roles/navigatorApplicationsPublisher",
                    "members": [PUBLISHER_MEMBER],
                    "condition": {
                        "expression": publisher_condition_expression(
                            "proj-applications", "sample-litigation"),
                    },
                }],
            })))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Storage, server.uri());
        ensure_publisher_grant(
            &client,
            "proj",
            "proj-applications",
            "sample-litigation",
            PUBLISHER,
        )
        .await
        .unwrap();
    }

    /// A converged policy is left alone — no PUT at all.
    ///
    /// `navigator ops gcp setup` is expected to be re-runnable, and a second run
    /// reporting no change is the observable form of that. No PUT mock is
    /// mounted, so a write fails the test rather than passing silently.
    #[tokio::test]
    async fn a_converged_policy_is_not_rewritten() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/storage/v1/b/proj-applications/iam"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "version": 3,
                "bindings": [{
                    "role": "projects/proj/roles/navigatorApplicationsPublisher",
                    "members": [PUBLISHER_MEMBER],
                    "condition": {
                        "expression": publisher_condition_expression(
                            "proj-applications", "sample-litigation"),
                    },
                }],
            })))
            .mount(&server)
            .await;
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Storage, server.uri());
        ensure_publisher_grant(
            &client,
            "proj",
            "proj-applications",
            "sample-litigation",
            PUBLISHER,
        )
        .await
        .unwrap();
    }

    /// A superseded wide grant is stripped, not left beside the narrow one.
    ///
    /// This is the reconcile case: production was hand-patched to unconditioned
    /// `objectAdmin` when create-only refused a republish. IAM is a union of
    /// grants, so adding the conditioned binding while leaving `objectAdmin` in
    /// place would narrow nothing at all. Another member of the same wide
    /// binding is kept — only the publisher is stripped out of it.
    #[tokio::test]
    async fn a_superseded_wide_grant_is_stripped_from_the_publisher() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/storage/v1/b/proj-applications/iam"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "bindings": [{
                    "role": "roles/storage.objectAdmin",
                    "members": [PUBLISHER_MEMBER, "serviceAccount:proj-web@proj.iam.gserviceaccount.com"],
                }],
            })))
            .mount(&server)
            .await;
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = std::sync::Arc::clone(&captured);
        Mock::given(method("PUT"))
            .and(path("/storage/v1/b/proj-applications/iam"))
            .respond_with(move |req: &wiremock::Request| {
                sink.lock()
                    .expect("lock")
                    .push(String::from_utf8_lossy(&req.body).to_string());
                ResponseTemplate::new(200)
            })
            .expect(1)
            .mount(&server)
            .await;
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Storage, server.uri());
        ensure_publisher_grant(
            &client,
            "proj",
            "proj-applications",
            "sample-litigation",
            PUBLISHER,
        )
        .await
        .unwrap();

        let body = captured.lock().expect("lock").join("");
        let policy: Value = serde_json::from_str(&body).expect("the written policy parses");
        let bindings = policy["bindings"].as_array().expect("bindings written");

        let admin = bindings
            .iter()
            .find(|b| b["role"] == "roles/storage.objectAdmin")
            .expect("the other member's binding survives");
        let members: Vec<&str> = admin["members"]
            .as_array()
            .expect("members")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            !members.contains(&PUBLISHER_MEMBER),
            "the publisher must be stripped from the wide grant, got {members:?}",
        );
        assert!(
            members.contains(&"serviceAccount:proj-web@proj.iam.gserviceaccount.com"),
            "an unrelated member of the same binding must be preserved, got {members:?}",
        );

        assert!(
            bindings.iter().any(|b| {
                b["role"] == "projects/proj/roles/navigatorApplicationsPublisher"
                    && b["condition"]["expression"].is_string()
            }),
            "the conditioned narrow binding must be present: {bindings:?}",
        );
        assert_eq!(policy["version"], json!(3));
    }

    /// A stale prefix from a repository rename is replaced, not added to.
    ///
    /// The condition carries the Project code, so a rename leaves a binding
    /// naming the old prefix. Left in place it would stay writable, which is
    /// exactly the isolation this grant exists to provide.
    #[tokio::test]
    async fn a_stale_prefix_condition_is_replaced_rather_than_accumulated() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/storage/v1/b/proj-applications/iam"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "version": 3,
                "bindings": [{
                    "role": "projects/proj/roles/navigatorApplicationsPublisher",
                    "members": [PUBLISHER_MEMBER],
                    "condition": {
                        "expression": publisher_condition_expression(
                            "proj-applications", "navigator-sample-project-litigation"),
                    },
                }],
            })))
            .mount(&server)
            .await;
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = std::sync::Arc::clone(&captured);
        Mock::given(method("PUT"))
            .and(path("/storage/v1/b/proj-applications/iam"))
            .respond_with(move |req: &wiremock::Request| {
                sink.lock()
                    .expect("lock")
                    .push(String::from_utf8_lossy(&req.body).to_string());
                ResponseTemplate::new(200)
            })
            .expect(1)
            .mount(&server)
            .await;
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Storage, server.uri());
        ensure_publisher_grant(
            &client,
            "proj",
            "proj-applications",
            "sample-litigation",
            PUBLISHER,
        )
        .await
        .unwrap();

        let body = captured.lock().expect("lock").join("");
        assert!(
            !body.contains("navigator-sample-project-litigation"),
            "the stale prefix must not survive the write: {body}",
        );
        assert!(body.contains("objects/sample-litigation/portal"));
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
