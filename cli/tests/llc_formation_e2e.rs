#![allow(clippy::doc_markdown)]
//! End-to-end: form a Nevada LLC entirely through the `navigator` CLI
//! binary, driven against an in-process `web` app on a loopback port.
//!
//! This proves the formation flow through the **CLI surface** the prompt
//! specifies — `matter open` → `intake answer` (the seven `nv__llc_formation`
//! questions, including a `people_list` row) → `notation status` →
//! `notation approve` → `notation document` — and asserts with
//! `pdf::read_field_value` that the downloaded bytes are the official
//! Nevada SoS packet carrying the founder's answers, the same assertions
//! `features/tests/nest_formation.rs` makes, now proven through the binary.
//!
//! Both the interactive walk (scripted stdin) and the non-interactive
//! flag walk (`--answer` / `--person`) are exercised. CI-safe: the
//! `StubSignatureProvider` records the send, so nothing reaches DocuSign,
//! and no cloud account is touched (FsStorage, in-memory runtime).

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use store::entity::person::Role;
use store::seed;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use uuid::Uuid;
use web::session::SessionData;
use web::{AppState, AuthConfig, SessionStore};
use workflows::{DispatchingRuntime, InMemoryRuntime, StateMachineRuntime};

const SESSION_KEY: &str = "cli-llc-e2e-key-not-for-production";

/// Build the seeded app with the same wiring `features::journey` uses —
/// canonical templates, FsStorage, a `DispatchingRuntime` that renders +
/// dispatches in-process, and a `StubSignatureProvider`. Auth is ENFORCED
/// (HS256) so the CLI's `Authorization: Bearer <SessionData>` is exercised
/// for real and the document download's required session is populated.
async fn build_app(tag: &str) -> axum::Router {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("navigator-cli-llc-e2e-{tag}")))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&db, &storage).await.unwrap();

    let runtime = Arc::new(InMemoryRuntime::new());
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(DispatchingRuntime::new(
        runtime.clone(),
        email.clone(),
        storage.clone(),
    ));
    let state = AppState {
        auth: AuthConfig::new(false, Some("unused-hs256-secret")),
        sessions: SessionStore::new(SESSION_KEY),
        storage,
        workflow_runtime,
        questionnaire_runtime: runtime,
        signature_provider: Arc::new(web::signature::StubSignatureProvider::new()),
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR))
}

/// A fresh admin session bearer, signed with the test session key — the
/// blob the CLI presents as `Authorization: Bearer …`.
fn admin_token() -> String {
    let mut session = SessionData::fresh("cli-admin", Role::Admin);
    session.email = Some("nick@neonlaw.com".into());
    SessionStore::new(SESSION_KEY).encode(&session)
}

/// Spawn the app on a loopback port and return its base URL.
async fn spawn(tag: &str) -> String {
    let app = build_app(tag).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://127.0.0.1:{}", addr.port())
}

/// Write a `~/.navigator.json`-shaped credential file for `base`, holding
/// the admin bearer with a far-future expiry, and return its path. The CLI
/// reads it via `NAVIGATOR_CREDENTIALS_FILE`.
fn write_creds(dir: &Path, base: &str) -> std::path::PathBuf {
    let path = dir.join("navigator.json");
    let body = serde_json::json!({
        "hosts": { base: { "token": admin_token(), "expires_at": 9_999_999_999i64 } }
    });
    std::fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
    path
}

/// Run the `navigator` binary with the credential file wired in; return
/// (success, stdout). stderr is surfaced into stdout on failure for
/// debugging.
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

/// Run the binary feeding `stdin`, for the interactive walk.
async fn run_cli_stdin(creds: &Path, args: &[&str], stdin: &str) -> (bool, String) {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_navigator"))
        .env("NAVIGATOR_CREDENTIALS_FILE", creds)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn navigator");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .await
        .unwrap();
    let out = child.wait_with_output().await.unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out.status.success(), format!("{stdout}\n{stderr}"))
}

/// Pull the notation UUID out of `matter open`'s stdout (color is
/// stripped for a pipe, so tokens are plain).
fn notation_id_from(stdout: &str) -> Uuid {
    stdout
        .split_whitespace()
        .find_map(|tok| Uuid::parse_str(tok.trim()).ok())
        .unwrap_or_else(|| panic!("no notation id in matter-open output:\n{stdout}"))
}

