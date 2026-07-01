//! Admin /projects pages.

use maud::{html, Markup};
use uuid::Uuid;

use crate::components::data_table::{data_table, Column};
use crate::components::form::{Choice, Field, FormCard};
use crate::components::row_actions::RowActions;
use crate::components::sort_spec::SortSpec;
use crate::PageLayout;

pub struct Row<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub status: &'a str,
    pub entity_name: Option<&'a str>,
    /// True when the matter has no onboarding (`onboarding__*`) notation —
    /// it was never opened on a retainer. Surfaced as a warning badge so
    /// the "every matter opens on a retainer" lifecycle gap is visible.
    pub missing_retainer: bool,
    /// True when a `closed` matter has no `closing__letter` notation — it
    /// was closed without an offboarding letter. Always `false` for an
    /// open matter (the letter is only owed at close).
    pub missing_closing_letter: bool,
}

pub struct EntityChoice<'a> {
    pub id: Uuid,
    pub name: &'a str,
}

/// One option in the required client-DRI picker: an existing `Role::Client`
/// person. A matter's client-side DRI must be a real, pre-existing client
/// (the client field exists before the project), so the form selects one
/// rather than conjuring a client mid-create.
pub struct PersonChoice<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub email: &'a str,
}

/// One row in the project detail page's "Documents" table. The list
/// view is intentionally lean — just enough to scan ("which file is
/// which?") and click through. Full provenance lives on the
/// per-document detail page ([`DocumentDetail`]) one click away.
pub struct DocumentRow<'a> {
    pub id: Uuid,
    pub filename: &'a str,
}

/// One per-document detail page. Renders the full provenance off
/// `documents` + `blobs` plus the Download action.
pub struct DocumentDetail<'a> {
    pub project_id: Uuid,
    pub doc_id: Uuid,
    pub filename: &'a str,
    pub kind: &'a str,
    pub source: &'a str,
    pub source_revision_id: Option<&'a str>,
    pub received_at: &'a str,
    pub description: Option<&'a str>,
    pub content_type: &'a str,
    pub byte_size: i64,
    pub sha256_hex: &'a str,
    /// First 12 hex characters of the SHA, surfaced in the listing
    /// view's title.
    pub sha256_short: &'a str,
    /// Pre-built href to the signed-URL redirect endpoint.
    pub download_href: &'a str,
    /// Pre-built href back to the project detail page.
    pub back_href: &'a str,
}

/// One generated estate instrument shown to staff on the matter page.
pub struct EstateDraftRow<'a> {
    pub title: &'a str,
    pub kind: &'a str,
    /// `draft` | `pending_review` | `approved`.
    pub status: &'a str,
}

/// A Northstar estate matter surfaced on the admin matter page so the
/// staff disclosed to it can drive the recorded-sitting flow. The page
/// renders the transcript-upload form while the workflow is at `BEGIN`,
/// lists the generated drafts with the release control at `staff_review`,
/// and shows where the matter stands at every later state.
pub struct EstateMatter<'a> {
    pub notation_id: Uuid,
    /// Current workflow state of the estate notation (e.g. `BEGIN`,
    /// `document_intake__transcript`, `staff_review`, `client_review`).
    pub state: &'a str,
    /// The generated instrument drafts (empty until the sitting is filed).
    pub drafts: &'a [EstateDraftRow<'a>],
}

pub struct Detail<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub status: &'a str,
    pub entity_name: Option<&'a str>,
    /// The staff-side Directly Responsible Individual's name, if designated.
    pub staff_dri: Option<&'a str>,
    /// The client-side Directly Responsible Individual's name, if designated.
    pub client_dri: Option<&'a str>,
    /// The matter's git repo clone URL (`<base>/projects/<id>.git`).
    /// Rendered only in this admin view, which staff and admin reach —
    /// clients get the portal view and never see it (the repo is internal
    /// workspace; the client sees a "Documents" view, never the word
    /// "git"). See `docs/git-project-repos.md`.
    pub clone_url: &'a str,
    pub documents: &'a [DocumentRow<'a>],
    /// Present when this matter is a transcript-driven estate plan. Drives
    /// the Northstar section: the transcript-upload form at `BEGIN`, a
    /// status line otherwise.
    pub estate: Option<EstateMatter<'a>>,
    /// Per-session CSRF token — `None` in tests that don't carry a
    /// session cookie; `Some(t)` rendered into form hidden fields so the
    /// CSRF middleware accepts the POST.
    pub csrf_token: Option<&'a str>,
}

