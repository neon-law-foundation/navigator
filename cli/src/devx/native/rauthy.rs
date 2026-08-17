//! Acquire the Rauthy identity provider for the native tier.
//!
//! Rauthy is the one dependency Homebrew cannot supply, and the reason
//! is not that a formula is merely missing: Rauthy publishes **no
//! binaries at all**. Every GitHub release carries zero assets, and the
//! server is not on crates.io — only `rauthy-client` is. Distribution is
//! the container image, and lifting the binary out of that image is a
//! dead end on this lane's only supported host, because the image
//! carries a Linux ELF that macOS cannot execute.
//!
//! So the native tier builds it, once per version, and caches the
//! result the way [`super::super::chrome`] caches Chrome for Testing.
//!
//! The recipe is deliberately *not* the documented one. Rauthy's
//! `CONTRIBUTING` presents `just setup && just build-ui && cargo build
//! --release`, and `just setup` runs `npm install`, `cargo install
//! wasm-pack mdbook mdbook-admonish`, and a wasm build — a Node
//! toolchain this workspace does not have and does not want. Reading
//! the justfile at the pinned tag shows the shortcut: `extract-ui-archive`
//! compiles nothing. It untars `assets/static_html/static_v1.tar.gz` and
//! `templates_html.tar.gz`, both committed to Rauthy's own repository.
//! The prebuilt frontend ships in the git tree, so the whole build is a
//! shallow clone, two extractions, and `cargo build`. Verified against
//! v0.36.1: a 52 MB `arm64` Mach-O reporting `rauthy 0.36.1`.
//!
//! The checkout is deleted once the binary is copied out. It is ~1.9 GB
//! of source and build artifacts against a 52 MB result, and keeping it
//! would undo the disk saving this lane exists to deliver.
//!
//! # Starting it
//!
//! The cluster lane configures Rauthy from
//! `k8s/overlays/kind/rauthy/local-fixture.yaml`: a `config.toml`, two
//! bootstrap JSON files, and a Secret of keys and administrator
//! credentials. The native lane needs all three, so it writes the same
//! content to disk — and a test parses that fixture and asserts the
//! bootstrap payloads are byte-identical. Two lanes with two different
//! sets of seeded users is the kind of difference that only shows up as
//! a login failing in one of them.
//!
//! Three ports move. Rauthy's HTTP listener takes this worktree's slot
//! port instead of the `NodePort`, and the two embedded Hiqlite
//! listeners — Raft and API — are derived from the slot for the same
//! reason Garage's RPC port is: they are internal, nothing outside the
//! process connects to them, and at their defaults a second worktree
//! would fail to bind.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use super::supervisor::Service;

/// The Rauthy release the native tier builds.
///
/// Held equal to the image tag in `k8s/staging/rauthy.yaml` by a test —
/// the two lanes must authenticate against the same provider version or
/// an OIDC behavior difference shows up as a login bug in one lane only.
pub(super) const RAUTHY_VERSION: &str = "0.36.1";

/// Manifest whose image tag this build must match. Read only by the
/// parity test — that assertion is the constant's whole purpose.
#[cfg_attr(not(test), allow(dead_code))]
const RAUTHY_MANIFEST: &str = "k8s/staging/rauthy.yaml";

/// Rauthy's git tags carry a `v` prefix that the container tag does not.
fn release_tag(version: &str) -> String {
    format!("v{version}")
}

/// Where a built Rauthy of a given version lives.
///
/// Version-keyed, and shared across worktrees: the build happens once
/// per machine per version, not once per checkout.
fn cached_binary(root: &Path, version: &str) -> PathBuf {
    root.join(version).join("rauthy")
}

/// The cache root, overridable for CI or a scratch build.
fn cache_root() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("NAVIGATOR_RAUTHY_CACHE_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let home = std::env::var("HOME").context("HOME is unset — set NAVIGATOR_RAUTHY_CACHE_DIR")?;
    Ok(PathBuf::from(home)
        .join(".cache")
        .join("navigator")
        .join("rauthy"))
}