/// Assert the downloaded packet is the official Nevada SoS form carrying
/// the founder's answers — the same fields `nest_formation.rs` checks.
fn assert_filled_packet(bytes: &[u8]) {
    assert!(bytes.starts_with(b"%PDF"), "the download is a PDF");
    assert_eq!(
        pdf::read_field_value(bytes, "NAME OF ENTITY").as_deref(),
        Some("Bright Star Ventures"),
        "entity name lands on the Initial List",
    );
    assert_eq!(
        pdf::read_field_value(bytes, "managers_b").as_deref(),
        Some("members"),
        "the member-managed box is checked",
    );
    assert_eq!(
        pdf::read_field_value(bytes, "Name3").as_deref(),
        Some("Libra"),
        "the managing member fills slot 1 of the Articles",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forms_an_llc_through_the_cli_with_answer_flags() {
    let base = spawn("flags").await;
    let tmp = tempfile::tempdir().unwrap();
    let creds = write_creds(tmp.path(), &base);

    // 1. Open the formation matter.
    let (ok, out) = run_cli(
        &creds,
        &[
            "matter",
            "open",
            "--host",
            &base,
            "--template",
            "nv__llc_formation",
            "--client-email",
            "libra@example.com",
        ],
    )
    .await;
    assert!(ok, "matter open failed:\n{out}");
    let id = notation_id_from(&out).to_string();

    // 2. Answer all seven questions non-interactively: six scalars in
    //    order + one people_list row via --person.
    let (ok, out) = run_cli(
        &creds,
        &[
            "intake",
            "answer",
            &id,
            "--host",
            &base,
            "--answer",
            "Libra",
            "--answer",
            "libra@example.com",
            "--answer",
            "Bright Star Ventures",
            "--answer",
            "Neon Law Registered Agent",
            "--answer",
            "members",
            "--person",
            "name=Libra,street=1 Main St,city=Las Vegas,state=NV,zip=89101,country=USA",
            "--answer",
            "2026-07-01",
        ],
    )
    .await;
    assert!(ok, "intake answer failed:\n{out}");
    assert!(
        out.contains("questionnaire complete"),
        "walk completes:\n{out}"
    );

    // 3. Status: the walk auto-rendered the packet and reached the
    //    signature wait.
    let (ok, out) = run_cli(
        &creds,
        &["notation", "status", &id, "--host", &base, "--json"],
    )
    .await;
    assert!(ok, "notation status failed:\n{out}");
    assert!(out.contains("sent_for_signature__pending"), "state:\n{out}");
    assert!(
        out.contains("\"document_ready\": true"),
        "packet ready:\n{out}"
    );

    // 4. Approve (idempotent once rendered).
    let (ok, out) = run_cli(&creds, &["notation", "approve", &id, "--host", &base]).await;
    assert!(ok, "notation approve failed:\n{out}");

    // 5. Download the filled packet and assert its AcroForm fields.
    let pdf_path = tmp.path().join("llc.pdf");
    let pdf_str = pdf_path.to_str().unwrap();
    let (ok, out) = run_cli(
        &creds,
        &[
            "notation", "document", &id, "--out", pdf_str, "--host", &base,
        ],
    )
    .await;
    assert!(ok, "notation document failed:\n{out}");
    assert_filled_packet(&std::fs::read(&pdf_path).unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forms_an_llc_through_the_interactive_cli_walk() {
    let base = spawn("interactive").await;
    let tmp = tempfile::tempdir().unwrap();
    let creds = write_creds(tmp.path(), &base);

    let (ok, out) = run_cli(
        &creds,
        &[
            "matter",
            "open",
            "--host",
            &base,
            "--template",
            "nv__llc_formation",
            "--client-email",
            "libra@example.com",
        ],
    )
    .await;
    assert!(ok, "matter open failed:\n{out}");
    let id = notation_id_from(&out).to_string();

    // Scripted stdin: five scalars, then the people_list row (name, then
    // title/street/city/state/zip/country, then a blank name to end), then
    // the final scalar. A blank line is an empty answer for that prompt.
    let stdin = concat!(
        "Libra\n",
        "libra@example.com\n",
        "Bright Star Ventures\n",
        "Neon Law Registered Agent\n",
        "members\n",
        // managing_members people_list, row 1:
        "Libra\n", // name
        "\n",      // title (blank)
        "1 Main St\n",
        "Las Vegas\n",
        "NV\n",
        "89101\n",
        "USA\n",
        "\n", // blank name ends the rows
        // formation_date:
        "2026-07-01\n",
    );
    let (ok, out) = run_cli_stdin(&creds, &["intake", "answer", &id, "--host", &base], stdin).await;
    assert!(ok, "interactive intake answer failed:\n{out}");
    assert!(
        out.contains("questionnaire complete"),
        "walk completes:\n{out}"
    );

    // The interactive walk fills the same packet. Download via the CLI and
    // assert the founder's answers landed on the official form.
    let pdf_path = tmp.path().join("llc.pdf");
    let pdf_str = pdf_path.to_str().unwrap();
    let (ok, out) = run_cli(
        &creds,
        &[
            "notation", "document", &id, "--out", pdf_str, "--host", &base,
        ],
    )
    .await;
    assert!(ok, "notation document failed:\n{out}");
    assert_filled_packet(&std::fs::read(&pdf_path).unwrap());
}
