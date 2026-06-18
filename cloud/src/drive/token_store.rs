//! Read and write the persisted Drive refresh token.
//!
//! The file at `~/.config/navigator/drive_token.json` holds the
//! refresh token minted by `cli drive login` (commit 4). It is
//! written with file mode `0o600` on Unix — owner read+write, no
//! group/other access. Scorpio's red line from the design council.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::DriveError;

/// On-disk representation of the persisted refresh token. The only
/// field we strictly require is `refresh_token`; everything else is
/// metadata we'd like to surface in errors and dashboards but won't
/// fail on if it's absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveToken {
    /// The long-lived refresh token. Trade this at the OAuth token
    /// endpoint for short-lived access tokens.
    pub refresh_token: String,
    /// Email address (or `sub`) of the Google account this token
    /// was minted for. Informational only — Drive bearers don't
    /// carry user identity in any header.
    #[serde(default)]
    pub account: Option<String>,
    /// Space-separated scopes the user consented to. Recorded so a
    /// future `cli drive doctor` command can tell the user their
    /// token is too narrow for what they're trying to do.
    #[serde(default)]
    pub scope: Option<String>,
    /// RFC 3339 timestamp this token was minted. Lets us tell the
    /// user when they last consented.
    #[serde(default)]
    pub minted_at: Option<String>,
}

/// Default on-disk location for the persisted token.
#[must_use]
pub fn default_drive_token_path() -> PathBuf {
    config_dir().join("drive_token.json")
}

fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NAVIGATOR_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join("navigator")
}

/// Load and parse the persisted Drive token at `path`. Returns a
/// friendly error if the file is missing.
pub fn load_drive_token(path: &Path) -> Result<DriveToken, DriveError> {
    let bytes = std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            DriveError::MissingConfig(format!(
                "drive token not found at {} — run `cli drive login` to mint one",
                path.display()
            ))
        } else {
            DriveError::Io(e)
        }
    })?;
    let token: DriveToken = serde_json::from_slice(&bytes)?;
    if token.refresh_token.trim().is_empty() {
        return Err(DriveError::InvalidConfig(format!(
            "{} has an empty refresh_token",
            path.display()
        )));
    }
    Ok(token)
}

/// Persist `token` to `path` with `0o600` permissions on Unix.
/// Creates the parent directory if needed.
pub fn save_drive_token(path: &Path, token: &DriveToken) -> Result<(), DriveError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(token)?;
    write_with_mode_0600(path, &bytes)?;
    Ok(())
}

#[cfg(unix)]
fn write_with_mode_0600(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    std::io::Write::write_all(&mut file, bytes)
}

#[cfg(not(unix))]
fn write_with_mode_0600(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    // No POSIX mode bits on non-Unix; ACLs are the platform's job.
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_refresh_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drive_token.json");
        let token = DriveToken {
            refresh_token: "1//abc".into(),
            account: Some("northstar@navigator.dev".into()),
            scope: Some("https://www.googleapis.com/auth/drive.readonly".into()),
            minted_at: Some("2026-05-26T00:00:00Z".into()),
        };
        save_drive_token(&path, &token).unwrap();
        let back = load_drive_token(&path).unwrap();
        assert_eq!(back.refresh_token, "1//abc");
        assert_eq!(back.account.as_deref(), Some("northstar@navigator.dev"));
    }

    #[cfg(unix)]
    #[test]
    fn save_sets_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drive_token.json");
        let token = DriveToken {
            refresh_token: "1//abc".into(),
            account: None,
            scope: None,
            minted_at: None,
        };
        save_drive_token(&path, &token).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
    }

    #[test]
    fn missing_file_is_a_friendly_error() {
        let p = std::path::Path::new("/tmp/no-such-navigator-drive-token-test.json");
        let err = load_drive_token(p).unwrap_err();
        assert!(matches!(err, DriveError::MissingConfig(_)), "{err:?}");
    }

    #[test]
    fn empty_refresh_token_is_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drive_token.json");
        std::fs::write(&path, br#"{"refresh_token":""}"#).unwrap();
        let err = load_drive_token(&path).unwrap_err();
        assert!(matches!(err, DriveError::InvalidConfig(_)), "{err:?}");
    }

    #[test]
    fn save_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/sub/drive_token.json");
        let token = DriveToken {
            refresh_token: "x".into(),
            account: None,
            scope: None,
            minted_at: None,
        };
        save_drive_token(&path, &token).unwrap();
        assert!(path.exists());
    }
}