/// Whether `rauthy --version` output names the version we expect.
///
/// The binary is checked rather than trusted on presence alone: an
/// interrupted copy leaves a file at the right path that would otherwise
/// be reused forever, failing later and further from the cause.
fn reports_version(output: &str, version: &str) -> bool {
    output
        .split_whitespace()
        .any(|token| token.trim_start_matches('v') == version)
}

/// Resolve the pinned Rauthy binary, building it on first use.
pub(super) fn resolve() -> Result<PathBuf> {
    let binary = cached_binary(&cache_root()?, RAUTHY_VERSION);
    if is_usable(&binary) {
        eprintln!("==> Rauthy {RAUTHY_VERSION} cached at {}", binary.display());
        return Ok(binary);
    }
    build(&binary)
}

/// Whether a cached binary exists and reports the pinned version.
fn is_usable(binary: &Path) -> bool {
    binary.is_file()
        && Command::new(binary)
            .arg("--version")
            .output()
            .is_ok_and(|out| {
                out.status.success()
                    && reports_version(&String::from_utf8_lossy(&out.stdout), RAUTHY_VERSION)
            })
}

/// Clone, extract the committed UI archives, build, and cache.
fn build(binary: &Path) -> Result<PathBuf> {
    super::super::require_tools(&["git", "tar", "cargo"])?;
    let version_root = binary
        .parent()
        .context("cached Rauthy path has no parent directory")?;
    std::fs::create_dir_all(version_root)
        .with_context(|| format!("create cache dir {}", version_root.display()))?;

    // Build beside the destination rather than in a temp dir: the
    // checkout is ~1.9 GB, and a cache root the caller has already
    // pointed somewhere with room is the right place for it.
    let checkout = version_root.join("src");
    if checkout.exists() {
        std::fs::remove_dir_all(&checkout)
            .with_context(|| format!("clear stale checkout {}", checkout.display()))?;
    }

    let tag = release_tag(RAUTHY_VERSION);
    eprintln!("==> building Rauthy {RAUTHY_VERSION} from source (first run only, ~20 min)");
    run(Command::new("git").args([
        "clone",
        "--depth",
        "1",
        "--branch",
        &tag,
        "https://github.com/sebadob/rauthy.git",
        &checkout.to_string_lossy(),
    ]))?;

    // The prebuilt frontend, committed to Rauthy's repo. This is what
    // replaces `just setup` + `just build-ui` and their npm/wasm-pack
    // toolchain — see the module docs.
    std::fs::create_dir_all(checkout.join("static"))
        .context("create Rauthy's static/ directory")?;
    for (archive, into) in [
        ("assets/static_html/static_v1.tar.gz", "static/"),
        ("assets/static_html/templates_html.tar.gz", "templates/"),
    ] {
        run(Command::new("tar")
            .arg("-xf")
            .arg(checkout.join(archive))
            .arg("-C")
            .arg(&checkout)
            .arg(into))?;
    }

    run(Command::new("cargo").current_dir(&checkout).args([
        "build",
        "--release",
        "--bin",
        "rauthy",
    ]))?;

    let built = checkout.join("target").join("release").join("rauthy");
    std::fs::copy(&built, binary)
        .with_context(|| format!("cache {} → {}", built.display(), binary.display()))?;

    // Reclaim the checkout. Leaving ~1.9 GB per version behind would
    // undo the disk saving this lane exists for.
    std::fs::remove_dir_all(&checkout).ok();

    if !is_usable(binary) {
        bail!(
            "built Rauthy at {} does not report version {RAUTHY_VERSION}",
            binary.display()
        );
    }
    eprintln!("==> Rauthy {RAUTHY_VERSION} cached at {}", binary.display());
    Ok(binary.to_path_buf())
}

// ---------- starting it ----------

