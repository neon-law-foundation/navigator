//! The migrated generic read-only admin listings (#641 Phase 3, admin cluster).
//!
//! Each page is a thin pair built on [`crate::admin_listing`]: a `#[server]`
//! function that gates, reads, and projects its rows, and a component that
//! renders the result through [`crate::admin_listing::render_resource`]. The
//! chrome, table, and loading/error states live once in `admin_listing`; only
//! the read and the projection are per-page.
//!
//! Server-only entity paths stay fully qualified inside the `#[server]` bodies
//! so the wasm client build (which stubs those bodies) carries no unused
//! `store`/`SeaORM` imports.

use dioxus::prelude::*;

use crate::admin_listing::{render_resource, AdminListingView};

/// The `?sort=` a sortable listing reads back to render its header direction.
#[derive(serde::Deserialize, Default)]
pub struct SortQuery {
    #[serde(default)]
    pub sort: Option<String>,
}

/// Read the validated `?sort=` for a sortable listing. The route's pre-handler
/// has already rejected an unadvertised key with a `400`, so whatever arrives
/// here is safe to order by.
#[cfg(feature = "server")]
async fn requested_sort() -> String {
    dioxus_fullstack_core::FullstackContext::extract::<axum::extract::Query<SortQuery>, _>()
        .await
        .ok()
        .and_then(|axum::extract::Query(q)| q.sort)
        .unwrap_or_default()
}

/// Lawyer jurisdictions directory — the reference table of jurisdictions
/// (name + code), ordered by code as the page was. Gate first, then read,
/// then project.
#[server]
pub async fn list_jurisdictions() -> Result<AdminListingView, ServerFnError> {
    // Gate before touching the query, so a non-lawyer caller never
    // triggers it.
    let role = crate::admin_listing::require_lawyer().await?;

    let surreal = consume_context::<store::surreal::SurrealDb>();
    let mut rows = store::jurisdictions::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    rows.sort_by(|a, b| a.code.cmp(&b.code));

    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Jurisdictions",
        "Jurisdictions",
        &["Name", "Code"],
        rows.into_iter().map(|j| vec![j.name, j.code]).collect(),
    )
    .await)
}

/// Lawyer jurisdictions directory component.
#[component]
pub fn LawyerJurisdictions() -> Element {
    let resource = use_server_future(list_jurisdictions)?;
    render_resource(&resource)
}

/// Lawyer git-repositories directory — the tracked repositories (remote hash +
/// last commit SHA). Gate first, then read, then project.
#[server]
pub async fn list_git_repositories() -> Result<AdminListingView, ServerFnError> {
    // Gate before touching the query, so a non-lawyer caller never
    // triggers it.
    let role = crate::admin_listing::require_lawyer().await?;

    let surreal = consume_context::<store::surreal::SurrealDb>();
    let rows = store::git_repositories::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Git repositories",
        "Git repositories",
        &["Remote hash", "Last commit SHA"],
        rows.into_iter()
            .map(|g| vec![g.remote_hash, g.last_commit_sha])
            .collect(),
    )
    .await)
}

/// Lawyer git-repositories directory component.
#[component]
pub fn LawyerGitRepositories() -> Element {
    let resource = use_server_future(list_git_repositories)?;
    render_resource(&resource)
}

/// Lawyer person-entity roles directory — the person↔entity role assignments
/// (person, entity, role).
///
/// The ties live in the `entity_role` relation (ENG-120). Gate first, then
/// read, then project.
#[server]
pub async fn list_person_entity_roles() -> Result<AdminListingView, ServerFnError> {
    // Gate before touching the query, so a non-lawyer caller never
    // triggers it.
    let role = crate::admin_listing::require_lawyer().await?;

    let surreal = consume_context::<store::surreal::SurrealDb>();
    let rows = store::entity_roles::all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Person-entity roles",
        "Person-entity roles",
        &["Person", "Entity", "Role"],
        rows.into_iter()
            .map(|tie| {
                vec![
                    tie.person_id.to_string(),
                    tie.entity_id.to_string(),
                    tie.role,
                ]
            })
            .collect(),
    )
    .await)
}

