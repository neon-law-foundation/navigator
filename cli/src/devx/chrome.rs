//! Chrome for Testing resolver — the single source of the pinned
//! browser build the browser/accessibility e2e suites run against.
//!
//! `.github/workflows/deploy.yml`'s `integration` job downloads a
//! **pinned** Chrome for Testing + matching chromedriver so CI never
//! drifts onto whatever Chrome a runner happens to ship. A dev box,
//! by contrast, drove whatever system Chrome was installed — so a box
//! on Chrome 149 hit `SessionNotCreated: only supports Chrome version
//! 150` and the e2e gate could not be reproduced locally without
//! hand-downloading the exact build.
//!
//! This module makes the CLI resolve the *same* pinned build CI uses:
//! [`CHROME_FOR_TESTING_VERSION`] is the one constant both sides read
//! (a guard test asserts `deploy.yml` names the same version, so the
//! two can't drift), and [`resolve`] downloads + caches that build for
//! the host architecture, returning the two binary paths the
//! chromedriver harness needs.
//!
//! The download shells out to `curl` + `unzip`, faithful to the
//! deploy.yml step it mirrors; the cache is keyed by version, so a
//! second worktree reuses the first's download instead of re-fetching
//! ~150 MB.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use super::{require_tools, run};

/// The pinned Chrome for Testing build. This is the **single source**
/// the CLI and `.github/workflows/deploy.yml` both read — bump it here
/// and the guard test (`deploy_yml_pins_the_same_chrome_version`) fails
/// until `deploy.yml`'s `CHROME_FOR_TESTING_VERSION` is bumped to match.
pub const CHROME_FOR_TESTING_VERSION: &str = "150.0.7871.46";

/// The two Chrome for Testing artifacts we download per version: the
/// browser and its exact-match chromedriver.
#[derive(Clone, Copy)]
enum Artifact {
    Chrome,
    Chromedriver,
}

impl Artifact {
    /// The `chrome` / `chromedriver` stem used in both the zip name
    /// (`{stem}-{platform}.zip`) and the extracted directory
    /// (`{stem}-{platform}/`).
    fn stem(self) -> &'static str {
        match self {
            Artifact::Chrome => "chrome",
            Artifact::Chromedriver => "chromedriver",
        }
    }
}

/// A resolved, on-disk Chrome for Testing pair.
pub struct ChromeForTesting {
    /// The Chrome executable (`CHROME_BINARY` for the harness).
    pub chrome: PathBuf,
    /// The exact-match chromedriver executable.
    pub chromedriver: PathBuf,
}

/// Map a Rust `(OS, ARCH)` pair to the Chrome for Testing platform
/// slug that names its download. Pure so it is unit-tested against the
/// slugs Google publishes; the host wrapper ([`host_platform`]) feeds
/// it `std::env::consts`.
fn platform_for(os: &str, arch: &str) -> Result<&'static str> {
    Ok(match (os, arch) {
        ("linux", "x86_64") => "linux64",
        ("macos", "aarch64") => "mac-arm64",
        ("macos", "x86_64") => "mac-x64",
        _ => bail!("no Chrome for Testing build for os={os:?} arch={arch:?}"),
    })
}

/// The Chrome for Testing platform slug for the host running this CLI.
fn host_platform() -> Result<&'static str> {
    platform_for(env::consts::OS, env::consts::ARCH)
}

/// The public download URL for one artifact of a pinned version, e.g.
/// `…/chrome-for-testing-public/150.0.7871.46/mac-arm64/chrome-mac-arm64.zip`.
/// Pure — unit-tested to match the layout `deploy.yml` hard-codes.
fn artifact_url(version: &str, platform: &str, artifact: Artifact) -> String {
    let stem = artifact.stem();
    format!(
        "https://storage.googleapis.com/chrome-for-testing-public/\
         {version}/{platform}/{stem}-{platform}.zip"
    )
}

/// The extracted Chrome executable path for a platform. macOS ships the
/// browser inside an `.app` bundle; linux ships a bare `chrome`. Pure so
/// both layouts are pinned by test.
fn chrome_binary_path(root: &Path, platform: &str) -> PathBuf {
    let dir = root.join(format!("chrome-{platform}"));
    match platform {
        "mac-arm64" | "mac-x64" => dir
            .join("Google Chrome for Testing.app")
            .join("Contents")
            .join("MacOS")
            .join("Google Chrome for Testing"),
        _ => dir.join("chrome"),
    }
}

/// The extracted chromedriver executable path for a platform. Uniform
/// across platforms (`chromedriver-{platform}/chromedriver`).
fn chromedriver_binary_path(root: &Path, platform: &str) -> PathBuf {
    root.join(format!("chromedriver-{platform}"))
        .join("chromedriver")
}