/// The Hiqlite Raft and API secrets, the encryption key, and the
/// bootstrap administrator.
///
/// Copied from `k8s/overlays/kind/rauthy/local-fixture.yaml` and held
/// equal to it by a test. These are safe to commit for exactly the
/// reason the fixture's header gives: every listener they protect is
/// bound to loopback on a developer's machine.
const HQL_SECRET_RAFT: &str = "ECmmHCFwIXzxwPpxbGSPNJztwtlCbcZfGRDFWfVGqqrHDnTc";
const HQL_SECRET_API: &str = "cPEyggSrcRFCRJhvYsWLircnULuZdGLbwnVVvDUTUwmVlYzT";
const ENC_KEYS: &str = "navdev/3QWXNRYth48aapMLiYBCOzICs2m/H1dzwfPu6pwykhw=";
const ENC_KEY_ACTIVE: &str = "navdev";
const BOOTSTRAP_ADMIN_EMAIL: &str = "nick@neonlaw.com";
const BOOTSTRAP_ADMIN_PASSWORD: &str = "admin";

/// The seeded local identities, byte-identical to the fixture's
/// `users.json`. One account per role — `owner@neonlaw.com`, `admin@neonlaw.com`,
/// `lawyer@neonlaw.com`, `clerk@neonlaw.com`, `client@neonlaw.com`, each with the
/// password `password` — are the credentials the local sign-in loop documents,
/// so the two lanes have to seed the same people.
const SEEDED_USERS: &str = r#"[
  {
    "email": "owner@neonlaw.com",
    "preferred_username": "owner",
    "given_name": "Olive",
    "family_name": "Owner",
    "password": { "Plain": "password" },
    "roles": ["user"],
    "enabled": true,
    "email_verified": true
  },
  {
    "email": "admin@neonlaw.com",
    "preferred_username": "admin",
    "given_name": "Ada",
    "family_name": "Admin",
    "password": { "Plain": "password" },
    "roles": ["user"],
    "enabled": true,
    "email_verified": true
  },
  {
    "email": "lawyer@neonlaw.com",
    "preferred_username": "lawyer",
    "given_name": "Lawrence",
    "family_name": "Lawyer",
    "password": { "Plain": "password" },
    "roles": ["user"],
    "enabled": true,
    "email_verified": true
  },
  {
    "email": "clerk@neonlaw.com",
    "preferred_username": "clerk",
    "given_name": "Clara",
    "family_name": "Clerk",
    "password": { "Plain": "password" },
    "roles": ["user"],
    "enabled": true,
    "email_verified": true
  },
  {
    "email": "client@neonlaw.com",
    "preferred_username": "client",
    "given_name": "Cleo",
    "family_name": "Client",
    "password": { "Plain": "password" },
    "roles": ["user"],
    "enabled": true,
    "email_verified": true
  }
]
"#;

/// The registered OIDC client, byte-identical to the fixture's
/// `clients.json`. The wildcard `http://localhost:*` redirect is what
/// lets one registration serve every worktree's `web` port.
const REGISTERED_CLIENTS: &str = r#"[
  {
    "id": "navigator-web",
    "name": "Neon Law Navigator",
    "secret": {
      "Plain": "navigatorwebsecretnavigatorwebsecretnavigatorwebsecretnavigatorw"
    },
    "redirect_uris": ["http://localhost:*"],
    "post_logout_redirect_uris": ["http://localhost:*"],
    "enabled": true,
    "flows_enabled": ["authorization_code", "refresh_token"],
    "access_token_alg": "RS256",
    "id_token_alg": "RS256",
    "auth_code_lifetime": 60,
    "access_token_lifetime": 3600,
    "scopes": ["openid", "profile", "email"],
    "default_scopes": ["openid", "profile", "email"],
    "challenges": ["S256"],
    "force_mfa": false
  }
]
"#;

fn state_dir(root: &Path) -> PathBuf {
    super::supervisor::service_dir(root, super::RAUTHY_LABEL)
}

