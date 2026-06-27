// Zed extension that registers `navigator-lsp` for Markdown buffers.
//
// The server binary is resolved most-specific first:
//   1. an explicit user override in Zed settings —
//        "lsp": { "navigator-lsp": { "binary": { "path": "...", "arguments": [...] } } }
//   2. else a `navigator-lsp` already on the worktree's PATH (`cargo install
//      --path lsp`, or any copy on $PATH);
//   3. else the matching prebuilt binary downloaded from the latest
//      `neon-law-foundation/navigator` GitHub Release and cached per version,
//      so a marketplace install is self-contained — the user installs the
//      extension and gets the official LSP with no extra steps.
//
// Built with `zed_extension_api`; targets `wasm32-wasip2`.

use zed_extension_api::{
    self as zed, current_platform, download_file, latest_github_release, make_file_executable,
    set_language_server_installation_status, settings::LspSettings, Architecture, Command,
    DownloadedFileType, GithubReleaseOptions, LanguageServerId,
    LanguageServerInstallationStatus as Status, Os, Result, Worktree,
};

const SERVER_NAME: &str = "navigator-lsp";

/// The public repository whose Releases carry the prebuilt `navigator-lsp`
/// binaries (`navigator-lsp-<tag>-<platform>.tar.gz` / `.zip`, attached by
/// `deploy.yml`). Forks point this at their own repository.
const RELEASE_REPO: &str = "neon-law-foundation/navigator";

struct NavigatorLsp {
    /// Cached path to the downloaded binary, so a re-resolve within one Zed
    /// session doesn't re-hit GitHub once a version is on disk.
    cached_binary_path: Option<String>,
}

impl NavigatorLsp {
    /// Resolve the server binary, downloading the matching Release asset when
    /// it isn't already present.
    fn download_binary(&mut self, language_server_id: &LanguageServerId) -> Result<String> {
        // The platform → Release-asset mapping. Today the Release ships one
        // arch per OS (Apple Silicon macOS, x86-64 Linux, x86-64 Windows);
        // other arches (Intel Mac, ARM Linux) get a clear error until those
        // cross-builds are attached. `platform_token` matches deploy.yml's
        // `navigator-lsp-<tag>-<token>.<ext>` asset name.
        let (os, arch) = current_platform();
        let (platform_token, archive_ext, file_type, binary_leaf) = match (os, arch) {
            (Os::Mac, Architecture::Aarch64) => {
                ("macos", "tar.gz", DownloadedFileType::GzipTar, SERVER_NAME)
            }
            (Os::Linux, Architecture::X8664) => {
                ("linux", "tar.gz", DownloadedFileType::GzipTar, SERVER_NAME)
            }
            (Os::Windows, Architecture::X8664) => (
                "windows",
                "zip",
                DownloadedFileType::Zip,
                "navigator-lsp.exe",
            ),
            _ => {
                return Err(format!(
                    "navigator-lsp has no prebuilt binary for this platform yet ({os:?}/{arch:?}). \
                     Install it on your $PATH (`cargo install --path lsp`) or set \
                     lsp.navigator-lsp.binary.path in your Zed settings."
                ));
            }
        };

        set_language_server_installation_status(language_server_id, &Status::CheckingForUpdate);
        let release = latest_github_release(
            RELEASE_REPO,
            GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let asset_name = format!(
            "navigator-lsp-{}-{platform_token}.{archive_ext}",
            release.version
        );
        let version_dir = format!("navigator-lsp-{}", release.version);
        let binary_path = format!("{version_dir}/{binary_leaf}");

        // Already downloaded for this release — reuse it, so a re-resolve after
        // a Zed restart doesn't re-fetch.
        if std::fs::metadata(&binary_path).is_ok_and(|stat| stat.is_file()) {
            self.cached_binary_path = Some(binary_path.clone());
            set_language_server_installation_status(language_server_id, &Status::None);
            return Ok(binary_path);
        }

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| format!("no `{asset_name}` asset on the {} release", release.version))?;

        set_language_server_installation_status(language_server_id, &Status::Downloading);
        // The archive holds a single `navigator-lsp[.exe]`; extract it into the
        // per-version directory, then mark it runnable.
        download_file(&asset.download_url, &version_dir, file_type)
            .map_err(|err| format!("failed to download {asset_name}: {err}"))?;
        make_file_executable(&binary_path)
            .map_err(|err| format!("failed to make {binary_path} executable: {err}"))?;

        // Prune older version directories so the cache doesn't grow unbounded.
        if let Ok(entries) = std::fs::read_dir(".") {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("navigator-lsp-") && name != version_dir {
                    std::fs::remove_dir_all(entry.path()).ok();
                }
            }
        }

        self.cached_binary_path = Some(binary_path.clone());
        set_language_server_installation_status(language_server_id, &Status::None);
        Ok(binary_path)
    }
}

impl zed::Extension for NavigatorLsp {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        // A user `binary` override (path + optional arguments) wins over every
        // form of discovery. Missing settings are not an error — they just mean
        // "fall back to PATH, then the downloaded binary."
        let lsp_settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree).ok();
        let binary = lsp_settings.and_then(|settings| settings.binary);
        let user_args = binary
            .as_ref()
            .and_then(|b| b.arguments.clone())
            .unwrap_or_default();

        let command = if let Some(path) = binary.as_ref().and_then(|b| b.path.clone()) {
            path
        } else if let Some(path) = worktree.which(SERVER_NAME) {
            // A `navigator-lsp` already on PATH (contributors, `cargo install`)
            // wins over a download — no network, and it tracks their checkout.
            path
        } else if let Some(path) = self.cached_binary_path.clone() {
            path
        } else {
            self.download_binary(language_server_id)?
        };

        Ok(Command {
            command,
            args: user_args,
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(NavigatorLsp);
