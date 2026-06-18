//! The acting principal for an MCP call.
//!
//! Defined in `mcp` (not `web`) so the dispatcher and the tools
//! can mention it without dragging in a web-stack dependency.
//! The `web` crate is responsible for actually populating the
//! [`Principal`] from whatever auth path it owns (Google OAuth
//! access tokens via `web::google_oauth`, JWTs via
//! `web::auth::require_auth`, IAP headers, etc.) — `mcp` only
//! reads it back out of request extensions.

/// The authenticated email behind an MCP call. Trusted: the value
/// has already been validated by the upstream auth middleware
/// (`require_google_oauth` against Google's `tokeninfo`, or
/// `require_auth` against a signed JWT).
///
/// Tools that mutate data must trust this over any caller-supplied
/// `email`-style argument: in production the LLM doesn't know
/// who's signed in, only the auth layer does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub email: String,
}

impl Principal {
    #[must_use]
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
        }
    }
}
