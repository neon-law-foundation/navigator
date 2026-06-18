//! White-label "portal-only" mode.
//!
//! When `NAVIGATOR_PORTAL_ONLY` is set (`true` / `1`), `web` mounts only
//! the *application* surface — `/portal`, auth, the JSON `/api`, `/mcp`,
//! the git transport, webhooks, the health probes, and the legal pages —
//! and drops the entire public marketing + Foundation surface (the firm
//! home page, `/services/*`, `/about`, `/contact`, `/foundation/*`,
//! `/navigator`, the workshops, presentations, statutes, docs, and the
//! `/es` twins). The bare host `/` 303-redirects to `/portal`.
//!
//! The use case is a law firm that deploys Navigator under its own brand:
//! it already runs its own marketing website (WordPress, a marketing
//! team) and only wants Navigator to be the client portal + workflow
//! engine, not a second public site. See `docs/oss-install.md` and the
//! "Deploy the Navigator" workshop.
//!
//! Disabled by default — NeonLaw's own deploy serves the full public
//! site, so the flag ships off and the router is unchanged unless it is
//! lit. Portal-only decides *whether the public pages exist at all*.

/// Env-driven toggle for portal-only mode. `Copy` so [`crate::AppState`]
/// can hand it to [`crate::build_router`] without a clone.
#[derive(Debug, Clone, Copy, Default)]
pub struct PortalOnly(bool);

impl PortalOnly {
    /// Read `NAVIGATOR_PORTAL_ONLY`. Enabled on `true` / `1`; any other
    /// value (or unset) leaves the full marketing site mounted.
    #[must_use]
    pub fn from_env() -> Self {
        Self(matches!(
            std::env::var("NAVIGATOR_PORTAL_ONLY").ok().as_deref(),
            Some("true" | "1")
        ))
    }

    /// Construct explicitly (tests build the router both ways without
    /// stomping the process env).
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self(enabled)
    }

    /// True when the marketing surface should be suppressed.
    #[must_use]
    pub fn enabled(self) -> bool {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::PortalOnly;

    #[test]
    fn default_is_disabled() {
        assert!(!PortalOnly::default().enabled());
    }

    #[test]
    fn new_round_trips() {
        assert!(PortalOnly::new(true).enabled());
        assert!(!PortalOnly::new(false).enabled());
    }
}