/// The version-keyed cache directory. Overridable with
/// `NAVIGATOR_CHROME_CACHE_DIR` (CI points it at the runner tool cache);
/// otherwise `~/.cache/navigator/chrome-for-testing`, shared across
/// worktrees so the ~150 MB build downloads once.
fn cache_root() -> Result<PathBuf> {
    if let Ok(dir) = env::var("NAVIGATOR_CHROME_CACHE_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let home = env::var("HOME").context("HOME is unset — set NAVIGATOR_CHROME_CACHE_DIR")?;
    Ok(PathBuf::from(home)
        .join(".cache")
        .join("navigator")
        .join("chrome-for-testing"))
}

/// Resolve the pinned Chrome for Testing pair for the host, downloading
/// and caching it on first use. Returns the two binary paths the
/// chromedriver harness needs (`CHROME_BINARY` + the driver to launch).
pub fn resolve() -> Result<ChromeForTesting> {
    let platform = host_platform()?;
    let version_root = cache_root()?.join(CHROME_FOR_TESTING_VERSION);
    let chrome = chrome_binary_path(&version_root, platform);
    let chromedriver = chromedriver_binary_path(&version_root, platform);

    if chrome.is_file() && chromedriver.is_file() {
        eprintln!(
            "=== Chrome for Testing {CHROME_FOR_TESTING_VERSION} ({platform}) cached at {} ===",
            version_root.display()
        );
        return Ok(ChromeForTesting {
            chrome,
            chromedriver,
        });
    }

    require_tools(&["curl", "unzip"])?;
    std::fs::create_dir_all(&version_root)
        .with_context(|| format!("create cache dir {}", version_root.display()))?;

    for artifact in [Artifact::Chrome, Artifact::Chromedriver] {
        let url = artifact_url(CHROME_FOR_TESTING_VERSION, platform, artifact);
        eprintln!("=== downloading {url} ===");
        let zip = version_root.join(format!("{}-{platform}.zip", artifact.stem()));
        run(Command::new("curl")
            .arg("--fail")
            .arg("--show-error")
            .arg("--location")
            .arg("--output")
            .arg(&zip)
            .arg(&url))?;
        run(Command::new("unzip")
            .arg("-q")
            .arg("-o")
            .arg(&zip)
            .arg("-d")
            .arg(&version_root))?;
        std::fs::remove_file(&zip).ok();
    }

    // macOS tags `curl`-downloaded binaries with `com.apple.quarantine`,
    // which makes Gatekeeper refuse to launch the extracted chromedriver
    // / Chrome. Strip it best-effort (no-op on Linux, where CI runs and
    // there is no quarantine). Failure here is not fatal — the launch
    // below surfaces any real problem.
    if env::consts::OS == "macos" {
        let _ = Command::new("xattr")
            .arg("-dr")
            .arg("com.apple.quarantine")
            .arg(&version_root)
            .status();
    }

    if !chrome.is_file() || !chromedriver.is_file() {
        bail!(
            "Chrome for Testing extraction did not produce the expected binaries \
             ({} / {})",
            chrome.display(),
            chromedriver.display()
        );
    }
    Ok(ChromeForTesting {
        chrome,
        chromedriver,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_slugs_match_google_published_names() {
        assert_eq!(platform_for("linux", "x86_64").unwrap(), "linux64");
        assert_eq!(platform_for("macos", "aarch64").unwrap(), "mac-arm64");
        assert_eq!(platform_for("macos", "x86_64").unwrap(), "mac-x64");
        assert!(platform_for("windows", "x86_64").is_err());
        assert!(platform_for("linux", "aarch64").is_err());
    }

    #[test]
    fn artifact_url_matches_the_deploy_yml_layout() {
        assert_eq!(
            artifact_url("150.0.7871.46", "linux64", Artifact::Chrome),
            "https://storage.googleapis.com/chrome-for-testing-public/\
             150.0.7871.46/linux64/chrome-linux64.zip"
        );
        assert_eq!(
            artifact_url("150.0.7871.46", "linux64", Artifact::Chromedriver),
            "https://storage.googleapis.com/chrome-for-testing-public/\
             150.0.7871.46/linux64/chromedriver-linux64.zip"
        );
        assert_eq!(
            artifact_url("150.0.7871.46", "mac-arm64", Artifact::Chrome),
            "https://storage.googleapis.com/chrome-for-testing-public/\
             150.0.7871.46/mac-arm64/chrome-mac-arm64.zip"
        );
    }

    #[test]
    fn binary_paths_track_the_platform_layout() {
        let root = Path::new("/cache/150.0.7871.46");
        assert_eq!(
            chrome_binary_path(root, "linux64"),
            Path::new("/cache/150.0.7871.46/chrome-linux64/chrome")
        );
        assert_eq!(
            chrome_binary_path(root, "mac-arm64"),
            Path::new(
                "/cache/150.0.7871.46/chrome-mac-arm64/\
                 Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
            )
        );
        assert_eq!(
            chromedriver_binary_path(root, "mac-arm64"),
            Path::new("/cache/150.0.7871.46/chromedriver-mac-arm64/chromedriver")
        );
    }

    /// The pin is single-sourced: `deploy.yml` must name the exact
    /// version this constant does, or CI and local drift apart — the
    /// gap #370 exists to close. Reads the workflow relative to this
    /// crate so it runs in-tree.
    #[test]
    fn deploy_yml_pins_the_same_chrome_version() {
        let deploy_yml = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".github")
            .join("workflows")
            .join("deploy.yml");
        let body = std::fs::read_to_string(&deploy_yml)
            .unwrap_or_else(|e| panic!("read {}: {e}", deploy_yml.display()));
        let needle = format!("CHROME_FOR_TESTING_VERSION: {CHROME_FOR_TESTING_VERSION}");
        assert!(
            body.contains(&needle),
            "deploy.yml must pin `{needle}` to stay in lockstep with the CLI constant"
        );
    }
}
