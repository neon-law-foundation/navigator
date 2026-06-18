//! Read the installed-app OAuth client config from disk.
//!
//! The file at `~/.config/navigator/oauth_client.json` is the
//! standard "OAuth 2.0 Client ID — Desktop app" JSON download from
//! the GCP Console. It looks like:
//!
//! ```json
//! { "installed": {
//!     "client_id": "…apps.googleusercontent.com",
//!     "client_secret": "GOCSPX-…",
//!     "token_uri": "https://oauth2.googleapis.com/token",
//!     "auth_uri":  "https://accounts.google.com/o/oauth2/auth",
//!     "redirect_uris": ["http://localhost"]
//! }}
//! ```
//!
//! This file is a credential. It must never be committed; `cli drive
//! login` (commit 4) reads it to perform the consent flow.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::DriveError;

/// Subset of the installed-app client config Navigator needs.
#[derive(Debug, Clone, Deserialize)]
pub struct OauthClientConfig {
    /// OAuth 2.0 client id (the `…apps.googleusercontent.com` form).
    pub client_id: String,
    /// OAuth 2.0 client secret. Not a true secret for installed
    /// apps (it ships with the binary in many ecosystems), but we
    /// still keep it out of source control.
    pub client_secret: String,
    /// Token endpoint Google routes refresh-token grants through.
    pub token_uri: String,
    /// User-consent endpoint the `cli drive login` flow opens in
    /// the browser.
    pub auth_uri: String,
    /// Allowed redirect URIs. For installed apps this is
    /// typically `["http://localhost"]`.
    pub redirect_uris: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OnDiskRoot {
    installed: Option<OnDiskInstalled>,
}

#[derive(Debug, Deserialize)]
struct OnDiskInstalled {
    client_id: String,
    client_secret: String,
    token_uri: String,
    auth_uri: String,
    redirect_uris: Vec<String>,
}

/// Default on-disk location for the client config.
#[must_use]
pub fn default_oauth_client_path() -> PathBuf {
    config_dir().join("oauth_client.json")
}

/// `~/.config/navigator/` (overridable via `NAVIGATOR_CONFIG_DIR`
/// for tests).
fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NAVIGATOR_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join("navigator")
}

/// Load and parse the installed-app client config at `path`.
pub fn load_oauth_client(path: &Path) -> Result<OauthClientConfig, DriveError> {
    let bytes = std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            DriveError::MissingConfig(format!(
                "oauth client config not found at {} — download \
                 'OAuth 2.0 Client ID (Desktop)' from the GCP Console \
                 and save it there",
                path.display()
            ))
        } else {
            DriveError::Io(e)
        }
    })?;
    let root: OnDiskRoot = serde_json::from_slice(&bytes)?;
    let installed = root.installed.ok_or_else(|| {
        DriveError::InvalidConfig(format!(
            "{} is missing the top-level `installed` block — \
             is this a 'Web' rather than a 'Desktop' OAuth client?",
            path.display()
        ))
    })?;
    Ok(OauthClientConfig {
        client_id: installed.client_id,
        client_secret: installed.client_secret,
        token_uri: installed.token_uri,
        auth_uri: installed.auth_uri,
        redirect_uris: installed.redirect_uris,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_well_formed_installed_client() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oauth_client.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(
            br#"{"installed":{
                  "client_id":"cid.apps.googleusercontent.com",
                  "client_secret":"GOCSPX-xyz",
                  "token_uri":"https://oauth2.googleapis.com/token",
                  "auth_uri":"https://accounts.google.com/o/oauth2/auth",
                  "redirect_uris":["http://localhost"]
                }}"#,
        )
        .unwrap();

        let cfg = load_oauth_client(&path).unwrap();
        assert_eq!(cfg.client_id, "cid.apps.googleusercontent.com");
        assert_eq!(cfg.client_secret, "GOCSPX-xyz");
        assert_eq!(cfg.redirect_uris, vec!["http://localhost".to_string()]);
    }

    #[test]
    fn missing_file_is_a_friendly_error() {
        let path = std::path::Path::new("/tmp/this-file-does-not-exist-navigator-test.json");
        let err = load_oauth_client(path).unwrap_err();
        assert!(matches!(err, DriveError::MissingConfig(_)), "{err:?}");
    }

    #[test]
    fn missing_installed_block_is_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oauth_client.json");
        std::fs::write(&path, br#"{"web":{"client_id":"x"}}"#).unwrap();
        let err = load_oauth_client(&path).unwrap_err();
        assert!(matches!(err, DriveError::InvalidConfig(_)), "{err:?}");
    }

    #[test]
    fn default_path_respects_override_env() {
        let dir = tempfile::tempdir().unwrap();
        // Save/restore env around the assertion to avoid polluting
        // sibling tests.
        let prev = std::env::var("NAVIGATOR_CONFIG_DIR").ok();
        std::env::set_var("NAVIGATOR_CONFIG_DIR", dir.path());
        let p = default_oauth_client_path();
        assert!(p.starts_with(dir.path()));
        assert!(p.ends_with("oauth_client.json"));
        match prev {
            Some(v) => std::env::set_var("NAVIGATOR_CONFIG_DIR", v),
            None => std::env::remove_var("NAVIGATOR_CONFIG_DIR"),
        }
    }
}
