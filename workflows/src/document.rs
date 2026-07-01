//! `document_open__*` step dispatch — render a document and persist it.
//!
//! Mirrors [`crate::email`]'s `email_send__*` dispatch: the caller
//! threads a [`DocumentPayload`] through the signal `value`, and the
//! worker (the `workflows-service` `NotationService` in prod, the
//! in-process [`crate::DispatchingRuntime`] in dev/tests) renders the
//! PDF and persists it via [`cloud::StorageService`] when a transition
//! lands on a `document_open__*` state.
//!
//! Why thread the payload instead of reloading template + answers from
//! the database here: it keeps one data path (the same one EmailSend
//! uses) and keeps this crate free of the intake-side substitution
//! logic. The caller (`web::retainer_walk`) does the templating
//! (template body + answers → Typst source); this step only renders
//! that source and stores the bytes.

use serde::{Deserialize, Serialize};

/// Everything the worker needs to produce and persist one document.
/// Carried (JSON, internally tagged on `kind`) as the `value` of the
/// signal that lands on a `document_open__*` state. Two production
/// modes share one dispatch:
///
/// - [`DocumentPayload::Typst`] — render fresh Typst source to a PDF
///   (the retainer and other generated documents).
/// - [`DocumentPayload::Acroform`] — fill an existing fillable
///   government form (AcroForm) fetched from storage with field values
///   (Nevada SoS articles, IRS 990, …). Output is
///   **attorney-review-ready, never auto-filed** — the workflow spec
///   parks it at `staff_review` before any filing step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentPayload {
    /// Render Typst `typst_source` to a PDF and persist it at
    /// `storage_key`. `typst_source` is the final document source with
    /// every `{{placeholder}}` already resolved by the caller — not the
    /// markdown template body, not the HTML preview.
    Typst {
        storage_key: String,
        typst_source: String,
    },
    /// Fetch the blank form at `blank_form_key`, fill its AcroForm
    /// `/Fields` from `fields` (name → value), **flatten** the result to
    /// static page content, and persist it at `storage_key`. The workflow
    /// spec reaches this fill only after `staff_review`
    /// ([`crate::staff_review_precedes_submission`]), so flattening here
    /// freezes exactly what an attorney approved — no downstream viewer
    /// can re-edit a value before it reaches a government office.
    Acroform {
        storage_key: String,
        blank_form_key: String,
        fields: std::collections::BTreeMap<String, String>,
    },
}

/// Errors from rendering / filling or persisting a document step.
#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("pdf: {0}")]
    Pdf(#[from] pdf::PdfError),
    #[error("storage: {0}")]
    Storage(#[from] cloud::StorageError),
}

/// Produce the document and persist it. The single side effect of a
/// `document_open` step; callers wrap it in `ctx.run` (worker) or call
/// it inline (`DispatchingRuntime`) so it is journaled / idempotent on
/// replay. Idempotent by construction: the same payload writes the same
/// bytes to the same key.
pub async fn dispatch_document_open(
    storage: &dyn cloud::StorageService,
    payload: &DocumentPayload,
) -> Result<(), DocumentError> {
    match payload {
        DocumentPayload::Typst {
            storage_key,
            typst_source,
        } => {
            let bytes = pdf::render(typst_source)?;
            storage.put(storage_key, &bytes, "application/pdf").await?;
        }
        DocumentPayload::Acroform {
            storage_key,
            blank_form_key,
            fields,
        } => {
            let blank = storage.get(blank_form_key).await?.bytes;
            let filled = pdf::fill_acroform(&blank, fields)?;
            // Flatten to static content before persisting: this fill sits
            // past staff_review, so nothing downstream may re-edit the
            // approved values on their way to a government office.
            let bytes = pdf::flatten(&filled)?;
            storage.put(storage_key, &bytes, "application/pdf").await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{dispatch_document_open, DocumentPayload};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    async fn fs_storage() -> Arc<dyn cloud::StorageService> {
        Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join("navigator-document-dispatch-test"))
                .await
                .expect("temp FsStorage"),
        )
    }

    #[tokio::test]
    async fn typst_dispatch_renders_and_persists_a_pdf_at_the_key() {
        let storage = fs_storage().await;
        let payload = DocumentPayload::Typst {
            storage_key: "notations/doc-test/retainer.pdf".into(),
            typst_source: "Hello, retainer.".into(),
        };
        dispatch_document_open(storage.as_ref(), &payload)
            .await
            .expect("dispatch succeeds");

        let stored = storage
            .get("notations/doc-test/retainer.pdf")
            .await
            .expect("object persisted");
        assert_eq!(stored.content_type, "application/pdf");
        // A real PDF starts with the `%PDF` magic bytes.
        assert!(
            stored.bytes.starts_with(b"%PDF"),
            "expected PDF magic bytes, got {:?}",
            &stored.bytes.get(..8)
        );
    }

    #[tokio::test]
    async fn acroform_dispatch_fills_flattens_and_persists_a_form() {
        let storage = fs_storage().await;
        // Stage a blank fillable form in storage, then dispatch a fill.
        let blank = pdf::blank_acroform(&["entity_name"]);
        storage
            .put("forms/nv_articles.pdf", &blank, "application/pdf")
            .await
            .unwrap();

        let mut fields = BTreeMap::new();
        fields.insert("entity_name".to_string(), "Neon Law LLC".to_string());
        let payload = DocumentPayload::Acroform {
            storage_key: "notations/acro-test/nv_articles.pdf".into(),
            blank_form_key: "forms/nv_articles.pdf".into(),
            fields,
        };
        dispatch_document_open(storage.as_ref(), &payload)
            .await
            .expect("acroform dispatch succeeds");

        let stored = storage
            .get("notations/acro-test/nv_articles.pdf")
            .await
            .expect("filled form persisted");
        // The persisted packet is flattened: no interactive fields remain,
        // yet the filled value is still readable as static page content.
        assert!(
            pdf::field_names(&stored.bytes).expect("parses").is_empty(),
            "the filed packet must carry no re-editable form fields"
        );
        assert!(
            pdf::page_text(&stored.bytes)
                .expect("extract text")
                .contains("Neon Law LLC"),
            "the reviewed value must survive as static content"
        );
    }

    #[tokio::test]
    async fn payload_is_internally_tagged_on_kind() {
        // Pin the wire shape so web and the worker stay in sync.
        let typst = serde_json::to_value(DocumentPayload::Typst {
            storage_key: "k".into(),
            typst_source: "s".into(),
        })
        .unwrap();
        assert_eq!(typst["kind"], "typst");
        let acro = serde_json::to_value(DocumentPayload::Acroform {
            storage_key: "k".into(),
            blank_form_key: "b".into(),
            fields: BTreeMap::new(),
        })
        .unwrap();
        assert_eq!(acro["kind"], "acroform");
    }
}