/// Lawyer person-entity roles directory component.
#[component]
pub fn LawyerPersonEntityRoles() -> Element {
    let resource = use_server_future(list_person_entity_roles)?;
    render_resource(&resource)
}

/// Lawyer notations directory. Gate first, then read, then project.
#[server]
pub async fn list_notations() -> Result<AdminListingView, ServerFnError> {
    // Gate before touching the query, so a non-lawyer caller never
    // triggers it.
    let role = crate::admin_listing::require_lawyer().await?;

    let surreal = consume_context::<store::surreal::SurrealDb>();
    let rows = store::notations::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Notations",
        "Notations",
        &["Template", "Person", "Entity", "State"],
        rows.into_iter()
            .map(|n| {
                vec![
                    n.template_id.to_string(),
                    n.person_id.to_string(),
                    n.entity_id.map_or("—".into(), |x| x.to_string()),
                    n.state,
                ]
            })
            .collect(),
    )
    .await)
}

/// Lawyer notations directory component.
#[component]
pub fn LawyerNotations() -> Element {
    let resource = use_server_future(list_notations)?;
    render_resource(&resource)
}

/// Lawyer answers directory. Gate first, then read, then project.
#[server]
pub async fn list_answers() -> Result<AdminListingView, ServerFnError> {
    // Gate before touching the query, so a non-lawyer caller never
    // triggers it.
    let role = crate::admin_listing::require_lawyer().await?;

    let surreal = consume_context::<store::surreal::SurrealDb>();
    let rows = store::answers::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Answers",
        "Answers",
        &["Question", "Person", "Value"],
        rows.into_iter()
            .map(|a| {
                vec![
                    a.question_id.to_string(),
                    a.person_id.to_string(),
                    store::answers::display_value(&a.value),
                ]
            })
            .collect(),
    )
    .await)
}

/// Lawyer answers directory component.
#[component]
pub fn LawyerAnswers() -> Element {
    let resource = use_server_future(list_answers)?;
    render_resource(&resource)
}

/// Lawyer addresses directory. The table lives in `SurrealDB` (ENG-20).
/// Gate first, then read, then project.
#[server]
pub async fn list_addresses() -> Result<AdminListingView, ServerFnError> {
    // Gate before touching the query, so a non-lawyer caller never
    // triggers it.
    let role = crate::admin_listing::require_lawyer().await?;

    let surreal = consume_context::<store::surreal::SurrealDb>();
    let rows = store::addresses::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Addresses",
        "Addresses",
        &["Owner", "Line 1", "City", "Region", "Country"],
        rows.into_iter()
            .map(|a| {
                let owner = a.person_id.map_or_else(
                    || a.entity_id.map_or("—".into(), |id| format!("entity/{id}")),
                    |id| format!("person/{id}"),
                );
                vec![owner, a.line1, a.city, a.region, a.country]
            })
            .collect(),
    )
    .await)
}

/// Lawyer addresses directory component.
#[component]
pub fn LawyerAddresses() -> Element {
    let resource = use_server_future(list_addresses)?;
    render_resource(&resource)
}

/// Lawyer assets directory.
#[server]
pub async fn list_assets() -> Result<AdminListingView, ServerFnError> {
    // Gate before touching the query, so a non-lawyer caller never
    // triggers it.
    let role = crate::admin_listing::require_lawyer().await?;

    let surreal = consume_context::<store::surreal::SurrealDb>();
    let rows = store::assets::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Assets",
        "Assets",
        &[
            "Storage key",
            "Filename",
            "Kind",
            "Content type",
            "Bytes",
            "SHA-256",
        ],
        rows.into_iter()
            .map(|a| {
                vec![
                    a.storage_key,
                    a.filename.unwrap_or_default(),
                    a.kind.unwrap_or_default(),
                    a.content_type,
                    a.byte_size.to_string(),
                    a.sha256_hex,
                ]
            })
            .collect(),
    )
    .await)
}

