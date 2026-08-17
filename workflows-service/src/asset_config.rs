//! Startup validation of the deployment asset origin for the worker.
//!
//! The email `@font-face` block renders in the `workflows` crate under
//! **this** binary, not `web`, so a `web`-only boot check would leave
//! outbound mail as the one un-validated reader of
//! `NAVIGATOR_ASSET_BASE_URL`. This module calls the shared
//! [`views::assets::validate_asset_base_url`] validator at worker startup
//! so a malformed origin crash-loops the pod instead of reaching the
//! email `<style>` block. Unset/blank is valid — the email layout falls
//! back to the site URL for its webfont origin.

use views::assets::{validate_asset_base_url, AssetBaseUrlError};

/// Env var carrying the public asset origin. Mirrors the key the email
/// layout (`workflows::email::layout`) and `web` read.
const ASSET_BASE_URL_ENV: &str = "NAVIGATOR_ASSET_BASE_URL";

/// Validate `NAVIGATOR_ASSET_BASE_URL` from a `key -> Option<value>`
/// lookup. A missing key is valid (the email layout falls back to the
/// site URL); a present value must satisfy
/// [`views::assets::validate_asset_base_url`].
///
/// # Errors
///
/// Returns [`AssetBaseUrlError`] when the configured value is present and
/// malformed.
pub fn validate_for_deployment<F: Fn(&str) -> Option<String>>(
    get: F,
) -> Result<(), AssetBaseUrlError> {
    match get(ASSET_BASE_URL_ENV) {
        Some(value) => validate_asset_base_url(&value),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::validate_for_deployment;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[test]
    fn unset_is_valid() {
        // The email layout falls back to the site URL when the asset origin
        // is unset, so a worker with no asset base URL is a legitimate deploy.
        assert!(validate_for_deployment(lookup(&[])).is_ok());
    }

    #[test]
    fn a_well_formed_absolute_origin_is_valid() {
        assert!(validate_for_deployment(lookup(&[(
            "NAVIGATOR_ASSET_BASE_URL",
            "https://storage.example.test/navigator-assets",
        )]))
        .is_ok());
    }

    #[test]
    fn a_style_breakout_is_rejected() {
        // The `</style>` breakout that would otherwise reach the email
        // `<style>` block must crash the worker at boot.
        assert!(validate_for_deployment(lookup(&[(
            "NAVIGATOR_ASSET_BASE_URL",
            "https://evil.test/x'</style><script>",
        )]))
        .is_err());
    }
}
