//! Blank government forms — logged-in browsing and download.
//!
//! `GET /portal/forms` lists every vendored form from the registry;
//! `GET /portal/forms/<form_code>.pdf` serves the canonical blank.
//! Both sit inside the `/portal` auth + policy stack — the OPA rule
//! admits any authenticated person, since the blanks are public
//! records. The bytes live only in the public assets bucket: the
//! download pulls them through `cloud::StorageService` and verifies
//! them against the repo's `.sha256` pin, exactly like the fill path —
//! a missing object or a pin mismatch is a loud error, never a
//! fallback.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

/// The two seams this surface needs, extracted [`axum::extract::FromRef`]
/// the full [`crate::admin::AdminState`].
#[derive(Clone)]
pub struct GovFormsState {
    /// Public-assets storage the blanks are pulled from.
    pub assets_storage: Arc<dyn cloud::StorageService>,
    /// The vendored-forms registry (metadata + `.sha256` pins).
    pub forms_registry: Arc<Vec<forms::FormMeta>>,
}

impl axum::extract::FromRef<crate::admin::AdminState> for GovFormsState {
    fn from_ref(s: &crate::admin::AdminState) -> Self {
        Self {
            assets_storage: s.assets_storage.clone(),
            forms_registry: s.forms_registry.clone(),
        }
    }
}

/// `GET /portal/forms` — the vendored-forms index.
pub async fn index_get(State(state): State<GovFormsState>) -> Response {
    let rows: Vec<views::pages::portal::forms::FormRow> = state
        .forms_registry
        .iter()
        .map(|f| views::pages::portal::forms::FormRow {
            code: f.code.to_string(),
            title: f.title.to_string(),
            jurisdiction: f.jurisdiction.to_string(),
            origin_url: f.origin_url.to_string(),
        })
        .collect();
    views::pages::portal::forms::index(&rows).into_response()
}

/// `GET /portal/forms/:file` — download one blank form. `:file` is
/// `<form_code>.pdf`; anything else is a 404.
pub async fn download_get(
    State(state): State<GovFormsState>,
    Path(file): Path<String>,
) -> Response {
    let Some(form_code) = file.strip_suffix(".pdf") else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let Some(form) = state
        .forms_registry
        .iter()
        .find(|f| f.code == form_code)
        .cloned()
    else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let blank = match state.assets_storage.get(form.object_path).await {
        Ok(blank) => blank,
        Err(cloud::StorageError::NotFound(_)) => {
            tracing::error!(
                form_code,
                object_path = form.object_path,
                "gov_forms: blank missing from the assets bucket — run `navigator forms sync`"
            );
            return (StatusCode::BAD_GATEWAY, "blank unavailable").into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, form_code, "gov_forms: assets storage read failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };
    if let Err(e) = form.verify(&blank.bytes) {
        tracing::error!(error = %e, form_code, "gov_forms: blank fails its sha256 pin");
        return (StatusCode::BAD_GATEWAY, "blank fails integrity pin").into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}.pdf\"", form.code),
            ),
        ],
        blank.bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{download_get, index_get, GovFormsState};
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use std::sync::Arc;

    async fn fs_storage(tag: &str) -> Arc<dyn cloud::StorageService> {
        Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join(format!(
                "navigator-gov-forms-{tag}-{}",
                uuid::Uuid::new_v4()
            )))
            .await
            .unwrap(),
        )
    }

    async fn state_with_staged_blanks() -> GovFormsState {
        let assets_storage = fs_storage("staged").await;
        let forms_registry = crate::test_support::stage_blank_forms(assets_storage.as_ref()).await;
        GovFormsState {
            assets_storage,
            forms_registry,
        }
    }

    #[tokio::test]
    async fn downloads_a_pull_verified_blank_as_pdf() {
        let state = state_with_staged_blanks().await;
        let resp = download_get(State(state), Path("nv__llc_formation.pdf".into())).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let headers = resp.headers();
        assert_eq!(headers["content-type"], "application/pdf");
        assert!(headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("nv__llc_formation.pdf"));
    }

    #[tokio::test]
    async fn unknown_codes_and_non_pdf_paths_404() {
        let state = state_with_staged_blanks().await;
        for file in ["nv__annual_list.pdf", "nv__llc_formation", "x.exe"] {
            let resp = download_get(State(state.clone()), Path(file.into())).await;
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{file}");
        }
    }

    #[tokio::test]
    async fn a_missing_bucket_object_is_a_loud_502_not_a_fallback() {
        // Registry pins exist, but nothing was staged in the bucket.
        let state = GovFormsState {
            assets_storage: fs_storage("empty").await,
            forms_registry: Arc::new(forms::registry().unwrap()),
        };
        let resp = download_get(State(state), Path("nv__llc_formation.pdf".into())).await;
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn bytes_failing_the_pin_are_refused() {
        let assets_storage = fs_storage("tampered").await;
        // Stage bytes at the right key that do NOT match the repo pin —
        // a silent re-vendor. The download must refuse to serve them.
        let form = forms::get("nv__llc_formation").unwrap().unwrap();
        assets_storage
            .put(form.object_path, b"%PDF-1.5 re-vendored", "application/pdf")
            .await
            .unwrap();
        let state = GovFormsState {
            assets_storage,
            forms_registry: Arc::new(forms::registry().unwrap()),
        };
        let resp = download_get(State(state), Path("nv__llc_formation.pdf".into())).await;
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn index_lists_every_registry_form() {
        let state = state_with_staged_blanks().await;
        let resp = index_get(State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
