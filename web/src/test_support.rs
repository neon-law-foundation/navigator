//! Shared test scaffolding: a canonical [`AppState`] builder.
//!
//! Every web integration test (and the `features` BDD runner) needs an
//! `AppState` to build a router, and they used to each inline the full
//! ~30-field literal — so adding one field meant editing ~25 files. This
//! builder is the single source of those defaults: a test takes
//! [`app_state`] and overrides only the fields it cares about via struct
//! update syntax:
//!
//! ```ignore
//! let state = AppState {
//!     auth: AuthConfig::new(true, Some(claims)),
//!     oauth: Some(cfg),
//!     ..web::test_support::app_state(db).await
//! };
//! ```
//!
//! It is always compiled (not feature-gated) so both the in-crate
//! integration tests and the downstream `features` crate can reach it
//! without a self dev-dependency; every type it touches
//! (`StubSignatureProvider`, `CapturingEmail`, the passthrough policy /
//! google-oauth configs, the empty indices) already ships in the binary
//! as a production fallback, so it adds no real surface.

use std::sync::Arc;

use crate::{AppState, AuthConfig, CanonicalHost, MarketingIndex, SessionStore, WorkshopIndex};

/// The session signing key the test builder uses. Stable so encoded
/// cookies round-trip across a test's requests.
pub const TEST_SESSION_KEY: &str = "test-session-key-not-for-production";

/// Build an [`AppState`] wired with dev/test defaults: in-memory runtimes,
/// passthrough policy + google-oauth, stub signature + billing providers,
/// a capturing email backend, filesystem storage in a temp dir, and empty
/// content indices. The caller supplies the `db` (one schema per test via
/// `store::test_support::pg`) and overrides any field through struct
/// update syntax — see the module docs.
#[doc(hidden)]
pub async fn app_state(db: store::Db) -> AppState {
    AppState {
        db,
        workshops: WorkshopIndex::empty(),
        docs: crate::DocsIndex::empty(),
        marketing: MarketingIndex::empty(),
        blog: crate::BlogIndex::empty(),
        auth: AuthConfig::new(true, None),
        google_oauth: crate::google_oauth::GoogleOauthConfig::passthrough(),
        rate_limit: crate::rate_limit::RateLimit::disabled(),
        canonical_host: CanonicalHost::new(None),
        portal_only: crate::PortalOnly::default(),
        sessions: SessionStore::new(TEST_SESSION_KEY),
        oauth: None,
        storage: Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join("navigator-web-test-storage"))
                .await
                .unwrap(),
        ),
        policy: crate::policy::PolicyClient::passthrough(),
        workflow_runtime: Arc::new(workflows::InMemoryRuntime::new()),
        questionnaire_runtime: Arc::new(workflows::InMemoryRuntime::new()),
        signature_provider: Arc::new(crate::signature::StubSignatureProvider::new()),
        billing_provider: Arc::new(crate::billing::StubBillingProvider::new()),
        contract_reviewer: Arc::new(crate::contract_review::StubContractReviewer),
        esignature_webhook_secret: None,
        esignature_hmac_key: None,
        email: Arc::new(crate::email::CapturingEmail::new()),
        inbound_email_secret: None,
        email_events_secret: None,
        sendgrid_events_public_key: None,
        bootstrap_admin_email: None,
        identity_password: None,
        identity_admin: None,
        a2a_router: None,
    }
}

// --- OIDC id_token test crypto -------------------------------------------
//
// The OIDC redirect callback now does full RS256 signature + `iss`/`aud`/
// `exp` + `nonce` verification, so integration tests can no longer mint an
// unsigned token. These helpers sign a real id_token with a throwaway test
// keypair and build the matching [`IdTokenVerifier`], so every callback
// test exercises the production verification path. The keypair is a
// generated 2048-bit RSA pair, never used against a real IdP.

use jsonwebtoken::{encode, Algorithm, DecodingKey, EncodingKey, Header};

use crate::oauth::{IdTokenVerifier, OAuthConfig};

/// Issuer the test verifier pins and the test tokens claim.
pub const TEST_OIDC_ISSUER: &str = "https://idp.test";
/// `kid` on the test signing key and in signed tokens' headers.
pub const TEST_OIDC_KID: &str = "test-oidc-key-1";