/// Lawyer assets directory component.
#[component]
pub fn LawyerAssets() -> Element {
    let resource = use_server_future(list_assets)?;
    render_resource(&resource)
}

/// Lawyer person-project roles directory.
#[server]
pub async fn list_person_project_roles() -> Result<AdminListingView, ServerFnError> {
    let role = crate::admin_listing::require_lawyer().await?;
    let surreal = consume_context::<store::surreal::SurrealDb>();
    let rows = store::projects::all_participations(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Person-project roles",
        "Person-project roles",
        &["Person", "Project", "Participation"],
        rows.into_iter()
            .map(|r| {
                vec![
                    r.person_id.to_string(),
                    r.project_id.to_string(),
                    r.participation,
                ]
            })
            .collect(),
    )
    .await)
}

/// Lawyer person-project roles directory component.
#[component]
pub fn LawyerPersonProjectRoles() -> Element {
    let resource = use_server_future(list_person_project_roles)?;
    render_resource(&resource)
}

/// Lawyer disclosures directory.
#[server]
pub async fn list_disclosures() -> Result<AdminListingView, ServerFnError> {
    crate::admin_listing::load_surreal(
        |surreal| async move {
            store::disclosures::all(&surreal)
                .await
                .map_err(|e| ServerFnError::new(e.to_string()))
        },
        "Lawyer | Disclosures",
        "Disclosures",
        &["Entity", "Project", "Kind", "Summary"],
        |d| {
            vec![
                d.entity_id.map_or("—".into(), |x| x.to_string()),
                d.project_id.map_or("—".into(), |x| x.to_string()),
                d.kind,
                d.summary,
            ]
        },
    )
    .await
}

/// Lawyer disclosures directory component.
#[component]
pub fn LawyerDisclosures() -> Element {
    let resource = use_server_future(list_disclosures)?;
    render_resource(&resource)
}

/// Lawyer relationship logs directory.
///
/// The trail reads newest-first. Gate first, then read, then project.
#[server]
pub async fn list_relationship_logs() -> Result<AdminListingView, ServerFnError> {
    // Gate before touching the query, so a non-lawyer caller never
    // triggers it.
    let role = crate::admin_listing::require_lawyer().await?;

    let surreal = consume_context::<store::surreal::SurrealDb>();
    let rows = store::relationship_logs::all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Relationship logs",
        "Relationship logs",
        &["Actor", "Subject type", "Subject", "Action", "Detail"],
        rows.into_iter()
            .map(|log| {
                vec![
                    log.actor_person_id
                        .map_or("—".into(), |actor| actor.to_string()),
                    log.subject_type,
                    log.subject_id.to_string(),
                    log.action,
                    log.detail,
                ]
            })
            .collect(),
    )
    .await)
}

/// Lawyer relationship logs directory component.
#[component]
pub fn LawyerRelationshipLogs() -> Element {
    let resource = use_server_future(list_relationship_logs)?;
    render_resource(&resource)
}

/// Lawyer mailrooms directory. Each row resolves its address through an in-memory
/// join (the handler did the same), so it builds rows itself and hands them
/// to `admin_listing::view` rather than the single-entity `load`.
#[server]
pub async fn list_mailrooms() -> Result<AdminListingView, ServerFnError> {
    let role = crate::admin_listing::require_lawyer().await?;
    let surreal = consume_context::<store::surreal::SurrealDb>();
    let mailrooms = store::mailrooms::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    // Both tables are single-engine now, so the in-memory join is one
    // engine's data rather than two.
    let addresses = store::addresses::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let by_address = |id| {
        addresses.iter().find(|a| a.id == id).map_or_else(
            || format!("(unknown address #{id})"),
            |a| format!("{}, {}, {}", a.line1, a.city, a.region),
        )
    };
    let rows = mailrooms
        .into_iter()
        .map(|m| vec![m.name, by_address(m.address_id)])
        .collect();
    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Mailrooms",
        "Mailrooms",
        &["Name", "Address"],
        rows,
    )
    .await)
}

