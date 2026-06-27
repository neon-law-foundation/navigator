//! `navigator worktree-env` — a dev environment per git worktree.
//!
//! Codex (and any worktree-based agent harness) creates a fresh git
//! worktree per task and runs a **Setup script** on creation and a
//! **Cleanup script** on teardown. This command is what those scripts
//! call:
//!
//! ```text
//! # Setup script   (runs at worktree creation)
//! cargo run -p cli -- worktree-env up   --path "$CODEX_WORKTREE_PATH"
//! # Cleanup script (runs before worktree cleanup)
//! cargo run -p cli -- worktree-env down --path "$CODEX_WORKTREE_PATH"
//! ```
//!
//! Two modes, both reached through the same front door:
//!
//! - **dev (default)** — the light path agents use in parallel. The KIND
//!   dependency cluster is a shared, persistent fixture (one Postgres,
//!   one Keycloak, one OPA, …); each worktree gets only its own Postgres
//!   **database** (`navigator_<slug>`) and its own host **`web` port**
//!   (`3001 + offset`). `web` runs on the host (`cargo run -p web`)
//!   against that database. Many worktrees coexist because the only
//!   per-worktree host resource is one TCP port; the heavy deps are
//!   shared. This is the sibling of the existing `start-dev-server`
//!   loop.
//! - **demo (`--demo`)** — the full stack running *in* KIND from the
//!   images CI published to ghcr (no local build). Delegates to the
//!   pull-based [`super::deploy`]; one full stack at a time (a demo is
//!   shown, not parallelised).
//!
//! The per-worktree state (`.devx/worktree.json` + `.devx/env`) lives
//! inside the worktree, which is itself gitignored (`/.devx/`), so
//! nothing here ever lands in the tree.

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use super::KindConfig;
use store::DbConfig;

/// The base host `web` port the default dev loop uses (`PORT=3001`). A
/// per-worktree port is this plus a slug-derived offset.
const WEB_PORT_BASE: u16 = 3001;
/// How many distinct host ports a worktree's `web` may land on
/// (`3001..=3100`). The slug hash picks a starting point in this span;
/// a live collision bumps to the next free port within it.
const WEB_PORT_SPAN: u16 = 100;
/// Longest slug we keep — enough to stay readable in a database name and
/// a Codex branch label without risking Postgres's 63-byte identifier
/// limit once `navigator_` is prepended.
const MAX_SLUG_LEN: usize = 40;