const TEST_OIDC_PRIV_PEM: &str = r"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCahp278eYjAS3G
gqLwL3yKvtJwn26QehDYt84GqA58FkEAR202VZbUVkSCKa8HG30Lsy5BN7/CoP1o
7wl6rr+AV4sf18A1O5k7u6FGrBMSozgydmIYbAgKITuvc2Dm9EU707fmOQEdICuH
gyIBz+Am5P8g7BUPIVic7l7ghRNifo7rWH4u8aWlZIxARzDammTRZp844pnDG0DN
GsGE8DIiYTqlErsOxuNWIr4fPREGPJzGSyCjiURCtDfBbcr1FiITf8kB/UXJUaYw
ttToClGzW2jk4UE0QLeMhYXDRjGVqcTMhDzyYXL5riSWQ8vKHXYnFBFLzJMGTexJ
RbOtlNQvAgMBAAECggEACjKAUz2gicZ9+P/Nn9sKYB+SmeheLqjs1q2z1LWfaxSO
3+VWxtikFklxG5kuRIz4Vgl82m9C4iWnQ2xO1v/pgZ8v/lR0Xy7v1Zoeskq7DCZQ
Qug+tfeJxPKyJ8m4kdUkgnuzbZJtHo5tFkloOPAOYz1bvBZIQieEW6rRVltXJE81
I1q7yzRYYn4UqqlULAZLM35J2tMwAvCJt+uiVKevDzE9Y6Th/eyaZpRk4H3HFXgh
oke/iq5A8DwG+WWUYCh4wAQfZNsgx4y/61Icw4dEgM1rrWl73rXrkJeJEhxr+TQj
11yPyMhBD+wK0RSKXqsn8WyJLETcfQB8PDCgDnt9TQKBgQDQcyTK0h8f7zDk70Kw
ubmVC85WfOP6jQF6qgXGoZHOsPonlZSIbv6ocWL9ax/moQYha12/7DakKMDpKoSL
SDVcXYIrQJEtCewJ4DNX/nbTNb5Igp/mJYUBQpbmVh4F3GIfXjFHCJL13uxYqODM
Tr8oawhGbsYDEtxEzFRWpxIZ8wKBgQC9xnj8t16d+IKHW43grlJrVXlYUzNh6M+2
0YDBdCx53V9sghCQb9H/VaRtiMaFtKqueT22mXtaX2fV+nNtuSjlA862CSw6ry+o
ceWJQ/tWKAZxJJOT7jgXBPTZHv4yq+fHytu/P3dsyVIqBGlQmnuO4bGvXrIgwUyV
257X9AAP1QKBgQCIPVmkvmTdaGYam06JVzo2cjrwSDxxO8vlsk6IHn3AC+fUC23D
JliHG2TJoUR+ZmwtV5E0qVylOoWrX8C1kAJgVjWHs3GvcDa31bN5JbXgIdY2ajm8
IHWn9y/NaCfDSOFRAy1N8gqrbIIpCGe04RsLfbkw36HHzIHu7WWKJTQthQKBgQCv
cE3lAvf7fgPdcmwk68LR60C0wKXdu8Zasi8fqHB9cIOI4mzBuj4emGPbxvgQH0cy
6G5+4kDA+TYbAN+47dW6cdylOLGkxtN+G10hmrE9ot7htfigZzd/QFvCZP6GhZlO
gGDJ2rhi33KP2Wgq1cWn/0muYBK4aTqNx2x/I9jyyQKBgHOnJa898JNANFFXbDgq
6/gZwbraIG6kP9KO84UXI/+/5/skcKK4eXYybB/HzrC7AQVQdJkIyzDYNDSEsTS6
GOFZJe6RN11Wfwq853r+yFHFnUEOac78/2P3LbfEo71JV0vWJIaKJtFfYIpLgBjU
ZAUSQlrz0bVbicQo41Jgr+pA
-----END PRIVATE KEY-----
";

const TEST_OIDC_PUB_PEM: &str = r"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAmoadu/HmIwEtxoKi8C98
ir7ScJ9ukHoQ2LfOBqgOfBZBAEdtNlWW1FZEgimvBxt9C7MuQTe/wqD9aO8Jeq6/
gFeLH9fANTuZO7uhRqwTEqM4MnZiGGwICiE7r3Ng5vRFO9O35jkBHSArh4MiAc/g
JuT/IOwVDyFYnO5e4IUTYn6O61h+LvGlpWSMQEcw2ppk0WafOOKZwxtAzRrBhPAy
ImE6pRK7DsbjViK+Hz0RBjycxksgo4lEQrQ3wW3K9RYiE3/JAf1FyVGmMLbU6ApR
s1to5OFBNEC3jIWFw0YxlanEzIQ88mFy+a4klkPLyh12JxQRS8yTBk3sSUWzrZTU
LwIDAQAB
-----END PUBLIC KEY-----
";

/// An [`IdTokenVerifier`] over the test keypair, pinned to
/// [`TEST_OIDC_ISSUER`] and the given `audience` (the OAuth `client_id`).
#[must_use]
pub fn oidc_verifier(audience: &str) -> IdTokenVerifier {
    let key = DecodingKey::from_rsa_pem(TEST_OIDC_PUB_PEM.as_bytes())
        .expect("test OIDC public key parses");
    IdTokenVerifier::from_keys(
        vec![(TEST_OIDC_KID.to_string(), key)],
        TEST_OIDC_ISSUER,
        audience,
    )
}

/// Wrap an [`OAuthConfig`] with a test id_token verifier pinned to
/// `client_id` so the callback's verification path is exercised end to end.
#[must_use]
pub fn oauth_config_with_verifier(cfg: OAuthConfig, client_id: &str) -> OAuthConfig {
    cfg.with_id_token_verifier(oidc_verifier(client_id))
}

#[derive(serde::Serialize)]
struct TestIdTokenClaims<'a> {
    sub: &'a str,
    email: &'a str,
    name: &'a str,
    nonce: &'a str,
    iss: &'a str,
    aud: &'a str,
    exp: i64,
}

/// Sign a valid RS256 id_token with the test key. `aud` must equal the
/// `client_id` the verifier is pinned to and `nonce` must match the
/// login's pre-auth nonce, or [`IdTokenVerifier::verify`] rejects it.
#[must_use]
pub fn sign_id_token(aud: &str, nonce: &str, sub: &str, email: &str, name: &str) -> String {
    let claims = TestIdTokenClaims {
        sub,
        email,
        name,
        nonce,
        iss: TEST_OIDC_ISSUER,
        aud,
        exp: crate::session::now_unix_secs() + 300,
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(TEST_OIDC_KID.to_string());
    let key = EncodingKey::from_rsa_pem(TEST_OIDC_PRIV_PEM.as_bytes())
        .expect("test OIDC private key parses");
    encode(&header, &claims, &key).expect("sign test id_token")
}
