//! Admin calls to **GCP Identity Platform** for the password-reset and
//! email-confirm flows.
//!
//! The public email/password sign-in door ([`crate::oauth::password_login`])
//! only needs the project's *browser* API key — a public value that scopes
//! anonymous Identity Toolkit calls. Resetting a password or marking an
//! email verified for a **signed-out** user is different: the user can't
//! present their own ID token, so we act as the project admin. That needs
//! a service-account **bearer token**.
//!
//! To keep `web` free of the GCP SDK (per the workspace's two-crate cloud
//! rule), we mint that token from the **GCE metadata server** over plain
//! `reqwest` — `GET …/instance/service-accounts/default/token` with the
//! `Metadata-Flavor: Google` header — exactly as Google documents for
//! Cloud Run. On the reference deploy `web` runs as a Workload-Identity
//! GSA granted `roles/identitytoolkit.admin`; the metadata endpoint is
//! overridable so tests point it at a mock.
//!
//! Nothing here logs a password, a token, or a `localId` — only the
//! call's outcome, per the observability rule.

use serde::Deserialize;

/// Configuration for the Identity Platform **admin** REST surface.
#[derive(Clone)]
pub struct IdentityAdminConfig {
    /// GCP project id — the `projects/{id}` segment of the admin REST
    /// path. Sourced from `NAVIGATOR_GCP_PROJECT_ID`.
    pub project_id: String,
    /// Identity Toolkit REST base. `https://identitytoolkit.googleapis.com`
    /// in prod; tests point it at a mock.
    pub endpoint: String,
    /// GCE metadata server base. The real server in prod; tests point it
    /// at a mock so a service-account token can be minted offline.
    pub metadata_endpoint: String,
}

impl IdentityAdminConfig {
    /// Production Identity Toolkit REST base.
    pub const DEFAULT_ENDPOINT: &'static str = "https://identitytoolkit.googleapis.com";
    /// Production GCE metadata server base.
    pub const DEFAULT_METADATA_ENDPOINT: &'static str = "http://metadata.google.internal";

    /// Build from the environment. Returns `None` when
    /// `NAVIGATOR_GCP_PROJECT_ID` is unset or empty — the admin door is
    /// then off and the reset / confirm flows refuse cleanly. Endpoint and
    /// metadata base default to the production hosts unless overridden
    /// (`NAVIGATOR_IDENTITY_PLATFORM_ENDPOINT` / `NAVIGATOR_GCP_METADATA_ENDPOINT`).
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let project_id = std::env::var("NAVIGATOR_GCP_PROJECT_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let endpoint = std::env::var("NAVIGATOR_IDENTITY_PLATFORM_ENDPOINT")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| Self::DEFAULT_ENDPOINT.to_string());
        let metadata_endpoint = std::env::var("NAVIGATOR_GCP_METADATA_ENDPOINT")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| Self::DEFAULT_METADATA_ENDPOINT.to_string());
        Some(Self {
            project_id,
            endpoint,
            metadata_endpoint,
        })
    }
}

/// What went wrong talking to the admin surface. Every variant is logged
/// by identifier/outcome only; the handler collapses them to a generic
/// 5xx so a caller never learns which Google call failed.
#[derive(Debug, thiserror::Error)]
pub enum IdentityAdminError {
    /// Couldn't obtain a service-account access token from the metadata
    /// server.
    #[error("metadata token fetch failed")]
    Token,
    /// An admin REST call failed at the transport or status level.
    #[error("identity-platform admin call failed")]
    Upstream,
}

/// The fields we read back from an `accounts:lookup`.
#[derive(Debug)]
pub struct AccountInfo {
    /// Identity Platform's stable user id (the `localId`), required to
    /// target an `accounts:update`.
    pub local_id: String,
    /// Whether the account has a password credential — i.e. it is a
    /// non-federated (non-Google) user we can reset / confirm. A
    /// Google-only account reports `false` and the caller treats it as
    /// "no resettable account here."
    pub has_password: bool,
    /// Whether the account carries the `google.com` federation provider.
    /// Lets the password-reset flow mail a truthful "you sign in with
    /// Google" notice instead of staying silent on a non-resettable
    /// account.
    pub is_google: bool,
}

#[derive(Deserialize)]
struct MetadataToken {
    access_token: String,
}

#[derive(Deserialize)]
struct LookupResponse {
    #[serde(default)]
    users: Vec<LookupUser>,
}

#[derive(Deserialize)]
struct LookupUser {
    #[serde(rename = "localId")]
    local_id: String,
    #[serde(rename = "passwordHash", default)]
    password_hash: Option<String>,
    #[serde(rename = "providerUserInfo", default)]
    provider_user_info: Vec<ProviderUserInfo>,
}

#[derive(Deserialize)]
struct ProviderUserInfo {
    #[serde(rename = "providerId", default)]
    provider_id: String,
}