#[derive(Default)]
pub struct Form<'a> {
    pub name: &'a str,
    pub status: &'a str,
    pub entity_id: Option<Uuid>,
    /// The required client-side DRI: which existing `Role::Client` person
    /// this matter is opened for. Echoed back (kept selected) on a
    /// validation re-render. `None` on the edit form, which hides the
    /// picker (the DRI is set at open, not re-chosen on edit).
    pub client_dri_person_id: Option<Uuid>,
    /// The existing client persons offered in the client-DRI picker. Empty
    /// on the edit form, which suppresses the picker.
    pub client_dri_choices: &'a [PersonChoice<'a>],
    /// The matter's scope narrative ("this project's story"). Persisted to
    /// `projects.description`; when a retainer is opened in the same
    /// action it is also seeded as the notation's position-0 custom clause.
    pub description: &'a str,
    pub error: Option<&'a str>,
    /// The retainer block. Every matter opens on a retainer (a project is
    /// not official until one exists), so the create form always requires
    /// an onboarding template; the retainer is sent to the selected client.
    /// The fields below echo back on a validation re-render. Only the create
    /// form renders this block; `edit_form` leaves `retainer_templates`
    /// empty, so the block is hidden.
    pub retainer_template_code: &'a str,
    pub scope_of_services: &'a str,
    /// `(code, label)` pairs for the onboarding-template picker. Empty on
    /// the edit form, which suppresses the whole retainer block.
    pub retainer_templates: &'a [(String, String)],
    /// When set, the pre-matter conflict check raised review-level
    /// findings (listed in `error`) and authorized staff may proceed by
    /// ticking the acknowledgment checkbox this renders. Off on a clean
    /// form and on a hard block (which has no override path).
    pub allow_conflict_override: bool,
}

const STATUSES: &[&str] = &["open", "closed", "archived"];

