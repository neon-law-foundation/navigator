//! Smart-HTTP git transport — serve each Project's append-only repo over
//! HTTPS at `/projects/<id>.git/...`, gated by a Personal Access Token
//! and the project-scope ACL we already run.
//!
//! See [the design](../../docs/git-project-repos.md) §1–§3. The wire
//! protocol is git's own: we shell to `git upload-pack` (fetch) and
//! `git receive-pack` (push) with `--stateless-rpc`, exactly as git's
//! reference HTTP server does, rather than reimplement pack negotiation
//! (`feedback_infra_kind_gke`).
//!
//! ## Auth (§2)
//!
//! A git CLI carries no session cookie — it sends HTTP Basic. The
//! password is a `web`-minted PAT (`store::git_access_tokens`); the
//! username is ignored (the GitHub convention). The token resolves to a
//! `persons` identity, validated in the same place `/mcp` validates its
//! bearer.
//!
//! ## Authorization (§3)
//!
//! One repo ↔ one Project, so the existing project-scope check *is* the
//! repo ACL ([`crate::access::can_see_project`]: `admin` bypass, else a
//! `person_project_roles` row). Fetch needs read; push needs a
//! write-scoped token over the same project check — push is a strict
//! superset of fetch. The three failure modes are distinct: `401`
//! (no/invalid PAT), `403` (valid identity, denied), `404` (no such
//! Project).
//!
//! The repo root is `NAVIGATOR_GIT_REPO_ROOT` (deploy config, never
//! hard-coded — `feedback_skills_no_hardcoded_values`); a Project's bare
//! repo is created lazily on first authorized access.

// The transport's internal helpers return `Result<_, Response>` so a
// failed auth/authz short-circuits with a ready-made HTTP response. An
// axum `Response` is a large `Err` variant, but it never crosses an
// allocation-sensitive boundary here (it is returned and immediately
// emitted), so the idiom is worth the size.
#![allow(clippy::result_large_err)]

use std::io::Read as _;
use std::path::Path;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use base64::Engine as _;
use sea_orm::EntityTrait as _;
use serde::Deserialize;
use uuid::Uuid;

use crate::access::can_see_project;
use crate::AppState;

/// Realm announced on a `401`. Git prompts the user for a credential.
const REALM: &str = "Navigator matter repository";

/// Mount the transport routes. `{repo}` captures `<project-id>.git`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/projects/{repo}/info/refs", get(info_refs))
        .route("/projects/{repo}/git-upload-pack", post(upload_pack))
        .route("/projects/{repo}/git-receive-pack", post(receive_pack))
}

/// `?service=git-upload-pack` (or `git-receive-pack`).
#[derive(Debug, Deserialize)]
struct InfoRefsQuery {
    service: Option<String>,
}

/// A git operation, in the two spellings the protocol uses: the service
/// name on the wire (`git-upload-pack`) and the subcommand we invoke
/// (`upload-pack`).
#[derive(Clone, Copy)]
struct GitService {
    /// Wire name, e.g. `git-upload-pack`.
    full: &'static str,
    /// Subcommand passed to `git`, e.g. `upload-pack`.
    bin: &'static str,
    /// `true` for push (`receive-pack`) — requires a write-scoped token.
    is_write: bool,
}

const UPLOAD_PACK: GitService = GitService {
    full: "git-upload-pack",
    bin: "upload-pack",
    is_write: false,
};
const RECEIVE_PACK: GitService = GitService {
    full: "git-receive-pack",
    bin: "receive-pack",
    is_write: true,
};

impl GitService {
    fn from_full(name: &str) -> Option<Self> {
        match name {
            "git-upload-pack" => Some(UPLOAD_PACK),
            "git-receive-pack" => Some(RECEIVE_PACK),
            _ => None,
        }
    }
}

// ---- handlers -------------------------------------------------------

