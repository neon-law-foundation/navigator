//! Client "download all my documents" — a ZIP of the matter's current
//! files.
//!
//! `GET /portal/projects/:id/documents.zip` streams a plain ZIP built
//! from the matter repo's HEAD working tree, with the documents' human
//! filenames. This is the client council's "get my files out cleanly":
//! never a git packfile or bundle, and **no git jargon reaches the
//! client** — the URL and the archive are about *documents*, not
//! repositories.
//!
//! # Authorization
//!
//! Row-scoped exactly like the rest of `/portal`: an admin sees any
//! matter; staff and clients only the matters they hold a
//! `person_project_roles` row for. A non-participant gets `404` — the
//! matter "doesn't exist" for them — never `403`.

use std::io::Write;

use axum::body::Body;
use axum::extract::{Extension, Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use sea_orm::EntityTrait;
use store::entity::{person::Role, project};
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use crate::access::can_see_project;
use crate::admin::AdminState;
use crate::session::SessionData;

/// `GET /portal/projects/:id/documents.zip`.
pub async fn download_all(
    State(state): State<AdminState>,
    Path(project_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let (person_id, role) = match session.as_deref() {
        Some(s) => (s.person_id, s.role),
        None => (None, Role::Client),
    };
    match can_see_project(&state.db, person_id, role, project_id).await {
        Ok(true) => {}
        Ok(false) => return not_found(),
        Err(e) => {
            tracing::error!(error = %e, %project_id, "documents.zip: can_see_project failed");
            return internal_error();
        }
    }
    let Ok(Some(proj)) = project::Entity::find_by_id(project_id).one(&state.db).await else {
        return not_found();
    };

    // Reading HEAD's tree shells git, so run it off the async pool.
    let files = match tokio::task::spawn_blocking(move || {
        let store = repos::RepoStore::from_env()?;
        store.read_head_tree(project_id)
    })
    .await
    {
        Ok(Ok(files)) => files,
        // Repo layer not configured (no NAVIGATOR_GIT_REPO_ROOT) — there
        // are simply no files to hand back; a valid empty archive.
        Ok(Err(repos::RepoError::RootUnset)) => Vec::new(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, %project_id, "documents.zip: read_head_tree failed");
            return internal_error();
        }
        Err(e) => {
            tracing::error!(error = %e, %project_id, "documents.zip: blocking task panicked");
            return internal_error();
        }
    };

    let zip_bytes = match build_zip(&files) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!(error = %e, %project_id, "documents.zip: zip build failed");
            return internal_error();
        }
    };

    let download_name = format!("{}-documents.zip", filename_slug(&proj.name));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{download_name}\""),
        )
        .body(Body::from(zip_bytes))
        .unwrap_or_else(|_| internal_error())
}

/// Package `(path, bytes)` pairs into an in-memory ZIP, deflate-compressed.
fn build_zip(files: &[(String, Vec<u8>)]) -> zip::result::ZipResult<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (path, bytes) in files {
            zip.start_file(path, opts)?;
            zip.write_all(bytes)?;
        }
        zip.finish()?;
    }
    Ok(cursor.into_inner())
}

/// Turn a matter name into a safe download-filename stem: lowercase,
/// alphanumerics kept, every run of anything else collapsed to a single
/// `-`. Empty names fall back to `matter`.
fn filename_slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "matter".to_string()
    } else {
        trimmed.to_string()
    }
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        views::not_found_page_with_auth(views::AuthState::Authenticated),
    )
        .into_response()
}

fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        views::internal_error_page(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{build_zip, filename_slug};

    #[test]
    fn slug_is_filename_safe() {
        assert_eq!(filename_slug("Libra Estate Plan"), "libra-estate-plan");
        assert_eq!(filename_slug("Acme, LLC — Formation"), "acme-llc-formation");
        assert_eq!(filename_slug("   "), "matter");
    }

    #[test]
    fn build_zip_round_trips_paths_and_bytes() {
        let files = vec![
            ("will.txt".to_string(), b"the will".to_vec()),
            ("folder/trust.pdf".to_string(), b"trust bytes".to_vec()),
        ];
        let bytes = build_zip(&files).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(archive.len(), 2);
        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["folder/trust.pdf", "will.txt"]);
    }
}
