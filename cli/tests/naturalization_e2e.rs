#![allow(clippy::doc_markdown)]
//! End-to-end: open a federal naturalization matter and walk the Form
//! N-400 intake entirely through the `navigator` CLI binary, driven against
//! an in-process `web` app on a loopback port.
//!
//! This is the CLI demo path for the immigration workflow — `matter open`
//! → `intake answer` (the twelve `naturalization__federal` questions) →
//! `notation status` → `notation approve` → `notation document` — proving
//! the applicant's answers render into the N-400 intake-summary PDF and the
//! matter parks at the signature wait, all through the binary.
//!
//! Both the non-interactive flag walk (`--answer`) and the interactive
//! scripted-stdin walk are exercised. CI-safe: the `StubSignatureProvider`
//! records the send, so nothing reaches DocuSign, and no cloud account is
//! touched (FsStorage, in-memory runtime).

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

const SESSION_KEY: &str = "cli-naturalization-e2e-key-not-for-production";

/// The twelve N-400 intake answers, in questionnaire order. Each is a
/// scalar (string / date / radio choice / yes_no), so each is one
/// `--answer` on the non-interactive walk and one line on the interactive
/// one.
const ANSWERS: [&str; 12] = [
    "Maria Santos",
    "maria@example.com",
    "1990-04-12",
    "Mexico",
    "Mexico",
    "A123456789",
    "2019-03-01",
    "702-555-0100",
    "five_year",
    "married",
    "45",
    "no",
];

/// Build the seeded app with the same wiring `features::journey` uses —
/// canonical templates (including `naturalization__federal`), FsStorage, a
/// `DispatchingRuntime` that renders + dispatches in-process, and a
/// `StubSignatureProvider`. Auth is ENFORCED (HS256) so the CLI's
/// `Authorization: Bearer <SessionData>` is exercised for real.
async fn build_app(tag: &str) -> axum::Router {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("navigator-cli-natz-e2e-{tag}")))
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
/// the admin bearer with a far-future expiry, and return its path.
fn write_creds(dir: &Path, base: &str) -> std::path::PathBuf {
    let path = dir.join("navigator.json");
    let body = serde_json::json!({
        "hosts": { base: { "token": admin_token(), "expires_at": 9_999_999_999i64 } }
    });
    std::fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
    path
}

/// Run the `navigator` binary with the credential file wired in; return
/// (success, stdout+stderr).
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

/// Pull the notation UUID out of `matter open`'s stdout.
fn notation_id_from(stdout: &str) -> Uuid {
    stdout
        .split_whitespace()
        .find_map(|tok| Uuid::parse_str(tok.trim()).ok())
        .unwrap_or_else(|| panic!("no notation id in matter-open output:\n{stdout}"))
}

/// Assert the downloaded artifact is the rendered N-400 intake-summary PDF.
/// There is no AcroForm here (the body renders from the answers), so the
/// proof is a non-trivial PDF document.
fn assert_rendered_pdf(bytes: &[u8]) {
    assert!(bytes.starts_with(b"%PDF"), "the download is a PDF");
    assert!(bytes.len() > 1024, "the rendered summary is non-trivial");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn naturalization_intake_through_the_cli_with_answer_flags() {
    let base = spawn("flags").await;
    let tmp = tempfile::tempdir().unwrap();
    let creds = write_creds(tmp.path(), &base);

    // 1. Open the naturalization matter.
    let (ok, out) = run_cli(
        &creds,
        &[
            "matter",
            "open",
            "--host",
            &base,
            "--template",
            "naturalization__federal",
            "--client-email",
            "maria@example.com",
        ],
    )
    .await;
    assert!(ok, "matter open failed:\n{out}");
    let id = notation_id_from(&out).to_string();

    // 2. Answer all twelve N-400 questions non-interactively.
    let mut args: Vec<&str> = vec!["intake", "answer", &id, "--host", &base];
    for a in ANSWERS {
        args.push("--answer");
        args.push(a);
    }
    let (ok, out) = run_cli(&creds, &args).await;
    assert!(ok, "intake answer failed:\n{out}");
    assert!(
        out.contains("questionnaire complete"),
        "walk completes:\n{out}"
    );

    // 3. Status: the walk auto-rendered the N-400 summary and reached the
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
        "summary ready:\n{out}"
    );

    // 4. Approve (idempotent once rendered).
    let (ok, out) = run_cli(&creds, &["notation", "approve", &id, "--host", &base]).await;
    assert!(ok, "notation approve failed:\n{out}");

    // 5. Download the rendered N-400 intake summary.
    let pdf_path = tmp.path().join("n400.pdf");
    let pdf_str = pdf_path.to_str().unwrap();
    let (ok, out) = run_cli(
        &creds,
        &[
            "notation", "document", &id, "--out", pdf_str, "--host", &base,
        ],
    )
    .await;
    assert!(ok, "notation document failed:\n{out}");
    assert_rendered_pdf(&std::fs::read(&pdf_path).unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn naturalization_intake_through_the_interactive_cli_walk() {
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
            "naturalization__federal",
            "--client-email",
            "maria@example.com",
        ],
    )
    .await;
    assert!(ok, "matter open failed:\n{out}");
    let id = notation_id_from(&out).to_string();

    // Scripted stdin: the twelve scalar answers, one per line.
    let stdin = format!("{}\n", ANSWERS.join("\n"));
    let (ok, out) =
        run_cli_stdin(&creds, &["intake", "answer", &id, "--host", &base], &stdin).await;
    assert!(ok, "interactive intake answer failed:\n{out}");
    assert!(
        out.contains("questionnaire complete"),
        "walk completes:\n{out}"
    );

    // The interactive walk renders the same summary. Download it via the CLI.
    let pdf_path = tmp.path().join("n400.pdf");
    let pdf_str = pdf_path.to_str().unwrap();
    let (ok, out) = run_cli(
        &creds,
        &[
            "notation", "document", &id, "--out", pdf_str, "--host", &base,
        ],
    )
    .await;
    assert!(ok, "notation document failed:\n{out}");
    assert_rendered_pdf(&std::fs::read(&pdf_path).unwrap());
}
