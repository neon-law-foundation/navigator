//! `cli drive` subcommand — mint a refresh token + list Drive
//! contents.
//!
//! Two actions, both behind the `Drive` enum in [`super::main`]:
//!
//! - [`run_login`] — runs Google's installed-app authorization-code
//!   flow against the OAuth client config at
//!   `~/.config/navigator/oauth_client.json`. Spins a one-shot HTTP
//!   listener on `127.0.0.1:8888` (overridable via
//!   `NAVIGATOR_DRIVE_CALLBACK_PORT`) to receive the redirect,
//!   exchanges the auth code for a refresh token, persists it to
//!   `~/.config/navigator/drive_token.json` with `0o600`.
//! - [`run_ls`] — list shared drives (no args) or list a folder's
//!   children (`--drive <id> [--folder <id>]`).

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use cloud::drive::{
    default_drive_token_path, default_oauth_client_path, load_drive_token, load_oauth_client,
    save_drive_token, CliRefreshTokenAuth, DriveClient, DriveToken, DRIVE_READONLY_SCOPE,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const DEFAULT_CALLBACK_PORT: u16 = 8888;

/// Run `cli drive login`. Prints the URL the user must open, waits
/// for the redirect, exchanges the code, saves the refresh token.
pub async fn run_login() -> ExitCode {
    match login_inner().await {
        Ok(token_path) => {
            println!("saved refresh token to {}", token_path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cli drive login: {e:#}");
            ExitCode::from(2)
        }
    }
}

/// Run `cli drive ls`. Lists shared drives by default; with
/// `--drive <id>` lists root contents of that drive; with
/// `--drive <id> --folder <id>` lists children of the named folder.
pub async fn run_ls(drive: Option<&str>, folder: Option<&str>) -> ExitCode {
    match ls_inner(drive, folder).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cli drive ls: {e:#}");
            ExitCode::from(2)
        }
    }
}

async fn login_inner() -> Result<std::path::PathBuf> {
    let client_path = default_oauth_client_path();
    let oauth = load_oauth_client(&client_path)
        .with_context(|| format!("loading {}", client_path.display()))?;
    let port = callback_port_from_env();
    let redirect_uri = format!("http://localhost:{port}");
    let state = random_state();

    // Build the consent URL with `url::Url` so query encoding is
    // correct (the user's client_id and our redirect_uri contain
    // characters that *must* be percent-encoded).
    let mut auth_url = url::Url::parse(&oauth.auth_uri)
        .with_context(|| format!("parse auth_uri `{}`", oauth.auth_uri))?;
    auth_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &oauth.client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", DRIVE_READONLY_SCOPE)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("state", &state);

    println!("Open this URL in your browser to consent:");
    println!();
    println!("    {auth_url}");
    println!();
    println!("Waiting for the redirect on http://localhost:{port} (Ctrl-C to cancel)…");

    let code = wait_for_oauth_callback(port, &state).await?;
    let token_response = exchange_code_for_refresh_token(&oauth, &redirect_uri, &code).await?;

    let token_path = default_drive_token_path();
    let token = DriveToken {
        refresh_token: token_response.refresh_token,
        account: None,
        scope: token_response.scope,
        minted_at: Some(Utc::now().to_rfc3339()),
    };
    save_drive_token(&token_path, &token)?;
    Ok(token_path)
}

async fn ls_inner(drive: Option<&str>, folder: Option<&str>) -> Result<()> {
    let client = build_client_from_disk()?;
    match (drive, folder) {
        (None, _) => {
            let drives = client.list_shared_drives().await?;
            if drives.is_empty() {
                println!("no shared drives — is your account a member of any?");
            } else {
                let id_h = "id";
                let name_h = "name";
                println!("{id_h:<32}  {name_h}");
                for d in drives {
                    println!("{:<32}  {}", d.id, d.name);
                }
            }
        }
        (Some(drive_id), folder_id) => {
            // Shared drive root folder id == drive id.
            let folder_id = folder_id.unwrap_or(drive_id);
            let files = client.list_folder_files(drive_id, folder_id).await?;
            if files.is_empty() {
                println!("(empty)");
            } else {
                let id_h = "id";
                let size_h = "size";
                let kind_h = "kind";
                let name_h = "name";
                println!("{id_h:<36}  {size_h:<8}  {kind_h:<10}  {name_h}");
                for f in files {
                    let kind = if f.mime_type.ends_with(".folder") {
                        "folder"
                    } else if f.mime_type.starts_with("application/vnd.google-apps.") {
                        "g-native"
                    } else {
                        "binary"
                    };
                    let size = f.size.map_or("—".to_string(), |n| n.to_string());
                    println!("{:<36}  {:<8}  {:<10}  {}", f.id, size, kind, f.name);
                }
            }
        }
    }
    Ok(())
}

/// Build a `DriveClient` from the on-disk OAuth client config + the
/// refresh token saved by `cli drive login`. Public so other CLI
/// subcommands (`project sync-drive`) can reuse the auth path
/// instead of re-deriving it.
pub fn build_client_from_disk() -> Result<DriveClient> {
    let oauth_path = default_oauth_client_path();
    let token_path = default_drive_token_path();
    let oauth = load_oauth_client(&oauth_path)
        .with_context(|| format!("loading {}", oauth_path.display()))?;
    let token = load_drive_token(&token_path)
        .with_context(|| format!("loading {}", token_path.display()))?;
    let auth = CliRefreshTokenAuth::with_token_uri(
        oauth.client_id,
        oauth.client_secret,
        token.refresh_token,
        oauth.token_uri,
    );
    Ok(DriveClient::new(Arc::new(auth)))
}

fn callback_port_from_env() -> u16 {
    std::env::var("NAVIGATOR_DRIVE_CALLBACK_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(DEFAULT_CALLBACK_PORT)
}

fn random_state() -> String {
    // 128 bits of entropy hex-encoded — enough to defeat any
    // accidental collision between concurrent `cli drive login`
    // runs on the same host.
    let a: u64 = rand::random();
    let b: u64 = rand::random();
    format!("{a:016x}{b:016x}")
}

/// One-shot loopback HTTP listener that returns the `code` query
/// parameter from the first GET it sees, or an error if the
/// `state` parameter doesn't match what we sent.
async fn wait_for_oauth_callback(port: u16, expected_state: &str) -> Result<String> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    // 5-minute hard timeout so a user who closes the browser tab
    // doesn't leave `cli drive login` hanging forever.
    #[allow(clippy::duration_suboptimal_units)]
    let timeout = Duration::from_secs(5 * 60);
    let (socket, _) = tokio::time::timeout(timeout, listener.accept())
        .await
        .map_err(|_| anyhow!("timed out waiting for browser redirect"))?
        .context("accept callback connection")?;
    handle_callback_socket(socket, expected_state).await
}

async fn handle_callback_socket(
    mut socket: tokio::net::TcpStream,
    expected_state: &str,
) -> Result<String> {
    let mut buf = [0u8; 8192];
    let n = socket.read(&mut buf).await.context("read callback")?;
    let request = std::str::from_utf8(&buf[..n]).context("non-utf8 in callback request")?;
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| anyhow!("empty callback request"))?;
    let path_and_query = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("malformed request line: {first_line}"))?;
    let url = url::Url::parse(&format!("http://localhost{path_and_query}"))
        .with_context(|| format!("parse callback path `{path_and_query}`"))?;
    let mut state = None;
    let mut code = None;
    let mut error = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "state" => state = Some(v.into_owned()),
            "code" => code = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            _ => {}
        }
    }
    if let Some(err) = error {
        let body = format!("<html><body><h1>Login failed</h1><p>{err}</p></body></html>");
        send_response(&mut socket, 400, "Bad Request", "text/html", &body).await;
        return Err(anyhow!("oauth provider returned error: {err}"));
    }
    let state = state.ok_or_else(|| anyhow!("callback missing `state`"))?;
    if state != expected_state {
        send_response(
            &mut socket,
            400,
            "Bad Request",
            "text/plain",
            "state mismatch",
        )
        .await;
        return Err(anyhow!("oauth state mismatch"));
    }
    let code = code.ok_or_else(|| anyhow!("callback missing `code`"))?;
    let body = "<html><body style=\"font-family:system-ui;padding:2rem\">\
                <h1>Login successful</h1>\
                <p>You can close this tab and return to the terminal.</p>\
                </body></html>";
    send_response(&mut socket, 200, "OK", "text/html", body).await;
    Ok(code)
}