impl IdentityAdminConfig {
    /// Mint a service-account access token from the metadata server.
    async fn access_token(&self) -> Result<String, IdentityAdminError> {
        let url = format!(
            "{}/computeMetadata/v1/instance/service-accounts/default/token",
            self.metadata_endpoint.trim_end_matches('/'),
        );
        let resp = reqwest::Client::new()
            .get(&url)
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "idp-admin: metadata token http error");
                IdentityAdminError::Token
            })?;
        if !resp.status().is_success() {
            tracing::warn!(
                status = resp.status().as_u16(),
                "idp-admin: metadata token non-2xx"
            );
            return Err(IdentityAdminError::Token);
        }
        let body: MetadataToken = resp.json().await.map_err(|e| {
            tracing::warn!(error = %e, "idp-admin: metadata token parse failed");
            IdentityAdminError::Token
        })?;
        Ok(body.access_token)
    }

    /// Admin REST URL for an `accounts:<verb>` call on this project.
    fn accounts_url(&self, verb: &str) -> String {
        format!(
            "{}/v1/projects/{}/accounts:{verb}",
            self.endpoint.trim_end_matches('/'),
            self.project_id,
        )
    }

    /// Look up an account by email. Returns `None` when no Identity
    /// Platform account carries the address. The returned
    /// [`AccountInfo::has_password`] tells the caller whether it is a
    /// resettable password account (vs Google-federated only).
    ///
    /// # Errors
    /// [`IdentityAdminError`] if the token fetch or REST call fails.
    pub async fn lookup_by_email(
        &self,
        email: &str,
    ) -> Result<Option<AccountInfo>, IdentityAdminError> {
        let token = self.access_token().await?;
        let resp = reqwest::Client::new()
            .post(self.accounts_url("lookup"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "email": [email] }))
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "idp-admin: lookup http error");
                IdentityAdminError::Upstream
            })?;
        if !resp.status().is_success() {
            tracing::warn!(status = resp.status().as_u16(), "idp-admin: lookup non-2xx");
            return Err(IdentityAdminError::Upstream);
        }
        let body: LookupResponse = resp.json().await.map_err(|e| {
            tracing::warn!(error = %e, "idp-admin: lookup parse failed");
            IdentityAdminError::Upstream
        })?;
        let Some(user) = body.users.into_iter().next() else {
            return Ok(None);
        };
        let has_password = user.password_hash.as_deref().is_some_and(|h| !h.is_empty())
            || user
                .provider_user_info
                .iter()
                .any(|p| p.provider_id == "password");
        let is_google = user
            .provider_user_info
            .iter()
            .any(|p| p.provider_id == "google.com");
        Ok(Some(AccountInfo {
            local_id: user.local_id,
            has_password,
            is_google,
        }))
    }

    /// Set a new password on the account (`accounts:update`).
    ///
    /// # Errors
    /// [`IdentityAdminError`] if the token fetch or REST call fails.
    pub async fn set_password(
        &self,
        local_id: &str,
        new_password: &str,
    ) -> Result<(), IdentityAdminError> {
        self.update(serde_json::json!({
            "localId": local_id,
            "password": new_password,
        }))
        .await
    }

    /// Mark the account's email verified (`accounts:update`).
    ///
    /// # Errors
    /// [`IdentityAdminError`] if the token fetch or REST call fails.
    pub async fn set_email_verified(&self, local_id: &str) -> Result<(), IdentityAdminError> {
        self.update(serde_json::json!({
            "localId": local_id,
            "emailVerified": true,
        }))
        .await
    }

    /// Shared `accounts:update` POST. The body is never logged — it
    /// carries the new password / the `localId`.
    async fn update(&self, body: serde_json::Value) -> Result<(), IdentityAdminError> {
        let token = self.access_token().await?;
        let resp = reqwest::Client::new()
            .post(self.accounts_url("update"))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "idp-admin: update http error");
                IdentityAdminError::Upstream
            })?;
        if !resp.status().is_success() {
            tracing::warn!(status = resp.status().as_u16(), "idp-admin: update non-2xx");
            return Err(IdentityAdminError::Upstream);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::IdentityAdminConfig;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cfg(endpoint: String, metadata: String) -> IdentityAdminConfig {
        IdentityAdminConfig {
            project_id: "demo-project".into(),
            endpoint,
            metadata_endpoint: metadata,
        }
    }

    async fn mock_metadata(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path(
                "/computeMetadata/v1/instance/service-accounts/default/token",
            ))
            .and(header("Metadata-Flavor", "Google"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.fake",
                "expires_in": 3599,
                "token_type": "Bearer",
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn lookup_reports_a_password_account() {
        let meta = MockServer::start().await;
        let idp = MockServer::start().await;
        mock_metadata(&meta).await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/demo-project/accounts:lookup"))
            .and(header("authorization", "Bearer ya29.fake"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "users": [{
                    "localId": "uid-123",
                    "email": "libra@example.com",
                    "emailVerified": false,
                    "providerUserInfo": [{ "providerId": "password" }],
                }],
            })))
            .mount(&idp)
            .await;

        let info = cfg(idp.uri(), meta.uri())
            .lookup_by_email("libra@example.com")
            .await
            .unwrap()
            .expect("account found");
        assert_eq!(info.local_id, "uid-123");
        assert!(info.has_password, "password provider ⇒ resettable");
    }

    #[tokio::test]
    async fn lookup_of_unknown_email_is_none() {
        let meta = MockServer::start().await;
        let idp = MockServer::start().await;
        mock_metadata(&meta).await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/demo-project/accounts:lookup"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&idp)
            .await;

        let info = cfg(idp.uri(), meta.uri())
            .lookup_by_email("nobody@example.com")
            .await
            .unwrap();
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn google_only_account_reports_no_password() {
        let meta = MockServer::start().await;
        let idp = MockServer::start().await;
        mock_metadata(&meta).await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/demo-project/accounts:lookup"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "users": [{
                    "localId": "uid-g",
                    "providerUserInfo": [{ "providerId": "google.com" }],
                }],
            })))
            .mount(&idp)
            .await;

        let info = cfg(idp.uri(), meta.uri())
            .lookup_by_email("g@example.com")
            .await
            .unwrap()
            .expect("account found");
        assert!(!info.has_password, "google-only ⇒ not resettable");
        assert!(info.is_google, "google.com provider ⇒ is_google");
    }

    #[tokio::test]
    async fn set_password_posts_to_accounts_update() {
        let meta = MockServer::start().await;
        let idp = MockServer::start().await;
        mock_metadata(&meta).await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/demo-project/accounts:update"))
            .and(header("authorization", "Bearer ya29.fake"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "localId": "uid-123" })),
            )
            .expect(1)
            .mount(&idp)
            .await;

        cfg(idp.uri(), meta.uri())
            .set_password("uid-123", "a-new-password")
            .await
            .expect("update succeeds");
    }

    #[tokio::test]
    async fn metadata_failure_surfaces_as_token_error() {
        let meta = MockServer::start().await;
        let idp = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/computeMetadata/v1/instance/service-accounts/default/token",
            ))
            .respond_with(ResponseTemplate::new(500))
            .mount(&meta)
            .await;

        let err = cfg(idp.uri(), meta.uri())
            .lookup_by_email("x@example.com")
            .await
            .unwrap_err();
        assert!(matches!(err, super::IdentityAdminError::Token));
    }

    /// Regression guard for the BigQuery log-sink schema. The `status` field
    /// must be emitted as a JSON **number** (via `tracing`'s `.as_u16()`),
    /// never the `%resp.status()` Display string `"400 Bad Request"`. The
    /// string form collided with the numeric `status` logged by
    /// `workflows::trigger`: Cloud Logging types every JSON number as a
    /// BigQuery `FLOAT` column, so the string rows were dropped with
    /// `table_invalid_schema` ("Cannot convert value to floating point").
    #[tokio::test]
    async fn non_2xx_logs_status_as_a_json_number() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};
        use tracing_subscriber::fmt::MakeWriter;

        // A `MakeWriter` that appends every emitted byte into a shared buffer
        // so the test can inspect the JSON the subscriber wrote.
        #[derive(Clone)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> MakeWriter<'a> for Buf {
            type Writer = Buf;
            fn make_writer(&'a self) -> Buf {
                self.clone()
            }
        }

        let meta = MockServer::start().await;
        let idp = MockServer::start().await;
        mock_metadata(&meta).await;
        // Identity Toolkit answers 400 → the `idp-admin: lookup non-2xx` warn.
        Mock::given(method("POST"))
            .and(path("/v1/projects/demo-project/accounts:lookup"))
            .respond_with(ResponseTemplate::new(400).set_body_string("Bad Request"))
            .mount(&idp)
            .await;

        let buf = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::WARN)
            .with_writer(Buf(buf.clone()))
            .finish();

        // `#[tokio::test]` runs on a current-thread runtime, so the `warn!`
        // fires on this thread and the thread-local default captures it.
        let err = {
            let _guard = tracing::subscriber::set_default(subscriber);
            cfg(idp.uri(), meta.uri())
                .lookup_by_email("x@example.com")
                .await
                .unwrap_err()
        };
        assert!(matches!(err, super::IdentityAdminError::Upstream));

        let logged = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        let line = logged
            .lines()
            .find(|l| l.contains("idp-admin: lookup non-2xx"))
            .unwrap_or_else(|| panic!("expected the non-2xx warning in: {logged}"));
        assert!(
            line.contains("\"status\":400"),
            "status must be a JSON number, got: {line}"
        );
        assert!(
            !line.contains("\"status\":\""),
            "status must NOT be a string (the bug was \"400 Bad Request\"): {line}"
        );
    }
}