/// Rauthy's configuration file.
///
/// The same settings the fixture's `config.toml` carries, with the
/// listener and the two Hiqlite node ports moved off their defaults —
/// see the module docs. `pub_url` is the one value Rauthy uses for
/// discovery, token issuance, and browser redirects alike, which is why
/// the cluster lane has a whole patch-and-restart dance
/// ([`super::super::align_rauthy_public_url`]) for it; on this lane it
/// is written at start.
fn config_toml(port: u16, raft_port: u16, api_port: u16, bootstrap_dir: &Path) -> String {
    format!(
        "[access]\n\
         rfc_8252_enable = true\n\
         \n\
         [bootstrap]\n\
         bootstrap_dir = '{bootstrap}'\n\
         \n\
         [cluster]\n\
         node_id = 1\n\
         nodes = ['1 localhost:{raft_port} localhost:{api_port}']\n\
         cache_storage_disk = false\n\
         \n\
         [database]\n\
         \n\
         [dev]\n\
         insecure_cookie = true\n\
         \n\
         [mfa]\n\
         admin_force_mfa = false\n\
         \n\
         [server]\n\
         scheme = 'http'\n\
         pub_url = 'localhost:{port}'\n\
         listen_address = '127.0.0.1'\n\
         port_http = {port}\n\
         \n\
         [user_registration]\n\
         enable = false\n\
         \n\
         [webauthn]\n\
         rp_id = 'localhost'\n\
         rp_origin = '{origin}'\n\
         rp_name = 'Neon Law Navigator'\n",
        bootstrap = bootstrap_dir.display(),
        origin = super::super::rauthy_origin(port),
    )
}