#[must_use]
pub fn list(rows: &[Row<'_>], csrf_token: &str, sort: &SortSpec) -> Markup {
    let columns = [
        Column::sortable("name", "Name"),
        Column::sortable("status", "Status"),
        Column::sortable("entity_name", "Entity"),
        Column::fixed("actions", ""),
    ];
    let table_rows: Vec<Vec<Markup>> = rows
        .iter()
        .map(|r| {
            let status_cell = html! {
                (r.status)
                @if r.missing_retainer {
                    " "
                    span."badge"."bg-warning"."text-dark"."matter-flag"
                        title="This matter has no onboarding notation — it was never opened on a retainer." {
                        (crate::i18n::t(crate::Locale::En, "portal.no_retainer"))
                    }
                }
                @if r.missing_closing_letter {
                    " "
                    span."badge"."bg-warning"."text-dark"."matter-flag"
                        title="This matter is closed but has no closing letter." {
                        (crate::i18n::t(crate::Locale::En, "portal.no_closing_letter"))
                    }
                }
            };
            vec![
                html! { (r.name) },
                status_cell,
                html! { (r.entity_name.unwrap_or("—")) },
                projects_action_cell(r, csrf_token),
            ]
        })
        .collect();
    let body = html! {
        section.admin { div.container {
            header.page-header {
                h1 { "Projects" }
                p { a class="btn btn-primary" href="/portal/projects/new" { "Add project" } }
            }
            @if rows.is_empty() {
                p.empty { "No projects yet." }
            } @else {
                (data_table(
                    &columns,
                    &table_rows,
                    sort,
                    "/portal/projects",
                    "No projects yet.",
                    &[],
                ))
            }
        } }
    };
    PageLayout::new("Projects — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

fn projects_action_cell(r: &Row<'_>, csrf_token: &str) -> Markup {
    let edit_href = format!("/portal/projects/{}/edit", r.id);
    let delete_action = format!("/portal/projects/{}/delete", r.id);
    let delete_confirm = format!("Delete project {}?", r.name);
    RowActions::new(csrf_token)
        .edit(&edit_href)
        .delete(&delete_action)
        .with_delete_confirm(&delete_confirm)
        .with_row_label(r.name)
        .render()
}

/// The Northstar estate section of the admin matter page. At `BEGIN`
/// it renders the phone-friendly transcript-upload form (text paste,
/// file, or link) that fires the workflow's `transcript_uploaded`
/// signal; at every later state it shows where the matter stands so the
/// staff disclosed to it know what is next. The form is multipart, so —
/// like the document upload above — it carries no `_csrf` field (the
/// CSRF middleware only checks `application/x-www-form-urlencoded`).
fn estate_section(project_id: Uuid, est: &EstateMatter<'_>, csrf_token: Option<&str>) -> Markup {
    let transcript_action = format!(
        "/portal/projects/{project_id}/notations/{}/transcript",
        est.notation_id
    );
    let release_action = format!("/portal/admin/notations/{}/release-drafts", est.notation_id);
    html! {
        section.project-estate {
            h2 { "Estate plan — Northstar" }
            p { "Workflow state: " strong.estate-state { (est.state) } }
            @if est.state == "BEGIN" {
                (FormCard::new(
                    &crate::i18n::t(crate::Locale::En, "portal.file_sitting_transcript"),
                    &transcript_action,
                    "File transcript",
                )
                    .section_heading()
                    .multipart()
                    .intro(html! {
                        "The sitting is recorded offline and transcribed. File it here in "
                        "whichever form you have — you can do this from a phone. Paste the "
                        "transcript text, upload a transcript file, or paste a link to the "
                        "recording."
                    })
                    .fields(vec![
                        Field::textarea("Paste the transcript", "transcript_text", "", 8)
                            .help("Paste the transcribed sitting here."),
                        Field::file("…or upload a transcript file", "file"),
                        Field::text("…or paste a link to the recording", "link", "")
                            .placeholder("https://…")
                            .help("A link to the recording or transcript."),
                    ])
                    .render())
            } @else {
                @if est.drafts.is_empty() {
                    p.muted {
                        "The transcript has been filed. The drafts are being prepared from the "
                        "sitting."
                    }
                } @else {
                    h3 { "Generated drafts" }
                    div.list-group.mb-3 {
                        @for d in est.drafts {
                            div."list-group-item d-flex justify-content-between align-items-center" {
                                span {
                                    (d.title)
                                    span."badge text-bg-light text-uppercase ms-2" { (d.kind) }
                                }
                                span."badge text-bg-secondary text-uppercase" { (d.status) }
                            }
                        }
                    }
                }
                @if est.state == "staff_review" {
                    (FormCard::new(
                        "Approve & release drafts to the client",
                        &release_action,
                        "Release drafts to client",
                    )
                        .section_heading()
                        .intro(html! {
                            "Releasing advances the matter to client review and makes each draft "
                            "readable on the client's review surface. Nothing reaches the client "
                            "until you do this."
                        })
                        .csrf(csrf_token.unwrap_or(""))
                        .render())
                } @else if est.state == "client_review" {
                    p.muted {
                        "Released to the client. Waiting for the client to read each draft and "
                        "approve the plan."
                    }
                }
            }
        }
    }
}

/// Project detail page — header info, documents table, and the
/// multipart upload form. The upload form posts to the existing
/// `/portal/projects/:id/documents/upload` endpoint and is
/// `enctype="multipart/form-data"` so byte payloads go through
/// untouched. CSRF is intentionally not threaded into the upload
/// form: the CSRF middleware short-circuits on multipart bodies
/// (only `application/x-www-form-urlencoded` is checked).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn detail(d: &Detail<'_>) -> Markup {
    let upload_action = format!("/portal/projects/{}/documents/upload", d.id);
    let edit_href = format!("/portal/projects/{}/edit", d.id);
    // CSRF is intentionally not threaded into the upload form: the
    // CSRF middleware short-circuits on multipart bodies (it only
    // checks `application/x-www-form-urlencoded`).
    let upload = FormCard::new("Upload a document", &upload_action, "Upload")
        .section_heading()
        .multipart()
        .fields(vec![
            Field::file("File", "file").required(),
            Field::text("Kind", "kind", "")
                .placeholder("intake")
                .help("Optional — defaults to `unclassified`."),
            Field::text("Description", "description", "")
                .placeholder("Letter from Acme Bank dated 2026-05-23")
                .help("Optional."),
        ]);
    let body = html! {
        section.admin { div.container {
            header.page-header {
                h1 { (d.name) }
                p {
                    "Status: " (d.status) " · "
                    "Entity: " (d.entity_name.unwrap_or("—")) " · "
                    "Staff DRI: " (d.staff_dri.unwrap_or("—")) " · "
                    "Client DRI: " (d.client_dri.unwrap_or("—")) " · "
                    a href=(edit_href) { "Edit project" }
                }
            }

            @if let Some(est) = &d.estate {
                (estate_section(d.id, est, d.csrf_token))
            }

            section.project-repo {
                h2 { "Repository" }
                p.help {
                    "This matter's append-only document repository. Clone it with a "
                    "personal access token (staff only)."
                }
                p { code.clone-url { (d.clone_url) } }
            }

            section.project-documents {
                h2 { "Documents" }
                @if d.documents.is_empty() {
                    p.empty { "No documents yet." }
                } @else {
                    table.admin-table {
                        thead { tr { th { "Filename" } th { "Download" } } }
                        tbody {
                            @for r in d.documents {
                                tr {
                                    td {
                                        a href={
                                            "/portal/projects/" (d.id)
                                            "/documents/" (r.id)
                                        } { (r.filename) }
                                    }
                                    td {
                                        a href={
                                            "/portal/projects/" (d.id)
                                            "/documents/" (r.id) "/download"
                                        } { "Download" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            section.project-upload {
                (upload.render())
            }

            // Close the matter — opens the firm-signed closing-letter
            // walk. Shown only while the matter is open; the status
            // flips to `closed` when the firm signs the letter. Posts to
            // the staff CRUD surface (`/portal/admin/...`).
            @if d.status == "open" {
                section.project-close {
                    (FormCard::new(
                        "Close this matter",
                        &format!("/portal/admin/projects/{}/close", d.id),
                        "Close matter",
                    )
                        .section_heading()
                        .intro(html! {
                            "Open the closing-letter walk. Neon Law drafts and signs a "
                            "closing letter; signing it marks the matter complete."
                        })
                        .csrf(d.csrf_token.unwrap_or(""))
                        .render())
                }
            }
        } }
    };
    PageLayout::new(&format!("{} — Project", d.name))
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// Per-document detail page — full provenance + the Download
/// action. Reached from the project detail page via the filename
/// link. Pre-built `download_href` and `back_href` so the view
/// stays UUID-formatting-agnostic.
#[must_use]
pub fn document_detail(d: &DocumentDetail<'_>) -> Markup {
    let body = html! {
        section.admin { div.container {
            header.page-header {
                h1 { (d.filename) }
                p { a href=(d.back_href) { "← Back to project" } }
            }

            section.document-actions {
                p {
                    a class="btn btn-primary" href=(d.download_href) { "Download" }
                    " "
                    span.muted { "(signed link valid for one hour)" }
                }
            }

            section.document-provenance {
                h2 { "Provenance" }
                dl.admin-dl {
                    dt { "Source" }            dd { (d.source) }
                    dt { "Source revision" }   dd.mono {
                        (d.source_revision_id.unwrap_or("—"))
                    }
                    dt { "Received" }          dd { (d.received_at) }
                    dt { "Description" }       dd { (d.description.unwrap_or("—")) }
                }
            }

            section.document-storage {
                h2 { "Storage" }
                dl.admin-dl {
                    dt { "Kind" }              dd { (d.kind) }
                    dt { "Content type" }      dd { (d.content_type) }
                    dt { "Bytes" }             dd { (d.byte_size) }
                    dt { "SHA-256" }           dd.mono { (d.sha256_hex) }
                }
            }
        } }
    };
    PageLayout::new(&format!("{} — Document", d.filename))
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[must_use]
pub fn new_form(f: &Form<'_>, entities: &[EntityChoice<'_>]) -> Markup {
    form_page("Add project", "/portal/projects", "Create", f, entities)
}

#[must_use]
pub fn edit_form(id: Uuid, f: &Form<'_>, entities: &[EntityChoice<'_>]) -> Markup {
    form_page(
        "Edit project",
        &format!("/portal/projects/{id}"),
        "Save",
        f,
        entities,
    )
}

#[allow(clippy::too_many_lines)]
fn form_page(
    title: &str,
    action: &str,
    submit: &str,
    f: &Form<'_>,
    entities: &[EntityChoice<'_>],
) -> Markup {
    let entity_ids: Vec<String> = entities.iter().map(|e| e.id.to_string()).collect();
    let selected_entity = f.entity_id.map(|id| id.to_string());
    // Hoisted so the borrowed strings outlive `fields` (which holds the
    // `Choice` borrows until the form is rendered at the end).
    let client_ids: Vec<String> = f
        .client_dri_choices
        .iter()
        .map(|c| c.id.to_string())
        .collect();
    let selected_client = f.client_dri_person_id.map(|id| id.to_string());
    let status_opts: Vec<Choice<'_>> = STATUSES.iter().map(|&s| Choice::new(s, s)).collect();
    let mut entity_opts = vec![Choice::new("", "—")];
    entity_opts.extend(
        entity_ids
            .iter()
            .zip(entities)
            .map(|(id, e)| Choice::new(id, e.name)),
    );

    let mut fields = vec![
        Field::text("Name", "name", f.name).required(),
        Field::select("Status", "status", status_opts, Some(f.status)).required(),
        Field::select(
            "Entity (optional)",
            "entity_id",
            entity_opts,
            selected_entity.as_deref(),
        ),
        Field::textarea("Description", "description", f.description, 3).help(
            "Optional — this matter's scope narrative (\"the project's story\"). When you \
             send a retainer, it is seeded as the agreement's first custom clause for you \
             to edit at review.",
        ),
    ];

    // The required client-side DRI: pick the existing client this matter is
    // for. Rendered only on the create form (the edit form leaves
    // `client_dri_choices` empty). The client must already exist — create
    // the client person first if they don't.
    if !f.client_dri_choices.is_empty() {
        let mut client_opts = vec![Choice::new("", "— pick the client —")];
        client_opts.extend(
            client_ids
                .iter()
                .zip(f.client_dri_choices)
                .map(|(id, c)| Choice::new(id, c.name)),
        );
        fields.push(
            Field::select(
                "Client",
                "client_dri_person_id",
                client_opts,
                selected_client.as_deref(),
            )
            .required()
            .help(
                "The client this matter is for — its client-side Directly \
                 Responsible Individual. Create the client person first if they \
                 aren't listed.",
            ),
        );
    }

    // The retainer block: every matter opens on a retainer (a project is
    // not official until one exists), so opening the matter always creates
    // the retainer and routes it through attorney review toward e-signature,
    // sent to the selected client. Rendered only on the create form (the
    // edit form leaves `retainer_templates` empty, hiding the block); the
    // onboarding template is required.
    if !f.retainer_templates.is_empty() {
        let mut template_opts = vec![Choice::new("", "— pick an onboarding template —")];
        template_opts.extend(
            f.retainer_templates
                .iter()
                .map(|(code, label)| Choice::new(code, label)),
        );
        let selected_template =
            (!f.retainer_template_code.is_empty()).then_some(f.retainer_template_code);
        fields.push(
            Field::select(
                "Retainer template",
                "retainer_template_code",
                template_opts,
                selected_template,
            )
            .required()
            .help(
                "Every matter opens on a retainer. Opening creates it and routes \
                 it through attorney review toward e-signature, sent to the client \
                 selected above.",
            ),
        );
        fields.push(
            Field::textarea(
                "Scope of services",
                "scope_of_services",
                f.scope_of_services,
                3,
            )
            .help(
                "Optional — describes the work this retainer covers; rendered into the agreement.",
            ),
        );
    }

    // Conflict-check override: rendered only when the check raised
    // review-level findings (the findings themselves are in `f.error`).
    // Ticking it re-submits with `conflict_ack=1`, which the handler
    // records to the relationship log as a staff override. A hard block
    // never sets this flag — there is no override for a blocking
    // conflict.
    if f.allow_conflict_override {
        fields.push(
            Field::checkbox(
                "I have reviewed the conflict findings above and am authorized to open this matter",
                "conflict_ack",
                "1",
                false,
            )
            .required(),
        );
    }

    let body = html! {
        section.admin { div.container {
            (FormCard::new(title, action, submit)
                .fields(fields)
                .error(f.error)
                .cancel("/portal/projects")
                .render())
        } }
    };
    PageLayout::new(title)
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{detail, edit_form, list, new_form, Detail, DocumentRow, EntityChoice, Form, Row};
    use crate::components::sort_spec::{SortDirection, SortSpec};
    use uuid::Uuid;

    const ID1: Uuid = Uuid::from_u128(1);
    const ID2: Uuid = Uuid::from_u128(2);
    const ID3: Uuid = Uuid::from_u128(3);

    #[test]
    fn list_renders_status_and_entity_name() {
        let rows = [Row {
            id: ID1,
            name: "Audit",
            status: "open",
            entity_name: Some("Acme"),
            missing_retainer: false,
            missing_closing_letter: false,
        }];
        let html = list(&rows, "TOK", &SortSpec::default()).into_string();
        assert!(html.contains("Audit"));
        assert!(html.contains("open"));
        assert!(html.contains(">Acme<"));
    }

    #[test]
    fn list_does_not_render_id_column() {
        let rows = [Row {
            id: ID1,
            name: "Audit",
            status: "open",
            entity_name: None,
            missing_retainer: false,
            missing_closing_letter: false,
        }];
        let html = list(&rows, "TOK", &SortSpec::default()).into_string();
        assert!(
            !html.contains("<th>ID</th>"),
            "ID column header should be gone, got: {html}",
        );
    }

    #[test]
    fn list_renders_sortable_headers() {
        let rows = [Row {
            id: ID1,
            name: "Audit",
            status: "open",
            entity_name: Some("Acme"),
            missing_retainer: false,
            missing_closing_letter: false,
        }];
        let html = list(&rows, "TOK", &SortSpec::default()).into_string();
        assert!(html.contains("href=\"/portal/projects?sort=name\""));
        assert!(html.contains("href=\"/portal/projects?sort=status\""));
        assert!(html.contains("href=\"/portal/projects?sort=entity_name\""));
    }

    #[test]
    fn list_active_sort_descending_arrow_renders() {
        let rows = [Row {
            id: ID1,
            name: "Audit",
            status: "open",
            entity_name: Some("Acme"),
            missing_retainer: false,
            missing_closing_letter: false,
        }];
        let html = list(
            &rows,
            "TOK",
            &SortSpec::single("status", SortDirection::Descending),
        )
        .into_string();
        assert!(html.contains("↓"));
    }

    #[test]
    fn list_renders_row_actions_with_csrf_and_named_confirm() {
        let rows = [Row {
            id: ID1,
            name: "Audit",
            status: "open",
            entity_name: None,
            missing_retainer: false,
            missing_closing_letter: false,
        }];
        let html = list(&rows, "SESSION_TOKEN", &SortSpec::default()).into_string();
        // Pencil + filled-trash glyphs.
        assert!(html.contains("class=\"bi bi-pencil-square\""));
        assert!(html.contains("class=\"bi bi-trash3-fill\""));
        // CSRF threaded into the delete form.
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"SESSION_TOKEN\""));
        // Confirm prompt echoes the project name.
        assert!(html.contains("Delete project Audit?"));
    }

    #[test]
    fn detail_renders_project_header_and_upload_form_to_correct_route() {
        let html = detail(&Detail {
            id: ID2,
            clone_url: "https://nav.test/projects/x.git",
            name: "Sison Trust",
            status: "open",
            entity_name: Some("Acme"),
            staff_dri: None,
            client_dri: None,
            documents: &[],
            estate: None,
            csrf_token: None,
        })
        .into_string();
        assert!(html.contains("Sison Trust"));
        assert!(html.contains("Status: open"));
        assert!(html.contains("Entity: Acme"));
        assert!(html.contains(&format!(
            "action=\"/portal/projects/{ID2}/documents/upload\""
        )));
        assert!(html.contains("enctype=\"multipart/form-data\""));
        assert!(html.contains(&format!("href=\"/portal/projects/{ID2}/edit\"")));
        assert!(html.contains("No documents yet."));
    }

    #[test]
    fn detail_renders_the_repository_clone_url() {
        let html = detail(&Detail {
            id: ID2,
            clone_url: "https://nav.test/projects/abc.git",
            name: "Sison Trust",
            status: "open",
            entity_name: Some("Acme"),
            staff_dri: None,
            client_dri: None,
            documents: &[],
            estate: None,
            csrf_token: None,
        })
        .into_string();
        assert!(html.contains("Repository"));
        assert!(
            html.contains("https://nav.test/projects/abc.git"),
            "the admin project view must surface the git clone URL"
        );
    }

    #[test]
    fn detail_upload_form_includes_description_field() {
        let html = detail(&Detail {
            id: ID2,
            clone_url: "https://nav.test/projects/x.git",
            name: "Acme Formation",
            status: "open",
            entity_name: None,
            staff_dri: None,
            client_dri: None,
            documents: &[],
            estate: None,
            csrf_token: None,
        })
        .into_string();
        assert!(
            html.contains("name=\"description\""),
            "upload form must include a description input"
        );
    }

    #[test]
    fn detail_lists_documents_with_links_when_present() {
        let docs = [DocumentRow {
            id: ID1,
            filename: "intake.pdf",
        }];
        let html = detail(&Detail {
            id: ID2,
            clone_url: "https://nav.test/projects/x.git",
            name: "Acme Formation",
            status: "open",
            entity_name: None,
            staff_dri: None,
            client_dri: None,
            documents: &docs,
            estate: None,
            csrf_token: None,
        })
        .into_string();
        assert!(html.contains("intake.pdf"));
        // Filename links to per-document detail.
        assert!(html.contains(&format!("href=\"/portal/projects/{ID2}/documents/{ID1}\"")));
        // Download link points to the signed-URL redirect endpoint.
        assert!(html.contains(&format!(
            "href=\"/portal/projects/{ID2}/documents/{ID1}/download\""
        )));
        assert!(!html.contains("No documents yet."));
        // The lean table must NOT spill provenance into the list — that
        // belongs on the per-document detail page.
        assert!(!html.contains("SHA-256"));
        assert!(!html.contains("Source revision"));
    }

    #[test]
    fn document_detail_renders_provenance_storage_and_download_link() {
        use super::{document_detail, DocumentDetail};
        let html = document_detail(&DocumentDetail {
            project_id: ID2,
            doc_id: ID1,
            filename: "engagement-letter.pdf",
            kind: "retainer",
            source: "drive_sync",
            source_revision_id: Some("rev-001"),
            received_at: "2026-05-26T12:00:01Z",
            description: Some("Initial sync"),
            content_type: "application/pdf",
            byte_size: 2_048,
            sha256_hex: "deadbeefcafe1234567890abcdef0000",
            sha256_short: "deadbeefcafe",
            download_href: "/portal/projects/X/documents/Y/download",
            back_href: "/portal/projects/X",
        })
        .into_string();
        assert!(html.contains("engagement-letter.pdf"));
        assert!(html.contains("Provenance"));
        assert!(html.contains("Storage"));
        assert!(html.contains("drive_sync"));
        assert!(html.contains("rev-001"));
        assert!(html.contains("Initial sync"));
        assert!(html.contains("application/pdf"));
        assert!(html.contains("2048"));
        assert!(html.contains("deadbeefcafe1234567890abcdef0000"));
        assert!(html.contains("href=\"/portal/projects/X/documents/Y/download\""));
        assert!(html.contains("href=\"/portal/projects/X\""));
    }

    #[test]
    fn detail_renders_close_button_only_while_matter_is_open() {
        // Open → the close walk is offered, posting to the staff route.
        let open = detail(&Detail {
            id: ID2,
            clone_url: "https://nav.test/projects/x.git",
            name: "Open matter",
            status: "open",
            entity_name: None,
            staff_dri: None,
            client_dri: None,
            documents: &[],
            estate: None,
            csrf_token: Some("CSRF-TOKEN"),
        })
        .into_string();
        assert!(open.contains("Close matter"));
        assert!(open.contains(&format!("action=\"/portal/admin/projects/{ID2}/close\"")));
        assert!(open.contains("name=\"_csrf\" value=\"CSRF-TOKEN\""));

        // Already closed → no close button (idempotent surface).
        let closed = detail(&Detail {
            id: ID2,
            clone_url: "https://nav.test/projects/x.git",
            name: "Closed matter",
            status: "closed",
            entity_name: None,
            staff_dri: None,
            client_dri: None,
            documents: &[],
            estate: None,
            csrf_token: Some("CSRF-TOKEN"),
        })
        .into_string();
        assert!(!closed.contains("Close matter"));
        assert!(!closed.contains("/close\""));
    }

    #[test]
    fn estate_at_begin_renders_the_multipart_transcript_form() {
        use super::EstateMatter;
        let nid = Uuid::from_u128(42);
        let html = detail(&Detail {
            id: ID2,
            clone_url: "https://nav.test/projects/x.git",
            name: "Capricorn estate plan",
            status: "open",
            entity_name: None,
            staff_dri: None,
            client_dri: None,
            documents: &[],
            estate: Some(EstateMatter {
                notation_id: nid,
                state: "BEGIN",
                drafts: &[],
            }),
            csrf_token: None,
        })
        .into_string();
        assert!(html.contains("Estate plan — Northstar"));
        assert!(html.contains("File the sitting transcript"));
        // Posts to the shipped transcript handler, multipart.
        assert!(html.contains(&format!(
            "action=\"/portal/projects/{ID2}/notations/{nid}/transcript\""
        )));
        assert!(html.contains("enctype=\"multipart/form-data\""));
        // The three phone-friendly capture modes.
        assert!(html.contains("name=\"transcript_text\""));
        assert!(html.contains("name=\"file\""));
        assert!(html.contains("name=\"link\""));
        // Never instruct the client to scan a PDF (client-council rule).
        assert!(!html.to_lowercase().contains("scan"));
    }

    #[test]
    fn estate_at_staff_review_lists_drafts_and_offers_the_release_control() {
        use super::{EstateDraftRow, EstateMatter};
        let nid = Uuid::from_u128(42);
        let html = detail(&Detail {
            id: ID2,
            clone_url: "https://nav.test/projects/x.git",
            name: "Capricorn estate plan",
            status: "open",
            entity_name: None,
            staff_dri: None,
            client_dri: None,
            documents: &[],
            estate: Some(EstateMatter {
                notation_id: nid,
                state: "staff_review",
                drafts: &[
                    EstateDraftRow {
                        title: "Last Will and Testament",
                        kind: "will",
                        status: "draft",
                    },
                    EstateDraftRow {
                        title: "Revocable Living Trust",
                        kind: "trust",
                        status: "draft",
                    },
                ],
            }),
            csrf_token: Some("CSRF-TOKEN"),
        })
        .into_string();
        assert!(html.contains("Estate plan — Northstar"));
        assert!(html.contains("staff_review"));
        // The generated drafts are listed for the attorney.
        assert!(html.contains("Last Will and Testament"));
        assert!(html.contains("Revocable Living Trust"));
        // The release control posts to the staff route with CSRF.
        assert!(html.contains(&format!(
            "action=\"/portal/admin/notations/{nid}/release-drafts\""
        )));
        assert!(html.contains("name=\"_csrf\" value=\"CSRF-TOKEN\""));
        // No transcript form once the matter has moved past BEGIN.
        assert!(!html.contains("File the sitting transcript"));
        assert!(!html.contains("/transcript\""));
    }

    #[test]
    fn estate_at_client_review_shows_waiting_not_the_release_control() {
        use super::{EstateDraftRow, EstateMatter};
        let html = detail(&Detail {
            id: ID2,
            clone_url: "https://nav.test/projects/x.git",
            name: "Capricorn estate plan",
            status: "open",
            entity_name: None,
            staff_dri: None,
            client_dri: None,
            documents: &[],
            estate: Some(EstateMatter {
                notation_id: Uuid::from_u128(42),
                state: "client_review",
                drafts: &[EstateDraftRow {
                    title: "Last Will and Testament",
                    kind: "will",
                    status: "pending_review",
                }],
            }),
            csrf_token: Some("CSRF-TOKEN"),
        })
        .into_string();
        assert!(html.contains("Waiting for the client"));
        assert!(!html.contains("release-drafts"));
    }

    #[test]
    fn non_estate_matter_renders_no_northstar_section() {
        let html = detail(&Detail {
            id: ID2,
            clone_url: "https://nav.test/projects/x.git",
            name: "Acme Formation",
            status: "open",
            entity_name: None,
            staff_dri: None,
            client_dri: None,
            documents: &[],
            estate: None,
            csrf_token: None,
        })
        .into_string();
        assert!(!html.contains("Estate plan — Northstar"));
    }

    #[test]
    fn forms_target_routes_and_render_dropdowns() {
        let entities = [EntityChoice {
            id: ID1,
            name: "Acme",
        }];
        assert!(new_form(&Form::default(), &entities)
            .into_string()
            .contains("action=\"/portal/projects\""));
        let edited = edit_form(
            ID3,
            &Form {
                name: "P",
                status: "closed",
                entity_id: Some(ID1),
                error: None,
                ..Default::default()
            },
            &entities,
        )
        .into_string();
        assert!(edited.contains(&format!("action=\"/portal/projects/{ID3}\"")));
        assert!(edited.contains("<option value=\"closed\" selected>closed</option>"));
        assert!(edited.contains(&format!("<option value=\"{ID1}\" selected>Acme</option>")));
    }
}
