//! End-to-end: `navigator site sync` against a live HTTP endpoint,
//! driving the real binary as a subprocess.
//!
//! The unit tests in `cli::sync` cover the tree-writing rules on a
//! tempdir. What only the binary can prove is the seam between them: that
//! `sync` sends the stored bearer to `GET /app/projects.csv`, parses the
//! CSV the server's `admin_csv` writer actually emits, and lands a real
//! folder tree on disk from it.
//!
//! The stub serves the exact column set `portal::admin::projects_csv`
//! writes (`id,code,name,status,entity_name`) and refuses a request with
//! no bearer, so a sync that silently dropped the token would fail here
//! rather than quietly syncing an anonymous — that is, empty — list. The
//! matters are the synthetic Henderson cast the workshop already uses.

use std::path::Path;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::routing::get;
use tokio::net::TcpListener;

const BEARER: &str = "sync-e2e-token-not-for-production";

/// The bytes `portal::admin::projects_csv` emits for two visible matters.
const PROJECTS_CSV: &str = "id,code,name,status,entity_name\r\n\
    3f2a1c88-0000-4000-8000-000000000001,henderson-bungalow,Henderson Bungalow Purchase,open,Henderson Holdings LLC\r\n\
    3f2a1c88-0000-4000-8000-000000000002,virgo-deed,Virgo Deed of Sale,open,\r\n";

/// A stub that answers `/app/projects.csv` only for the right bearer.
async fn spawn_stub() -> String {
    let app = axum::Router::new().route(
        "/app/projects.csv",
        get(|req: Request| async move {
            let authorized = req
                .headers()
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v == format!("Bearer {BEARER}"));
            if authorized {
                (StatusCode::OK, PROJECTS_CSV)
            } else {
                (StatusCode::FORBIDDEN, "")
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{}", addr.port())
}

/// Write a `~/.navigator.json`-shaped credential file holding `BEARER`
/// for `base` with a far-future expiry.
fn write_creds(dir: &Path, base: &str) -> std::path::PathBuf {
    let path = dir.join("navigator.json");
    let body = serde_json::json!({
        "hosts": { base: { "token": BEARER, "expires_at": 9_999_999_999i64 } }
    });
    std::fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
    path
}

async fn run_cli(creds: &Path, args: &[&str]) -> (bool, String) {
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_navigator"))
        .env("NAVIGATOR_CREDENTIALS_FILE", creds)
        .args(args)
        .output()
        .await
        .expect("run navigator");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out.status.success(), format!("{stdout}\n{stderr}"))
}

#[tokio::test]
async fn sync_writes_the_tree_the_site_lists() {
    let base = spawn_stub().await;
    let dir = tempfile::tempdir().unwrap();
    let creds = write_creds(dir.path(), &base);
    let root = dir.path().join("Projects");

    let (ok, out) = run_cli(
        &creds,
        &[
            "site",
            "sync",
            "--host",
            &base,
            "--root",
            root.to_str().unwrap(),
        ],
    )
    .await;
    assert!(ok, "sync failed: {out}");
    assert!(out.contains("2 matters"), "{out}");

    // One folder per matter, each carrying its card.
    let card = std::fs::read_to_string(root.join("henderson-bungalow/README.md")).unwrap();
    assert!(card.contains("# Henderson Bungalow Purchase"), "{card}");
    assert!(card.contains("Henderson Holdings LLC"), "{card}");
    assert!(
        card.contains(&format!(
            "{base}/app/projects/3f2a1c88-0000-4000-8000-000000000001"
        )),
        "the card must link back to the authoritative workbench: {card}"
    );
    assert!(std::fs::read_to_string(root.join("virgo-deed/README.md")).is_ok());

    // The standing guide, at the root, under both filenames.
    let guide = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert_eq!(
        guide,
        std::fs::read_to_string(root.join("AGENTS.md")).unwrap()
    );
    assert!(guide.contains("navigator site sync"), "{guide}");

    // Re-running is a no-op that reports honestly.
    let (ok, out) = run_cli(
        &creds,
        &[
            "site",
            "sync",
            "--host",
            &base,
            "--root",
            root.to_str().unwrap(),
        ],
    )
    .await;
    assert!(ok, "second sync failed: {out}");
    assert!(out.contains("2 matters"), "{out}");
    assert!(
        !out.contains("new "),
        "nothing was new the second time: {out}"
    );
}

#[tokio::test]
async fn dry_run_lists_the_matters_without_touching_the_filesystem() {
    let base = spawn_stub().await;
    let dir = tempfile::tempdir().unwrap();
    let creds = write_creds(dir.path(), &base);
    let root = dir.path().join("Projects");

    let (ok, out) = run_cli(
        &creds,
        &[
            "site",
            "sync",
            "--host",
            &base,
            "--root",
            root.to_str().unwrap(),
            "--dry-run",
        ],
    )
    .await;
    assert!(ok, "dry run failed: {out}");
    assert!(out.contains("henderson-bungalow"), "{out}");
    assert!(out.contains("virgo-deed"), "{out}");
    assert!(!root.exists(), "--dry-run must not create the tree");
}

#[tokio::test]
async fn sync_without_a_login_writes_nothing() {
    let base = spawn_stub().await;
    let dir = tempfile::tempdir().unwrap();
    // A credential file that knows nothing about this host.
    let creds = write_creds(dir.path(), "https://elsewhere.example");
    let root = dir.path().join("Projects");

    let (ok, out) = run_cli(
        &creds,
        &[
            "site",
            "sync",
            "--host",
            &base,
            "--root",
            root.to_str().unwrap(),
        ],
    )
    .await;
    assert!(!ok, "sync must refuse without a login: {out}");
    assert!(out.contains("not logged in"), "{out}");
    assert!(!root.exists(), "a refused sync must not create the tree");
}