#[derive(Subcommand)]
pub enum WorktreeEnvCmd {
    /// Stand up this worktree's environment. Idempotent: re-running
    /// restores it (e.g. after a reboot) and keeps the same port.
    Up {
        /// Worktree directory. Defaults to the current directory — Codex
        /// passes `$CODEX_WORKTREE_PATH`.
        #[arg(long)]
        path: Option<PathBuf>,
        /// Full-stack demo: run `web` + `workflows-service` IN the KIND
        /// cluster from published ghcr images, instead of the light
        /// host-`web` + shared-deps dev environment.
        #[arg(long)]
        demo: bool,
        /// Pin the ghcr image tag to pull (`YY.MM.DD`). Only meaningful
        /// with `--demo`; omit to pull the latest published tag.
        #[arg(long)]
        tag: Option<String>,
        /// Assume the shared KIND dependency cluster is already up; don't
        /// bring it up. Use on a machine where `start-dev-server` already
        /// runs.
        #[arg(long)]
        no_deps: bool,
    },
    /// Tear down this worktree's environment. Idempotent — exits 0 even
    /// if nothing is up. Never touches the shared dependency cluster or
    /// its port-forwards (other worktrees rely on them).
    Down {
        /// Worktree directory. Defaults to the current directory.
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Show this worktree's environment (slug, mode, database, port) and
    /// whether it is reachable.
    Status {
        /// Worktree directory. Defaults to the current directory.
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

/// The persisted descriptor for a worktree's environment, written to
/// `<worktree>/.devx/worktree.json` so `down`/`status` act on exactly
/// what `up` created rather than re-deriving it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WorktreeEnv {
    /// Sanitized worktree identity (from the branch name).
    slug: String,
    /// `"dev"` or `"demo"`.
    mode: String,
    /// The per-worktree Postgres database (dev mode only).
    db_name: Option<String>,
    /// Host `web` port (dev mode) / ingress note (demo mode).
    web_port: u16,
}

pub fn dispatch(cmd: WorktreeEnvCmd, base_cfg: &KindConfig) -> Result<()> {
    match cmd {
        WorktreeEnvCmd::Up {
            path,
            demo,
            tag,
            no_deps,
        } => {
            let root = worktree_root(path.as_deref())?;
            if demo {
                up_demo(&root, tag.as_deref(), base_cfg)
            } else {
                up_dev(&root, no_deps, base_cfg)
            }
        }
        WorktreeEnvCmd::Down { path } => down(&worktree_root(path.as_deref())?, base_cfg),
        WorktreeEnvCmd::Status { path } => {
            status(&worktree_root(path.as_deref())?, base_cfg);
            Ok(())
        }
    }
}

// ---------- dev mode ----------

fn up_dev(root: &Path, no_deps: bool, base_cfg: &KindConfig) -> Result<()> {
    let slug = slug_for(root)?;
    let db_name = db_name(&slug);
    eprintln!("==> worktree-env up (dev): slug={slug} database={db_name}");

    // The shared deps are a persistent fixture. Bring them up only if the
    // Postgres forward isn't already answering — a second worktree must
    // not try (and fail) to re-bind the host ports the first one holds.
    if !no_deps && !port_listening(base_cfg.postgres_port) {
        eprintln!(
            "==> shared deps not reachable on 127.0.0.1:{}; bringing up the KIND fixture",
            base_cfg.postgres_port
        );
        super::up(base_cfg)?;
    }
    super::wait_for_tcp("127.0.0.1", base_cfg.postgres_port).context(
        "the shared KIND Postgres must be reachable before creating a worktree database",
    )?;

    // Reuse this worktree's recorded port across re-runs so `web`'s
    // OAuth redirect URI stays stable; otherwise derive one and bump
    // past any live collision.
    let existing = read_descriptor(root);
    let web_port = choose_web_port(&slug, existing.as_ref().and_then(WorktreeEnv::dev_port))?;

    let maint = maintenance_url(base_cfg.postgres_port);
    let db_url = database_url(base_cfg.postgres_port, &db_name);
    run_async(async {
        let created = ensure_database(&maint, &db_name).await?;
        eprintln!(
            "==> database {db_name} {}",
            if created {
                "created"
            } else {
                "already present"
            }
        );
        // Migrate now so the schema is ready immediately (grant-staff,
        // psql, the first `web` boot all expect it). `web` re-migrates on
        // boot; `store::migrate` is idempotent.
        migrate_database(&db_url).await?;
        Ok::<(), anyhow::Error>(())
    })?;
    eprintln!("==> migrated {db_name} to the latest schema");

    let env_body = super::render_env_for(base_cfg, &db_name, web_port);
    write_worktree_env(root, &env_body)?;
    write_descriptor(
        root,
        &WorktreeEnv {
            slug: slug.clone(),
            mode: "dev".into(),
            db_name: Some(db_name.clone()),
            web_port,
        },
    )?;

    print_dev_summary(&slug, &db_name, web_port);
    Ok(())
}

fn down(root: &Path, base_cfg: &KindConfig) -> Result<()> {
    let desc = read_descriptor(root);
    if desc.as_ref().map(|d| d.mode.as_str()) == Some("demo") {
        eprintln!("==> worktree-env down (demo): deleting the in-cluster stack");
        // Removes the navigator namespace; leaves the cluster + deps.
        super::undeploy(base_cfg)?;
    } else {
        // dev mode (or no descriptor — derive the slug and clean up
        // best-effort so a half-written `up` still tears down).
        let db = desc
            .as_ref()
            .and_then(|d| d.db_name.clone())
            .or_else(|| slug_for(root).ok().map(|s| db_name(&s)));
        if let Some(db) = db {
            if port_listening(base_cfg.postgres_port) {
                let maint = maintenance_url(base_cfg.postgres_port);
                match run_async(drop_database(&maint, &db)) {
                    Ok(()) => eprintln!("==> dropped database {db}"),
                    Err(err) => eprintln!("WARN: could not drop {db}: {err:#}"),
                }
            } else {
                eprintln!(
                    "==> shared Postgres not reachable; leaving {db} (drop it later with \
                     `navigator worktree-env down` once the deps are up)"
                );
            }
        }
    }
    remove_worktree_state(root);
    eprintln!("==> worktree-env down complete");
    Ok(())
}

fn status(root: &Path, base_cfg: &KindConfig) {
    match read_descriptor(root) {
        None => println!("worktree-env: not set up (no .devx/worktree.json)"),
        Some(d) => {
            println!("worktree-env: slug={} mode={}", d.slug, d.mode);
            if let Some(db) = &d.db_name {
                println!("  database: {db}");
            }
            println!(
                "  web port {}: {}",
                d.web_port,
                yes_no(port_listening(d.web_port))
            );
            println!(
                "  shared Postgres 127.0.0.1:{}: {}",
                base_cfg.postgres_port,
                yes_no(port_listening(base_cfg.postgres_port))
            );
        }
    }
}

// ---------- demo mode ----------

fn up_demo(root: &Path, tag: Option<&str>, base_cfg: &KindConfig) -> Result<()> {
    let slug = slug_for(root)?;
    eprintln!("==> worktree-env up (demo): full stack in KIND from ghcr (slug={slug})");
    // Validate `--tag` up front so a bad tag fails before any cluster
    // work, then hand it to `deploy` as a parameter (no process-env
    // mutation).
    if let Some(tag) = tag {
        super::ghcr::validate_release_tag(tag)?;
    }
    super::deploy(base_cfg, tag)?;
    write_descriptor(
        root,
        &WorktreeEnv {
            slug,
            mode: "demo".into(),
            db_name: None,
            web_port: 8080,
        },
    )?;
    eprintln!();
    eprintln!("==> demo stack up. Reach navigator-web through the KIND ingress:");
    eprintln!("    http://localhost:8080");
    eprintln!("    (pre-seed a staff role with `navigator grant-staff`)");
    Ok(())
}

// ---------- slug + port derivation (pure, unit-tested) ----------

/// Derive the worktree's slug from its git branch (falling back to the
/// directory name when detached / not a repo).
fn slug_for(root: &Path) -> Result<String> {
    let branch = git_branch(root);
    let raw = match branch {
        Some(b) if b != "HEAD" => b,
        _ => root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let slug = slugify(&raw);
    if slug.is_empty() {
        bail!("could not derive a worktree slug from {}", root.display());
    }
    Ok(slug)
}

/// Current branch name for the repo at `root`, or `None` if git is
/// unavailable or the call fails.
fn git_branch(root: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Lowercase, replace every run of non-`[a-z0-9]` with a single `-`,
/// trim leading/trailing `-`, and truncate. The result is safe as both a
/// Kubernetes-ish label and (with `-`→`_`) a Postgres identifier.
fn slugify(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_dash = false;
    for ch in raw.to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    let mut s: String = trimmed.chars().take(MAX_SLUG_LEN).collect();
    // A truncation can leave a trailing dash — trim again.
    while s.ends_with('-') {
        s.pop();
    }
    s
}

/// The per-worktree Postgres database name: `navigator_<slug>` with the
/// slug's `-` mapped to `_` (Postgres identifiers don't take `-` without
/// quoting, and `_` reads cleaner).
fn db_name(slug: &str) -> String {
    format!("navigator_{}", slug.replace('-', "_"))
}

/// A small, stable, non-cryptographic hash (FNV-1a, 32-bit) so a given
/// slug always derives the same starting port — no randomness, which
/// would re-roll the port (and the OAuth redirect) on every run.
fn fnv1a(s: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// The starting host `web` port a slug derives, in `3001..3001+span`.
fn derived_web_port(slug: &str) -> u16 {
    WEB_PORT_BASE + u16::try_from(fnv1a(slug) % u32::from(WEB_PORT_SPAN)).unwrap_or(0)
}

/// Choose this worktree's host `web` port. A recorded port (from a prior
/// `up`) is reused verbatim so the OAuth redirect URI stays stable.
/// Otherwise start from the slug-derived port and bump past any port that
/// is currently listening (a rare hash collision with another worktree),
/// staying within the span.
fn choose_web_port(slug: &str, recorded: Option<u16>) -> Result<u16> {
    if let Some(p) = recorded {
        return Ok(p);
    }
    let start = derived_web_port(slug);
    for i in 0..WEB_PORT_SPAN {
        let candidate = WEB_PORT_BASE + ((start - WEB_PORT_BASE + i) % WEB_PORT_SPAN);
        if !port_listening(candidate) {
            return Ok(candidate);
        }
    }
    // Every port in the span is occupied (≈WEB_PORT_SPAN parallel worktree
    // webs). Fail loudly instead of recording a port we know can't bind —
    // a recorded port is reused unconditionally on the next `up`, so a bad
    // one would wedge the worktree until `.devx/worktree.json` is deleted.
    bail!(
        "all {} host ports in [{}..{}] are occupied — stop an unused worktree env \
         (`worktree-env down`) or free a port, then re-run",
        WEB_PORT_SPAN,
        WEB_PORT_BASE,
        WEB_PORT_BASE + WEB_PORT_SPAN - 1
    )
}

// ---------- database operations (async via store/SeaORM) ----------

/// Maintenance connection URL — the same KIND Postgres, but the
/// always-present `postgres` database, so we can `CREATE`/`DROP` the
/// per-worktree database from it.
fn maintenance_url(postgres_port: u16) -> String {
    format!("postgres://navigator:navigator@localhost:{postgres_port}/postgres")
}

/// Per-worktree database connection URL.
fn database_url(postgres_port: u16, db_name: &str) -> String {
    format!("postgres://navigator:navigator@localhost:{postgres_port}/{db_name}")
}

/// Create `db_name` if it does not already exist. Returns whether it was
/// created. `CREATE DATABASE` has no `IF NOT EXISTS`, so check the
/// catalog first. `db_name` is `navigator_` + a `[a-z0-9_]` slug, so the
/// interpolation is injection-safe.
async fn ensure_database(maintenance_url: &str, db_name: &str) -> Result<bool> {
    let db = store::connect(&DbConfig {
        url: maintenance_url.to_string(),
    })
    .await
    .context("connect to the maintenance database")?;
    let exists = db
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            format!("SELECT 1 FROM pg_database WHERE datname = '{db_name}'"),
        ))
        .await
        .context("check whether the worktree database exists")?
        .is_some();
    if !exists {
        db.execute_unprepared(&format!("CREATE DATABASE \"{db_name}\""))
            .await
            .with_context(|| format!("create database {db_name}"))?;
    }
    Ok(!exists)
}

/// Bring `db_url`'s schema to the latest migration (idempotent).
async fn migrate_database(db_url: &str) -> Result<()> {
    let db = store::connect(&DbConfig {
        url: db_url.to_string(),
    })
    .await
    .context("connect to the worktree database")?;
    store::migrate(&db)
        .await
        .context("migrate the worktree database")?;
    Ok(())
}

/// Drop `db_name`, terminating any lingering connections (`WITH FORCE`),
/// idempotent (`IF EXISTS`).
async fn drop_database(maintenance_url: &str, db_name: &str) -> Result<()> {
    let db = store::connect(&DbConfig {
        url: maintenance_url.to_string(),
    })
    .await
    .context("connect to the maintenance database")?;
    db.execute_unprepared(&format!(
        "DROP DATABASE IF EXISTS \"{db_name}\" WITH (FORCE)"
    ))
    .await
    .with_context(|| format!("drop database {db_name}"))?;
    Ok(())
}

/// Run an async future to completion on a private current-thread runtime,
/// mirroring how the rest of `devx` bridges its sync command handlers to
/// async work (`gcp`, `dns`).
fn run_async<F: std::future::Future<Output = Result<()>>>(fut: F) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for worktree database operations")?
        .block_on(fut)
}

// ---------- worktree state files (.devx/) ----------

fn devx_dir(root: &Path) -> PathBuf {
    root.join(".devx")
}

fn descriptor_path(root: &Path) -> PathBuf {
    devx_dir(root).join("worktree.json")
}

fn env_path(root: &Path) -> PathBuf {
    devx_dir(root).join("env")
}

fn read_descriptor(root: &Path) -> Option<WorktreeEnv> {
    let body = std::fs::read_to_string(descriptor_path(root)).ok()?;
    serde_json::from_str(&body).ok()
}

fn write_descriptor(root: &Path, desc: &WorktreeEnv) -> Result<()> {
    std::fs::create_dir_all(devx_dir(root))
        .with_context(|| format!("create {}", devx_dir(root).display()))?;
    let body = serde_json::to_string_pretty(desc).context("serialize worktree descriptor")?;
    std::fs::write(descriptor_path(root), body)
        .with_context(|| format!("write {}", descriptor_path(root).display()))
}

fn write_worktree_env(root: &Path, body: &str) -> Result<()> {
    std::fs::create_dir_all(devx_dir(root))
        .with_context(|| format!("create {}", devx_dir(root).display()))?;
    std::fs::write(env_path(root), body)
        .with_context(|| format!("write {}", env_path(root).display()))
}

fn remove_worktree_state(root: &Path) {
    let _ = std::fs::remove_file(descriptor_path(root));
    let _ = std::fs::remove_file(env_path(root));
}

// ---------- small helpers ----------

/// Walk up from `start` (or the current dir) to the workspace root — the
/// first ancestor holding both `Cargo.toml` and `k8s/`. In a git
/// worktree this is the worktree's own root.
fn worktree_root(start: Option<&Path>) -> Result<PathBuf> {
    let mut dir = match start {
        Some(p) => p
            .canonicalize()
            .with_context(|| format!("resolve worktree path {}", p.display()))?,
        None => std::env::current_dir().context("get current directory")?,
    };
    loop {
        if dir.join("Cargo.toml").is_file() && dir.join("k8s").is_dir() {
            return Ok(dir);
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => bail!(
                "could not find the workspace root (Cargo.toml + k8s/) from the worktree path"
            ),
        }
    }
}

/// Whether something is already listening on `127.0.0.1:<port>`.
fn port_listening(port: u16) -> bool {
    format!("127.0.0.1:{port}")
        .parse()
        .ok()
        .and_then(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(200)).ok())
        .is_some()
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn print_dev_summary(slug: &str, db_name: &str, web_port: u16) {
    eprintln!();
    eprintln!("===========================================================");
    eprintln!(" worktree-env up — dev environment for `{slug}`");
    eprintln!("===========================================================");
    eprintln!();
    eprintln!("  database : {db_name} (on the shared KIND Postgres)");
    eprintln!("  web port : {web_port}");
    eprintln!();
    eprintln!("Start this worktree's web server (under Doppler, or with a");
    eprintln!("stub .env for the Doppler-only secrets — see docs/RUNBOOK.md):");
    eprintln!();
    eprintln!("    set -a; source .devx/env; set +a");
    eprintln!("    cargo run -p web   # listens on :{web_port}");
    eprintln!();
    eprintln!("Tear down (drops {db_name}, leaves the shared deps):");
    eprintln!("    navigator worktree-env down");
    eprintln!();
}

impl WorktreeEnv {
    /// The recorded host `web` port, but only for a dev-mode descriptor —
    /// demo mode records the ingress port (8080), which is not a
    /// per-worktree allocation to reuse.
    fn dev_port(&self) -> Option<u16> {
        (self.mode == "dev").then_some(self.web_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_sanitizes_branch_names() {
        assert_eq!(slugify("codex/blog-rust-ferris"), "codex-blog-rust-ferris");
        assert_eq!(slugify("Feature/ABC_123"), "feature-abc-123");
        assert_eq!(slugify("---weird///name---"), "weird-name");
        assert_eq!(slugify("main"), "main");
        // Truncation never leaves a trailing dash.
        let long = "a".repeat(50) + "/" + &"b".repeat(50);
        let s = slugify(&long);
        assert!(s.len() <= MAX_SLUG_LEN);
        assert!(!s.ends_with('-'));
    }

    #[test]
    fn db_name_maps_dashes_to_underscores_and_prefixes() {
        assert_eq!(db_name("codex-blog-rust"), "navigator_codex_blog_rust");
        assert_eq!(db_name("main"), "navigator_main");
        // The result is a clean `[a-z0-9_]` identifier (injection-safe).
        let name = db_name("feature-abc-123");
        assert!(name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
    }

    #[test]
    fn derived_web_port_is_stable_and_in_span() {
        // Deterministic: same slug → same port across calls.
        assert_eq!(derived_web_port("main"), derived_web_port("main"));
        for slug in ["main", "codex-blog-rust", "feature-x", "a", "zzz-9"] {
            let p = derived_web_port(slug);
            assert!((WEB_PORT_BASE..WEB_PORT_BASE + WEB_PORT_SPAN).contains(&p));
        }
        // Distinct slugs generally land on distinct ports.
        assert_ne!(
            derived_web_port("main"),
            derived_web_port("codex-blog-rust")
        );
    }

    #[test]
    fn choose_web_port_reuses_recorded() {
        // A recorded port is returned verbatim — no probing, no re-roll.
        assert_eq!(choose_web_port("anything", Some(3042)).unwrap(), 3042);
    }

    #[test]
    fn choose_web_port_derives_within_span_when_unrecorded() {
        // With no recorded port it derives one (and probes for a free
        // port) inside the span. At least one port in [3001, 3100] is
        // free in the test environment, so this resolves rather than bails.
        let p = choose_web_port("a-fresh-unrecorded-slug", None).unwrap();
        assert!((WEB_PORT_BASE..WEB_PORT_BASE + WEB_PORT_SPAN).contains(&p));
    }

    #[test]
    fn maintenance_and_database_urls() {
        assert_eq!(
            maintenance_url(15432),
            "postgres://navigator:navigator@localhost:15432/postgres"
        );
        assert_eq!(
            database_url(15432, "navigator_codex_x"),
            "postgres://navigator:navigator@localhost:15432/navigator_codex_x"
        );
    }

    #[test]
    fn descriptor_round_trips_and_dev_port_is_mode_gated() {
        let dev = WorktreeEnv {
            slug: "feature-x".into(),
            mode: "dev".into(),
            db_name: Some("navigator_feature_x".into()),
            web_port: 3042,
        };
        let json = serde_json::to_string(&dev).unwrap();
        let back: WorktreeEnv = serde_json::from_str(&json).unwrap();
        assert_eq!(dev, back);
        assert_eq!(dev.dev_port(), Some(3042));

        // A demo descriptor's recorded port (8080) is NOT reused as a
        // per-worktree dev allocation.
        let demo = WorktreeEnv {
            mode: "demo".into(),
            db_name: None,
            ..dev
        };
        assert_eq!(demo.dev_port(), None);
    }
}