async fn info_refs(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    Query(q): Query<InfoRefsQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(service) = q.service.as_deref().and_then(GitService::from_full) else {
        // The "dumb" protocol (no `?service=`) is not served.
        return (StatusCode::FORBIDDEN, "smart HTTP only").into_response();
    };

    let repo_path = match authorize(&state, &repo, service, &headers).await {
        Ok(path) => path,
        Err(resp) => return resp,
    };

    let advertised = match git_rpc(&repo_path, service, true, None).await {
        Ok(out) => out,
        Err(e) => return git_failure(&e),
    };

    // Smart-HTTP ref advertisement: a pkt-line service banner, a flush,
    // then upload-pack/receive-pack's own advertisement.
    let mut body = pkt_line(&format!("# service={}\n", service.full));
    body.extend_from_slice(b"0000");
    body.extend_from_slice(&advertised);

    let content_type = format!("application/x-{}-advertisement", service.full);
    (StatusCode::OK, no_cache_headers(&content_type), body).into_response()
}

async fn upload_pack(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    rpc(state, repo, UPLOAD_PACK, headers, body).await
}

async fn receive_pack(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    rpc(state, repo, RECEIVE_PACK, headers, body).await
}

async fn rpc(
    state: AppState,
    repo: String,
    service: GitService,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let repo_path = match authorize(&state, &repo, service, &headers).await {
        Ok(path) => path,
        Err(resp) => return resp,
    };

    // Git gzips the upload-pack request body; inflate when announced.
    let input = match decode_body(&headers, body) {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    match git_rpc(&repo_path, service, false, Some(input)).await {
        Ok(out) => {
            let content_type = format!("application/x-{}-result", service.full);
            (StatusCode::OK, no_cache_headers(&content_type), out).into_response()
        }
        Err(e) => git_failure(&e),
    }
}

// ---- auth + authz ---------------------------------------------------

/// Authenticate the PAT and authorize the caller for the Project named
/// by `<project-id>.git`, returning the project id on success or a
/// ready-to-return error [`Response`]. Shared by the transport and the
/// LFS API ([`crate::git_lfs`]).
///
/// `write` selects the stricter push check: a write-scoped token over
/// the same project ACL. The failure statuses are distinct — `401`
/// (no/invalid PAT), `403` (valid identity, denied), `404` (no such
/// Project) — so a git client surfaces the right message.
pub(crate) async fn authorize_project(
    state: &AppState,
    repo_segment: &str,
    write: bool,
    headers: &HeaderMap,
) -> Result<Uuid, Response> {
    // `<project-id>.git` → project id.
    let project_id = repo_segment
        .strip_suffix(".git")
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| (StatusCode::NOT_FOUND, "no such repository").into_response())?;

    // PAT from HTTP Basic. Absent/garbled → 401 with a challenge.
    let Some(pat) = basic_password(headers) else {
        return Err(unauthorized());
    };
    let token = match store::git_access_tokens::validate(&state.db, &pat, chrono::Utc::now()).await
    {
        Ok(Some(t)) => t,
        Ok(None) => return Err(unauthorized()),
        Err(e) => {
            tracing::error!(error = %e, "git transport: token validation failed");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "auth error").into_response());
        }
    };

    // A project-scoped token may only touch its own Project.
    if let Some(scoped) = token.project_id {
        if scoped != project_id {
            return Err(forbidden());
        }
    }

    // Push requires a write-scoped token; fetch accepts read or write.
    if write && token.scope != store::entity::git_access_token::SCOPE_WRITE {
        return Err(forbidden());
    }

    // The project-scope check is the repo ACL.
    let person = match store::entity::person::Entity::find_by_id(token.person_id)
        .one(&state.db)
        .await
    {
        Ok(Some(p)) => p,
        Ok(None) => return Err(unauthorized()),
        Err(e) => {
            tracing::error!(error = %e, "git transport: person lookup failed");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "auth error").into_response());
        }
    };
    match can_see_project(&state.db, Some(person.id), person.role, project_id).await {
        Ok(true) => Ok(project_id),
        Ok(false) => Err(forbidden()),
        Err(e) => {
            tracing::error!(error = %e, "git transport: authz check failed");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "authz error").into_response())
        }
    }
}

/// Authorize the request and return the bare repo's path, lazily
/// creating it for an authorized caller.
async fn authorize(
    state: &AppState,
    repo_segment: &str,
    service: GitService,
    headers: &HeaderMap,
) -> Result<std::path::PathBuf, Response> {
    let project_id = authorize_project(state, repo_segment, service.is_write, headers).await?;

    // Authorized — locate (and lazily create) the bare repo.
    let store = match repos::RepoStore::from_env() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "git transport: repo root misconfigured");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "repo storage unavailable",
            )
                .into_response());
        }
    };
    match tokio::task::spawn_blocking(move || store.ensure(project_id)).await {
        Ok(Ok(path)) => Ok(path),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "git transport: repo ensure failed");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "repo init failed").into_response())
        }
        Err(e) => {
            tracing::error!(error = %e, "git transport: ensure task panicked");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "repo init failed").into_response())
        }
    }
}

