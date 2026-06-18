//! Provision Cloud SQL Postgres.
//!
//! Three calls in sequence (each idempotent on 409):
//!
//! 1. `instances.insert` — create the instance itself. Public IP
//!    only for now; the connection from Cloud Run happens through
//!    the Cloud SQL Auth Proxy (Unix socket).
//! 2. `databases.insert` — create the `navigator` database inside
//!    the instance.
//! 3. `users.insert` — create the `web` SQL user with a freshly
//!    generated password.
//!
//! ## On the password
//!
//! For a first cut we generate a 32-char URL-safe random password
//! on every run and **print it to stderr exactly once** so the
//! operator can paste it into Secret Manager / Cloud Run env. If
//! the user already exists (409), we don't rotate; the operator
//! must already know the password. When we want managed rotation,
//! switch to IAM database authentication and drop the user entry.

use rand::distr::{Alphanumeric, SampleString};
use serde_json::{json, Value};

use super::client::{GcpClient, GcpService};
use super::error::{SetupError, SetupResult};
use super::{lro, SetupConfig};

/// Default Cloud SQL instance name. Overridable via
/// `NAVIGATOR_SQL_INSTANCE`. The same default appears in
/// `SetupConfig::default`.
pub const DEFAULT_INSTANCE_NAME: &str = "navigator-pg";
pub const DATABASE_NAME: &str = "navigator";
pub const USER_NAME: &str = "web";
pub const DATABASE_VERSION: &str = "POSTGRES_15";
/// Smallest Postgres tier Cloud SQL offers — shared 1 vCPU + 1.7 GB
/// RAM. Cheapest legitimate option at ~$25/mo. Bump to
/// `db-custom-1-3840` (dedicated 1 vCPU + 3.75 GB) when load shows up.
pub const TIER: &str = "db-g1-small";
/// Smallest legal disk size Cloud SQL accepts on Postgres. With
/// `storageAutoResize=true` (set in `insert_instance`), the disk
/// grows on demand without our intervention.
pub const DISK_SIZE_GB: u32 = 10;

#[must_use]
pub fn generate_password() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 32)
}

pub async fn ensure_sql_instance(
    client: &GcpClient,
    project_id: &str,
    config: &SetupConfig,
) -> SetupResult<()> {
    insert_instance(client, project_id, config).await?;
    insert_database(client, project_id, &config.sql_instance).await?;
    insert_user(
        client,
        project_id,
        &config.sql_instance,
        &generate_password(),
    )
    .await?;
    Ok(())
}

async fn insert_instance(
    client: &GcpClient,
    project_id: &str,
    config: &SetupConfig,
) -> SetupResult<()> {
    let body = json!({
        "name": config.sql_instance,
        "region": config.region,
        "databaseVersion": DATABASE_VERSION,
        "settings": {
            "tier": TIER,
            "edition": "ENTERPRISE",
            "dataDiskSizeGb": DISK_SIZE_GB,
            "storageAutoResize": true,
            "backupConfiguration": { "enabled": true, "pointInTimeRecoveryEnabled": true },
            "ipConfiguration": { "ipv4Enabled": true },
            "databaseFlags": [
                { "name": "cloudsql.iam_authentication", "value": "on" }
            ]
        }
    });
    let resp = client
        .post_json(
            GcpService::SqlAdmin,
            &format!("/v1/projects/{project_id}/instances"),
            &body,
        )
        .await?;
    handle_insert_response(client, resp, "instance").await
}

async fn insert_database(client: &GcpClient, project_id: &str, instance: &str) -> SetupResult<()> {
    let body = json!({ "name": DATABASE_NAME, "instance": instance });
    let resp = client
        .post_json(
            GcpService::SqlAdmin,
            &format!("/v1/projects/{project_id}/instances/{instance}/databases"),
            &body,
        )
        .await?;
    handle_insert_response(client, resp, "database").await
}