/// Lawyer mailrooms directory component.
#[component]
pub fn LawyerMailrooms() -> Element {
    let resource = use_server_future(list_mailrooms)?;
    render_resource(&resource)
}

/// Lawyer letters directory. Each row resolves its mailroom through an in-memory
/// join, so it builds rows itself and hands them to `admin_listing::view`.
#[server]
pub async fn list_letters() -> Result<AdminListingView, ServerFnError> {
    let role = crate::admin_listing::require_lawyer().await?;
    let surreal = consume_context::<store::surreal::SurrealDb>();
    let letters = store::letters::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    // Both tables are single-engine now, so the in-memory join is one
    // engine's data rather than two.
    let mailrooms = store::mailrooms::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let by_mailroom = |id| {
        mailrooms
            .iter()
            .find(|m| m.id == id)
            .map_or_else(|| format!("(unknown #{id})"), |m| m.name.clone())
    };
    let rows = letters
        .into_iter()
        .map(|l| {
            vec![
                by_mailroom(l.mailroom_id),
                l.direction,
                l.sender,
                l.recipient,
                l.summary,
            ]
        })
        .collect();
    Ok(crate::admin_listing::view(
        role,
        "Lawyer | Letters",
        "Letters",
        &["Mailroom", "Direction", "Sender", "Recipient", "Summary"],
        rows,
    )
    .await)
}

/// Lawyer letters directory component.
#[component]
pub fn LawyerLetters() -> Element {
    let resource = use_server_future(list_letters)?;
    render_resource(&resource)
}

/// The email-log `?page=` query. 1-indexed; defaults to page 1.
#[derive(serde::Deserialize, Default)]
pub struct EmailLogQuery {
    #[serde(default)]
    pub page: Option<u64>,
}

/// How many `sent_emails` rows the email log shows per page.
#[cfg(feature = "server")]
const EMAIL_LOG_PER_PAGE: u64 = 50;

/// Lawyer email log — a read-only, `?page=`-paginated audit view over
/// `sent_emails`, newest first, metadata only (the body is intentionally not
/// shown). Unlike the other listings it carries pagination, so it sets the
/// view's `PageState` after building its rows.
#[server]
pub async fn list_email_log() -> Result<AdminListingView, ServerFnError> {
    let role = crate::admin_listing::require_lawyer().await?;
    let axum::extract::Query(query) =
        dioxus_fullstack_core::FullstackContext::extract::<axum::extract::Query<EmailLogQuery>, _>(
        )
        .await?;
    let requested_page = query.page.unwrap_or(1).max(1);

    let db = consume_context::<store::surreal::SurrealDb>();
    // The count, the clamp, and the fetch are one statement batch inside
    // `store::sent_emails::page`, which SurrealDB runs as one transaction, so
    // all three read one snapshot. Without it, a row logged between the count
    // and the fetch pushes
    // onto page 1 and shifts the rest down, stranding the oldest row on an
    // unreachable page N+1 under a pager that shows no Next link.
    let store::sent_emails::Page {
        rows: rows_raw,
        total_pages,
        page,
    } = store::sent_emails::page(&db, requested_page, EMAIL_LOG_PER_PAGE)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let rows = rows_raw
        .into_iter()
        .map(|r| {
            vec![
                r.sent_at.to_rfc3339(),
                r.recipient,
                r.subject,
                r.sender,
                r.template_slug.unwrap_or_else(|| "—".to_string()),
                r.outcome,
            ]
        })
        .collect();

    let mut view = crate::admin_listing::view(
        role,
        "Lawyer | Email log",
        "Email log",
        &[
            "Sent at",
            "Recipient",
            "Subject",
            "From",
            "Template",
            "Outcome",
        ],
        rows,
    )
    .await;
    view.subtitle = Some(
        "Every outbound message that went through the SendGrid path. Gmail mail \
         from Workspace mailboxes is intentionally not logged here."
            .to_string(),
    );
    view.pagination = Some(crate::admin_listing::PageState {
        current: u32::try_from(page).unwrap_or(u32::MAX),
        total: u32::try_from(total_pages).unwrap_or(u32::MAX),
        base_path: "/lawyer/email-log".to_string(),
    });
    Ok(view)
}