/// Extract the password from an HTTP Basic `Authorization` header. The
/// username is ignored (git sends an arbitrary one with a token).
fn basic_password(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let b64 = raw
        .strip_prefix("Basic ")
        .or_else(|| raw.strip_prefix("basic "))?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    // `user:password` — the password is everything after the first ':'.
    decoded.split_once(':').map(|(_, pass)| pass.to_string())
}

fn unauthorized() -> Response {
    let mut resp = (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_str(&format!("Basic realm=\"{REALM}\"")).expect("static realm"),
    );
    resp
}

fn forbidden() -> Response {
    (StatusCode::FORBIDDEN, "not authorized for this matter").into_response()
}

// ---- git plumbing ---------------------------------------------------

/// Run `git <service> --stateless-rpc [--advertise-refs] <repo>`,
/// optionally feeding `stdin`, returning stdout.
async fn git_rpc(
    repo: &Path,
    service: GitService,
    advertise: bool,
    stdin: Option<Bytes>,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncWriteExt as _;
    use tokio::process::Command;

    let mut cmd = Command::new("git");
    cmd.arg(service.bin).arg("--stateless-rpc");
    if advertise {
        cmd.arg("--advertise-refs");
    }
    cmd.arg(repo);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(if stdin.is_some() {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });

    let mut child = cmd.spawn()?;
    if let Some(bytes) = stdin {
        let mut sink = child.stdin.take().expect("stdin piped");
        sink.write_all(&bytes).await?;
        sink.shutdown().await?;
        drop(sink);
    }
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "git {} exited {}: {}",
            service.bin,
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out.stdout)
}

fn git_failure(e: &std::io::Error) -> Response {
    tracing::error!(error = %e, "git transport: subprocess failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "git backend error").into_response()
}

/// Inflate the request body if the client announced `Content-Encoding:
/// gzip` (git does this for upload-pack negotiation).
fn decode_body(headers: &HeaderMap, body: Bytes) -> Result<Bytes, Response> {
    let gzipped = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("gzip"));
    if !gzipped {
        return Ok(body);
    }
    let mut decoder = flate2::read::GzDecoder::new(&body[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(|e| {
        tracing::warn!(error = %e, "git transport: bad gzip request body");
        (StatusCode::BAD_REQUEST, "bad gzip body").into_response()
    })?;
    Ok(Bytes::from(out))
}

/// A git pkt-line: 4-hex length prefix (covering the prefix itself) plus
/// the payload.
fn pkt_line(s: &str) -> Vec<u8> {
    let len = s.len() + 4;
    let mut out = format!("{len:04x}").into_bytes();
    out.extend_from_slice(s.as_bytes());
    out
}

/// Smart-HTTP responses must not be cached (refs change).
fn no_cache_headers(content_type: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type).expect("valid content type"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, max-age=0, must-revalidate"),
    );
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkt_line_prefixes_hex_length() {
        // "# service=git-upload-pack\n" is 26 bytes; +4 = 30 = 0x1e — the
        // canonical git smart-HTTP service banner is `001e# service=…\n`.
        let line = pkt_line("# service=git-upload-pack\n");
        assert!(line.starts_with(b"001e"));
        assert_eq!(line.len(), 30);
    }

    #[test]
    fn basic_password_extracts_the_token() {
        let mut headers = HeaderMap::new();
        // base64("x:my-pat")
        let cred = base64::engine::general_purpose::STANDARD.encode("x:my-pat");
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Basic {cred}")).unwrap(),
        );
        assert_eq!(basic_password(&headers).as_deref(), Some("my-pat"));
    }

    #[test]
    fn basic_password_absent_is_none() {
        assert_eq!(basic_password(&HeaderMap::new()), None);
    }

    #[test]
    fn git_services_map_from_wire_names() {
        assert!(!GitService::from_full("git-upload-pack").unwrap().is_write);
        assert!(GitService::from_full("git-receive-pack").unwrap().is_write);
        assert!(GitService::from_full("git-bogus").is_none());
    }
}
