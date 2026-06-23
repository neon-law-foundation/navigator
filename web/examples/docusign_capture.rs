#![allow(clippy::doc_markdown)] // "DocuSign" et al. — same allow the crate root carries.
//! Exact-byte capture server for grounding the DocuSign Connect webhook.
//!
//! DocuSign Connect signs each delivery with HMAC-SHA256 over the **raw**
//! request body and sends the digest in `X-DocuSign-Signature-1`. To
//! ground [`web::webhook_auth::verify_hmac_sha256_b64`] against a
//! signature DocuSign actually produced — not a hand-rolled one — we
//! need the exact bytes it sent. Copy-pasting from a web capture tool
//! mangles whitespace and reopens the very re-serialization gap the
//! raw-body check exists to close, so this writes each delivery verbatim
//! to disk instead.
//!
//! Run it, expose it with a tunnel, and point a sandbox Connect config at
//! the tunnel URL:
//!
//! ```bash
//! cargo run -p web --example docusign_capture          # listens on :8088
//! ngrok http 8088                                       # public https URL
//! # DocuSign (DEMO account) → Admin → Connect → Add Configuration:
//! #   URL = <ngrok-url>/anything   (path is ignored — every POST is captured)
//! #   data format = JSON, HMAC enabled, events: Completed / Declined / Voided
//! ```
//!
//! Each POST writes two files under the capture dir (default
//! `./docusign-capture`, override with `DOCUSIGN_CAPTURE_DIR`):
//! `<n>.body.json` (the exact bytes) and `<n>.sig` (the
//! `X-DocuSign-Signature-1` value). It then prints a one-line summary so
//! you can see the event classify and whether the signature header was
//! present (absent = HMAC not enabled on the Connect config).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::Router;

/// The header DocuSign Connect carries the base64 HMAC digest in. Matches
/// `web::esignature_webhook`'s `SIGNATURE_HEADER` (lookups are
/// case-insensitive).
const SIGNATURE_HEADER: &str = "x-docusign-signature-1";

#[derive(Clone)]
struct Capture {
    dir: PathBuf,
    seq: Arc<AtomicUsize>,
}

#[tokio::main]
async fn main() {
    let dir = PathBuf::from(
        std::env::var("DOCUSIGN_CAPTURE_DIR").unwrap_or_else(|_| "docusign-capture".to_string()),
    );
    std::fs::create_dir_all(&dir).expect("create capture dir");
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8088);

    let state = Capture {
        dir: dir.clone(),
        seq: Arc::new(AtomicUsize::new(0)),
    };
    // Capture every POST regardless of path, so the Connect URL's path
    // segment (the `/webhook/esignature/:secret` shape, or anything else)
    // does not have to match.
    let app = Router::new()
        .route("/", post(capture))
        .route("/{*rest}", post(capture))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("bind capture port");
    println!(
        "docusign capture listening on :{port}, writing to {}/",
        dir.display()
    );
    println!(
        "expose it (e.g. `ngrok http {port}`) and point a DEMO Connect config at the tunnel URL."
    );
    axum::serve(listener, app).await.expect("serve");
}

async fn capture(State(cap): State<Capture>, headers: HeaderMap, body: Bytes) -> &'static str {
    let n = cap.seq.fetch_add(1, Ordering::SeqCst);
    let sig = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let body_path = cap.dir.join(format!("{n}.body.json"));
    let sig_path = cap.dir.join(format!("{n}.sig"));
    if let Err(e) = std::fs::write(&body_path, &body) {
        eprintln!("capture #{n}: failed to write body: {e}");
    }
    if let Err(e) = std::fs::write(&sig_path, sig) {
        eprintln!("capture #{n}: failed to write sig: {e}");
    }

    let event = classify(&body);
    let sig_note = if sig.is_empty() {
        "NO SIGNATURE HEADER (enable HMAC on the Connect config)".to_string()
    } else {
        format!("sig={}…", sig.chars().take(12).collect::<String>())
    };
    println!(
        "captured #{n}: {} bytes, event~={event}, {sig_note} -> {}",
        body.len(),
        body_path.display()
    );
    "OK"
}

/// Best-effort label for the console line — just scans the raw bytes for
/// the terminal status words. The real classification is grounded later
/// by the hermetic test that parses these fixtures with `ConnectPayload`.
fn classify(body: &[u8]) -> &'static str {
    let haystack = String::from_utf8_lossy(body).to_ascii_lowercase();
    if haystack.contains("completed") {
        "completed"
    } else if haystack.contains("declined") {
        "declined"
    } else if haystack.contains("voided") {
        "voided"
    } else {
        "other"
    }
}