/// Lawyer email log component.
#[component]
pub fn LawyerEmailLog() -> Element {
    let resource = use_server_future(list_email_log)?;
    render_resource(&resource)
}

/// Lawyer templates catalog — the public template catalog (code / title /
/// respondent type), sortable by code, title, and respondent type.
///
/// Project-scoped templates are deliberately hidden: this is the shared
/// catalog, and a matter's own templates belong to that matter.
#[server]
pub async fn list_templates() -> Result<AdminListingView, ServerFnError> {
    // Gate before touching the query, so a non-lawyer caller never
    // triggers it.
    let role = crate::admin_listing::require_lawyer().await?;
    let sort = requested_sort().await;

    let surreal = consume_context::<store::surreal::SurrealDb>();
    // `list_current` is already the shared catalog plus every Project's own
    // current rows; the filter below drops the scoped ones, which is what
    // the `project_id IS NULL` predicate did.
    let rows = store::templates::list_current(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(crate::admin_listing::sorted_view(
        role,
        "Lawyer | Templates",
        "Templates",
        &["Code", "Title", "Respondent type"],
        &crate::admin_listing::PortedSort {
            keys: &["code", "title", "respondent_type"],
            active: &sort,
            base_path: "/lawyer/templates",
        },
        rows.into_iter()
            .filter(|t| t.project_id.is_none())
            .map(|t| vec![t.code, t.title, t.respondent_type])
            .collect(),
    )
    .await)
}

/// Lawyer templates catalog component.
#[component]
pub fn LawyerTemplates() -> Element {
    let resource = use_server_future(list_templates)?;
    render_resource(&resource)
}

/// Lawyer questions directory — the seeded questionnaire questions, sortable by
/// code and answer type.
///
/// Questions are seeded from template frontmatter by `cli import`, so this is a
/// transparency surface only: no add / edit / delete.
/// The view is assembled through [`crate::admin_listing::sorted_view`]: gate
/// first, then read, then project, then order.
#[server]
pub async fn list_questions() -> Result<AdminListingView, ServerFnError> {
    // Gate before touching the query, so a non-lawyer caller never
    // triggers it.
    let role = crate::admin_listing::require_lawyer().await?;
    let sort = requested_sort().await;

    let surreal = consume_context::<store::surreal::SurrealDb>();
    let rows = store::questions::list_all(&surreal)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(crate::admin_listing::sorted_view(
        role,
        "Lawyer | Questions",
        "Questions",
        &["Code", "Prompt", "Answer type"],
        &crate::admin_listing::PortedSort {
            // The prompt is free prose; sorting by it says nothing useful,
            // so its key is empty and the header stays fixed.
            keys: &["code", "", "answer_type"],
            active: &sort,
            base_path: "/lawyer/questions",
        },
        rows.into_iter()
            .map(|q| vec![q.code, q.prompt, q.answer_type])
            .collect(),
    )
    .await)
}

/// Lawyer questions directory component.
#[component]
pub fn LawyerQuestions() -> Element {
    let resource = use_server_future(list_questions)?;
    render_resource(&resource)
}
