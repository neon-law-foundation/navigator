//! Tiny Cloud Run binary that serves the host-dispatched
//! redirects in [`cloud::redirect::router`].
//!
//! Cloud Run injects `PORT` (always `8080` today, but the docs
//! reserve the right to change it); fall back to `8080` for local
//! `cargo run -p cloud --bin redirect` invocations.

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // One observability seam for every binary: stdout logs (JSON when an
    // OTLP endpoint is set) plus OTLP traces + metrics. Held to end of main
    // so the drop flushes any batched export before the process exits.
    let _telemetry = telemetry::init("navigator-redirect");

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(?addr, "redirect server listening");
    axum::serve(listener, cloud::redirect::router()).await?;
    Ok(())
}
