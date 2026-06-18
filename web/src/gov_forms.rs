//! Blank government forms — logged-in browsing and download.
//!
//! `GET /portal/forms` lists every vendored form from the `forms`
//! registry (the bytes the repo committed and the workflows fill);
//! `GET /portal/forms/<form_code>.pdf` serves the canonical blank.
//! Both sit inside the `/portal` auth + policy stack — the OPA rule
//! admits any authenticated person, since the blanks are public
//! records — and the bytes come from the bundled registry, so the
//! download is exactly what the provenance ledger pins, with no
//! bucket round-trip.

use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

/// `GET /portal/forms` — the vendored-forms index.
pub async fn index_get() -> Response {
    let forms = match forms::registry() {
        Ok(forms) => forms,
        Err(e) => {
            tracing::error!(error = %e, "gov_forms: registry failed to load");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };
    let rows: Vec<views::pages::portal::forms::FormRow> = forms
        .iter()
        .map(|f| views::pages::portal::forms::FormRow {
            form_code: f.meta.form_code.clone(),
            name: f.meta.name.clone(),
            authority: f.meta.authority.clone(),
            revision: f.meta.revision.clone(),
            retrieved: f.meta.retrieved.clone(),
            source_url: f.meta.source_url.clone(),
        })
        .collect();
    views::pages::portal::forms::index(&rows).into_response()
}

/// `GET /portal/forms/:file` — download one blank form. `:file` is
/// `<form_code>.pdf`; anything else is a 404.
pub async fn download_get(Path(file): Path<String>) -> Response {
    let Some(form_code) = file.strip_suffix(".pdf") else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let form = match forms::get(form_code) {
        Ok(Some(form)) => form,
        Ok(None) => return (StatusCode::NOT_FOUND, "not found").into_response(),
        Err(e) => {
            tracing::error!(error = %e, form_code, "gov_forms: registry failed to load");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };
    (
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}-{}.pdf\"",
                    form.meta.form_code, form.meta.revision
                ),
            ),
        ],
        form.bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::download_get;
    use axum::extract::Path;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn downloads_a_vendored_blank_as_pdf() {
        let resp = download_get(Path("nv_sos__llc_formation.pdf".into())).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let headers = resp.headers();
        assert_eq!(headers["content-type"], "application/pdf");
        assert!(headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("nv_sos__llc_formation-2023-08.pdf"));
    }

    #[tokio::test]
    async fn unknown_codes_and_non_pdf_paths_404() {
        for file in ["nv_sos__annual_list.pdf", "nv_sos__llc_formation", "x.exe"] {
            let resp = download_get(Path(file.into())).await;
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{file}");
        }
    }
}