async fn insert_user(
    client: &GcpClient,
    project_id: &str,
    instance: &str,
    password: &str,
) -> SetupResult<()> {
    let body = json!({
        "name": USER_NAME,
        "instance": instance,
        "password": password,
    });
    let resp = client
        .post_json(
            GcpService::SqlAdmin,
            &format!("/v1/projects/{project_id}/instances/{instance}/users"),
            &body,
        )
        .await?;
    let status = resp.status_u16();
    if status == 409 {
        return Ok(());
    }
    handle_insert_response_owned(client, status, resp.into_text(), "user").await?;
    // The user is *new* — surface the password to the operator
    // exactly once. Dry-run skips this: the password is never
    // actually used, and printing it would confuse "preview"
    // output with credentials the operator needs to save.
    if client.mode() == super::client::Mode::Execute {
        eprintln!("devx gcp setup: generated Postgres password for user `{USER_NAME}`: {password}");
    }
    Ok(())
}

async fn handle_insert_response(
    client: &GcpClient,
    resp: super::client::HttpResponse,
    what: &'static str,
) -> SetupResult<()> {
    let status = resp.status_u16();
    handle_insert_response_owned(client, status, resp.into_text(), what).await
}

async fn handle_insert_response_owned(
    client: &GcpClient,
    status: u16,
    body: String,
    what: &'static str,
) -> SetupResult<()> {
    match status {
        409 => Ok(()),
        200..=299 => {
            let op: Value =
                serde_json::from_str(&body).map_err(|source| SetupError::Json { what, source })?;
            lro::wait(client, GcpService::SqlAdmin, &op, "/v1/{name}").await?;
            Ok(())
        }
        other => Err(SetupError::BadStatus {
            operation: format!("create SQL {what}"),
            status: other,
            body,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::{GcpClient, GcpService, StaticToken};
    use super::super::SetupConfig;
    use super::{ensure_sql_instance, generate_password, DEFAULT_INSTANCE_NAME};

    fn client_for(server: &MockServer) -> GcpClient {
        GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::SqlAdmin, server.uri())
    }

    /// Mount mocks for the three-step happy path: insert instance,
    /// insert database, insert user — each returning a finished
    /// operation. Returns the server so callers can mount more.
    async fn mount_happy_path(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/v1/projects/p/instances"))
            .and(body_partial_json(json!({
                "name": DEFAULT_INSTANCE_NAME,
                "databaseVersion": "POSTGRES_15"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/inst1",
                "done": true
            })))
            .expect(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!(
                "/v1/projects/p/instances/{DEFAULT_INSTANCE_NAME}/databases"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/db1",
                "done": true
            })))
            .expect(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!(
                "/v1/projects/p/instances/{DEFAULT_INSTANCE_NAME}/users"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/user1",
                "done": true
            })))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn happy_path_creates_instance_database_and_user() {
        let server = MockServer::start().await;
        mount_happy_path(&server).await;
        let client = client_for(&server);
        ensure_sql_instance(&client, "p", &SetupConfig::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn idempotent_when_every_resource_returns_409() {
        let server = MockServer::start().await;
        for endpoint_path in [
            "/v1/projects/p/instances".to_string(),
            format!("/v1/projects/p/instances/{DEFAULT_INSTANCE_NAME}/databases"),
            format!("/v1/projects/p/instances/{DEFAULT_INSTANCE_NAME}/users"),
        ] {
            Mock::given(method("POST"))
                .and(path(endpoint_path))
                .respond_with(ResponseTemplate::new(409))
                .expect(1)
                .mount(&server)
                .await;
        }
        let client = client_for(&server);
        ensure_sql_instance(&client, "p", &SetupConfig::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn dry_run_records_exactly_three_posts() {
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::SqlAdmin, "http://127.0.0.1:1")
            .with_dry_run();
        ensure_sql_instance(&client, "p", &SetupConfig::default())
            .await
            .unwrap();
        let calls = client.recorded_calls();
        assert_eq!(calls.len(), 3, "got {calls:?}");
        assert!(calls[0].url.ends_with("/instances"));
        assert!(calls[1].url.contains("/databases"));
        assert!(calls[2].url.contains("/users"));
    }

    #[test]
    fn generated_passwords_are_32_chars_and_random() {
        let a = generate_password();
        let b = generate_password();
        assert_eq!(a.len(), 32);
        assert_eq!(b.len(), 32);
        assert_ne!(a, b, "two consecutive passwords should not match");
    }
}