/// The secrets and public-URL values the cluster lane supplies through
/// the `rauthy-secrets` Secret.
///
/// `PUB_URL` and `RP_ORIGIN` restate what `config_toml` already says.
/// That is not redundancy for its own sake: the cluster's Deployment
/// reads both from the Secret, so keeping the native child on the same
/// two names means one description of "which URL is Rauthy" covers both
/// lanes.
fn environment(port: u16) -> Vec<(String, String)> {
    [
        ("HQL_SECRET_RAFT", HQL_SECRET_RAFT.to_string()),
        ("HQL_SECRET_API", HQL_SECRET_API.to_string()),
        ("ENC_KEYS", ENC_KEYS.to_string()),
        ("ENC_KEY_ACTIVE", ENC_KEY_ACTIVE.to_string()),
        ("PUB_URL", format!("localhost:{port}")),
        ("RP_ORIGIN", super::super::rauthy_origin(port)),
        ("BOOTSTRAP_ADMIN_EMAIL", BOOTSTRAP_ADMIN_EMAIL.to_string()),
        (
            "BOOTSTRAP_ADMIN_PASSWORD_PLAIN",
            BOOTSTRAP_ADMIN_PASSWORD.to_string(),
        ),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect()
}

/// Write the configuration and bootstrap payloads, then describe the
/// server as a supervised service.
///
/// The files are rewritten on every `up` rather than written once: the
/// slot port is baked into `pub_url`, and a worktree that moved slots
/// would otherwise keep issuing tokens for the port it used to own.
pub(super) fn service(root: &Path, port: u16, raft_port: u16, api_port: u16) -> Result<Service> {
    let dir = state_dir(root);
    let bootstrap = dir.join("bootstrap");
    std::fs::create_dir_all(&bootstrap)
        .with_context(|| format!("create {}", bootstrap.display()))?;
    for (name, body) in [
        ("users.json", SEEDED_USERS),
        ("clients.json", REGISTERED_CLIENTS),
    ] {
        std::fs::write(bootstrap.join(name), body)
            .with_context(|| format!("write {}", bootstrap.join(name).display()))?;
    }
    let config = dir.join("config.toml");
    std::fs::write(&config, config_toml(port, raft_port, api_port, &bootstrap))
        .with_context(|| format!("write {}", config.display()))?;

    Ok(Service {
        label: super::RAUTHY_LABEL,
        program: resolve()?,
        args: vec![
            "serve".to_string(),
            "--config-file".to_string(),
            config.display().to_string(),
        ],
        env: environment(port),
        cwd: dir,
        port,
    })
}

/// Run a command, failing with its stderr.
fn run(command: &mut Command) -> Result<()> {
    let output = command
        .output()
        .with_context(|| format!("run {}", command.get_program().display()))?;
    if !output.status.success() {
        bail!(
            "{} failed: {}",
            command.get_program().display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        cached_binary, config_toml, environment, release_tag, reports_version, ENC_KEYS,
        ENC_KEY_ACTIVE, HQL_SECRET_API, HQL_SECRET_RAFT, RAUTHY_MANIFEST, RAUTHY_VERSION,
        REGISTERED_CLIENTS, SEEDED_USERS,
    };
    use std::path::{Path, PathBuf};

    /// The KIND fixture's `ConfigMap` and Secret, parsed.
    fn fixture() -> (
        std::collections::BTreeMap<String, String>,
        std::collections::BTreeMap<String, String>,
    ) {
        let body = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("repo root is cli/'s parent")
                .join("k8s/overlays/kind/rauthy/local-fixture.yaml"),
        )
        .expect("read the KIND Rauthy fixture");

        let mut bootstrap = std::collections::BTreeMap::new();
        let mut secrets = std::collections::BTreeMap::new();
        for document in serde_yaml::Deserializer::from_str(&body) {
            let value = serde_yaml::Value::deserialize(document).expect("fixture document is YAML");
            let name = value
                .get("metadata")
                .and_then(|metadata| metadata.get("name"))
                .and_then(serde_yaml::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let entries = match name.as_str() {
                "rauthy-bootstrap" => value.get("data"),
                "rauthy-secrets" => value.get("stringData"),
                _ => None,
            };
            let Some(mapping) = entries.and_then(serde_yaml::Value::as_mapping) else {
                continue;
            };
            let into = if name == "rauthy-bootstrap" {
                &mut bootstrap
            } else {
                &mut secrets
            };
            for (key, entry) in mapping {
                if let (Some(key), Some(entry)) = (key.as_str(), entry.as_str()) {
                    into.insert(key.to_string(), entry.to_string());
                }
            }
        }
        (bootstrap, secrets)
    }

    use serde::Deserialize as _;

    /// The two lanes must seed the same people and register the same
    /// client. A user present in one lane only surfaces as a documented
    /// local credential that works on a colleague's machine and not on
    /// yours — and the client registration carries the secret
    /// `render_env_for` hands `web`.
    #[test]
    fn the_bootstrap_payloads_are_identical_to_the_cluster_fixtures() {
        let (bootstrap, _) = fixture();

        assert_eq!(
            bootstrap.get("users.json").map(String::as_str),
            Some(SEEDED_USERS),
            "the native lane seeds different users than the cluster lane"
        );
        assert_eq!(
            bootstrap.get("clients.json").map(String::as_str),
            Some(REGISTERED_CLIENTS),
            "the native lane registers a different client than the cluster lane"
        );
    }

    /// The encryption key and Hiqlite secrets are what a Rauthy data
    /// directory is written against. Two lanes with two sets of keys
    /// cannot read each other's — and, more immediately, a typo here
    /// fails at start with an opaque decryption error.
    #[test]
    fn the_keys_and_bootstrap_administrator_match_the_cluster_fixture() {
        let (_, secrets) = fixture();

        for (key, ours) in [
            ("HQL_SECRET_RAFT", HQL_SECRET_RAFT),
            ("HQL_SECRET_API", HQL_SECRET_API),
            ("ENC_KEYS", ENC_KEYS),
            ("ENC_KEY_ACTIVE", ENC_KEY_ACTIVE),
            ("BOOTSTRAP_ADMIN_EMAIL", super::BOOTSTRAP_ADMIN_EMAIL),
            (
                "BOOTSTRAP_ADMIN_PASSWORD_PLAIN",
                super::BOOTSTRAP_ADMIN_PASSWORD,
            ),
        ] {
            assert_eq!(
                secrets.get(key).map(String::as_str),
                Some(ours),
                "{key} differs between the lanes"
            );
        }
    }

    /// The registered client's secret is the one `render_env_for` writes
    /// as `OAUTH_CLIENT_SECRET`. If they drift, `web` starts, redirects
    /// to Rauthy, and fails the token exchange.
    #[test]
    fn the_registered_client_carries_the_secret_the_environment_renders() {
        assert!(
            REGISTERED_CLIENTS.contains(super::super::super::LOCAL_RAUTHY_CLIENT_SECRET),
            "the client registration does not carry the rendered client secret"
        );
        assert!(REGISTERED_CLIENTS.contains("\"id\": \"navigator-web\""));
    }

    /// `pub_url` is what Rauthy puts in its discovery document, its token
    /// issuer, and its redirects. A worktree binding its slot port while
    /// advertising another one authenticates nobody.
    #[test]
    fn every_public_url_names_the_slots_own_port() {
        let config = config_toml(20_459, 21_559, 21_659, Path::new("/checkout/bootstrap"));

        assert!(config.contains("pub_url = 'localhost:20459'"), "{config}");
        assert!(config.contains("port_http = 20459"), "{config}");
        assert!(
            config.contains("rp_origin = 'http://localhost:20459'"),
            "{config}"
        );

        let environment = environment(20_459);
        assert!(environment.contains(&("PUB_URL".into(), "localhost:20459".into())));
        assert!(environment.contains(&("RP_ORIGIN".into(), "http://localhost:20459".into())));
    }

    /// Hiqlite's Raft and API listeners are per-process. Left at the
    /// fixture's 8100/8200 they would bind whichever worktree started
    /// first, and every later one would fail with a bind error that
    /// names a port nothing in the slot table mentions.
    #[test]
    fn the_embedded_cluster_ports_are_slot_derived_rather_than_the_defaults() {
        let config = config_toml(20_459, 21_559, 21_659, Path::new("/bootstrap"));

        assert!(
            config.contains("nodes = ['1 localhost:21559 localhost:21659']"),
            "{config}"
        );
        assert!(!config.contains("8100"), "{config}");
        assert!(!config.contains("8200"), "{config}");
    }

    /// Rauthy seeds users and clients by reading this directory at
    /// start. Pointing it at the container path the fixture uses would
    /// leave the native lane with no identities at all.
    #[test]
    fn the_bootstrap_directory_is_the_one_the_worktree_wrote() {
        let config = config_toml(
            1,
            2,
            3,
            Path::new("/checkout/.devx/native/rauthy/bootstrap"),
        );

        assert!(
            config.contains("bootstrap_dir = '/checkout/.devx/native/rauthy/bootstrap'"),
            "{config}"
        );
    }

    /// Both lanes must authenticate against the same Rauthy. A version
    /// difference between the built binary and the cluster image would
    /// surface as a login bug reproducible in only one lane — the most
    /// expensive kind of drift to chase.
    #[test]
    fn the_built_version_matches_the_image_the_cluster_lane_runs() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root is cli/'s parent")
            .join(RAUTHY_MANIFEST);
        let body = std::fs::read_to_string(&manifest).expect("read the Rauthy manifest");
        let image = body
            .lines()
            .find(|line| line.trim_start().starts_with("image:"))
            .expect("the manifest declares an image");

        assert!(
            image.contains(&format!("rauthy:{RAUTHY_VERSION}")),
            "pin {RAUTHY_VERSION} but the manifest runs `{}`",
            image.trim()
        );
    }

    /// Rauthy's git tag carries a `v` its container tag does not, so the
    /// clone would 404 on the container spelling.
    #[test]
    fn the_git_tag_prefixes_the_container_tag_with_v() {
        assert_eq!(release_tag("0.36.1"), "v0.36.1");
    }

    #[test]
    fn the_cache_path_is_version_keyed_so_a_bump_does_not_reuse_the_old_build() {
        let root = PathBuf::from("/cache/rauthy");

        assert_eq!(
            cached_binary(&root, "0.36.1"),
            PathBuf::from("/cache/rauthy/0.36.1/rauthy")
        );
        assert_ne!(
            cached_binary(&root, "0.36.1"),
            cached_binary(&root, "0.37.0")
        );
    }

    /// An interrupted copy leaves a file at the right path. Presence
    /// alone would trust it forever, so the version is read back.
    #[test]
    fn the_version_probe_reads_the_reported_version_not_the_path() {
        assert!(reports_version("rauthy 0.36.1", "0.36.1"));
        assert!(reports_version("rauthy v0.36.1\n", "0.36.1"));
        assert!(!reports_version("rauthy 0.35.2", "0.36.1"));
        assert!(!reports_version("", "0.36.1"));
    }
}