async fn send_response(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &str,
) {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len(),
    );
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

#[derive(serde::Deserialize)]
struct TokenExchangeResponse {
    refresh_token: Option<String>,
    #[allow(dead_code)]
    access_token: Option<String>,
    scope: Option<String>,
}

struct ParsedToken {
    refresh_token: String,
    scope: Option<String>,
}

async fn exchange_code_for_refresh_token(
    oauth: &cloud::drive::OauthClientConfig,
    redirect_uri: &str,
    code: &str,
) -> Result<ParsedToken> {
    let http = reqwest::Client::new();
    let resp = http
        .post(&oauth.token_uri)
        .form(&[
            ("code", code),
            ("client_id", oauth.client_id.as_str()),
            ("client_secret", oauth.client_secret.as_str()),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .context("post token_uri")?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("token exchange failed: {status} — {body}"));
    }
    let parsed: TokenExchangeResponse =
        serde_json::from_str(&body).context("parse token endpoint response")?;
    let refresh_token = parsed.refresh_token.ok_or_else(|| {
        anyhow!(
            "token endpoint returned no refresh_token — Google only mints one when \
             `access_type=offline` and the user hasn't previously consented. Revoke at \
             myaccount.google.com/permissions and re-run."
        )
    })?;
    Ok(ParsedToken {
        refresh_token,
        scope: parsed.scope,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    async fn random_localhost_port() -> u16 {
        // Bind to port 0 so the OS picks a free one, then drop the
        // listener to free it for the real callback handler.
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let p = l.local_addr().unwrap().port();
        drop(l);
        p
    }

    async fn drive_a_callback(addr: SocketAddr, query: &str) {
        // Tiny client: write a GET request, read enough to see the
        // response, drop. Doesn't validate the HTML.
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        let line = format!("GET {query} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        s.write_all(line.as_bytes()).await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = s.read(&mut buf).await;
    }

    #[tokio::test]
    async fn callback_returns_code_on_state_match() {
        let port = random_localhost_port().await;
        let server =
            tokio::spawn(async move { wait_for_oauth_callback(port, "expected-state").await });
        // Give the server a moment to bind.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        drive_a_callback(addr, "/?code=auth-code-abc&state=expected-state").await;
        let code = server.await.unwrap().unwrap();
        assert_eq!(code, "auth-code-abc");
    }

    #[tokio::test]
    async fn callback_rejects_on_state_mismatch() {
        let port = random_localhost_port().await;
        let server =
            tokio::spawn(async move { wait_for_oauth_callback(port, "expected-state").await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        drive_a_callback(addr, "/?code=abc&state=tampered").await;
        let err = server.await.unwrap().unwrap_err();
        assert!(format!("{err:#}").contains("state mismatch"));
    }

    #[tokio::test]
    async fn callback_surfaces_provider_error() {
        let port = random_localhost_port().await;
        let server =
            tokio::spawn(async move { wait_for_oauth_callback(port, "expected-state").await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        drive_a_callback(addr, "/?error=access_denied&state=expected-state").await;
        let err = server.await.unwrap().unwrap_err();
        assert!(format!("{err:#}").contains("access_denied"));
    }

    #[test]
    fn random_state_is_32_hex_chars() {
        let s = random_state();
        assert_eq!(s.len(), 32);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
        // Two independent draws shouldn't collide.
        let t = random_state();
        assert_ne!(s, t);
    }
}
