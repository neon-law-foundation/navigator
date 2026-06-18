//! Admin playbook CRUD — `/portal/admin/playbooks`.
//!
//! A **playbook** is a client Entity's set of negotiating positions, the
//! yardstick the inbound-contract review measures a third-party contract
//! against (see [`crate::contract_review_walk`]). This surface lets an
//! attorney create a playbook for a Company and edit its positions.
//!
//! Positions are entered as one textarea, one position per line,
//! pipe-delimited: `topic | preferred | fallback | walk-away | severity`.
//! [`parse_positions`] / [`positions_to_text`] are the round-trip between
//! that text and [`store::playbooks::Position`].

use axum::extract::{Extension, Form, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

use sea_orm::EntityTrait;
use store::entity::{entity, playbook};
use store::playbooks::{self, Position, SEVERITY_HIGH, SEVERITY_LOW, SEVERITY_MEDIUM};
use store::Db;
use views::pages::admin::playbooks as views_playbooks;

use crate::admin::csrf_token;
use crate::session::SessionData;

/// `?sort=` query for the list view.
#[derive(Deserialize)]
pub struct ListQuery {
    sort: Option<String>,
}

/// `GET /portal/admin/playbooks` — every playbook, by Company then name.
pub async fn index(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    Query(q): Query<ListQuery>,
) -> Response {
    use views::components::SortSpec;
    let allowed: std::collections::HashSet<&str> = ["entity", "name"].into_iter().collect();
    let sort = match SortSpec::parse(q.sort.as_deref()).validated(&allowed) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let token = csrf_token(session.as_deref());

    let playbooks = playbook::Entity::find().all(&db).await.unwrap_or_default();
    let names = entity_name_map(&db).await;

    let mut rows: Vec<(String, String, usize, bool, Uuid)> = playbooks
        .iter()
        .map(|p| {
            let count = playbooks::positions_of(p).map_or(0, |v| v.len());
            let entity_name = names
                .get(&p.entity_id)
                .cloned()
                .unwrap_or_else(|| "(unknown)".to_string());
            (entity_name, p.name.clone(), count, p.active, p.id)
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let view_rows: Vec<views_playbooks::PlaybookRow<'_>> = rows
        .iter()
        .map(
            |(entity_name, name, count, active, id)| views_playbooks::PlaybookRow {
                id: *id,
                entity_name,
                name,
                position_count: *count,
                active: *active,
            },
        )
        .collect();
    views_playbooks::list(&view_rows, token, &sort).into_response()
}

/// `GET /portal/admin/playbooks/new` — the create form.
pub async fn new_form(State(db): State<Db>, session: Option<Extension<SessionData>>) -> Response {
    let token = csrf_token(session.as_deref());
    render_new(&db, &views_playbooks::PlaybookForm::default(), token).await
}

#[derive(Deserialize)]
pub struct CreateInput {
    entity_id: Uuid,
    name: String,
    positions: String,
}

/// `POST /portal/admin/playbooks` — create a playbook for a Company.
pub async fn create(
    State(db): State<Db>,
    session: Option<Extension<SessionData>>,
    Form(input): Form<CreateInput>,
) -> Response {
    let token = csrf_token(session.as_deref());
    if input.name.trim().is_empty() {
        return reload_new(&db, &input, "A playbook name is required.", token).await;
    }
    let positions = match parse_positions(&input.positions) {
        Ok(p) if p.is_empty() => {
            return reload_new(&db, &input, "Enter at least one position.", token).await
        }
        Ok(p) => p,
        Err(e) => return reload_new(&db, &input, &e, token).await,
    };
    match playbooks::create(
        &db,
        &playbooks::NewPlaybook {
            entity_id: input.entity_id,
            name: input.name.trim(),
            positions: &positions,
        },
    )
    .await
    {
        Ok(_) => Redirect::to("/portal/admin/playbooks").into_response(),
        Err(e) if store::is_unique_violation(&e) => {
            reload_new(
                &db,
                &input,
                "That Company already has a playbook with that name.",
                token,
            )
            .await
        }
        Err(e) => {
            tracing::error!(error = %e, "admin: create playbook failed");
            reload_new(&db, &input, "Could not create the playbook.", token).await
        }
    }
}

/// `GET /portal/admin/playbooks/:id/edit` — edit positions.
pub async fn edit_form(
    State(db): State<Db>,
    Path(id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let token = csrf_token(session.as_deref());
    let Some(row) = playbooks::by_id(&db, id).await.ok().flatten() else {
        return (StatusCode::NOT_FOUND, views::not_found_page()).into_response();
    };
    let positions = playbooks::positions_of(&row).unwrap_or_default();
    let text = positions_to_text(&positions);
    let entity_name = entity_name_map(&db)
        .await
        .get(&row.entity_id)
        .cloned()
        .unwrap_or_else(|| "(unknown)".to_string());
    views_playbooks::edit_form(
        id,
        &views_playbooks::PlaybookForm {
            name: &row.name,
            entity_id: Some(row.entity_id),
            positions_text: &text,
            error: None,
        },
        &entity_name,
        token,
    )
    .into_response()
}

#[derive(Deserialize)]
pub struct UpdateInput {
    positions: String,
}

/// `POST /portal/admin/playbooks/:id` — replace the position set.
pub async fn update(
    State(db): State<Db>,
    Path(id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
    Form(input): Form<UpdateInput>,
) -> Response {
    let token = csrf_token(session.as_deref());
    let Some(row) = playbooks::by_id(&db, id).await.ok().flatten() else {
        return (StatusCode::NOT_FOUND, views::not_found_page()).into_response();
    };
    let positions = match parse_positions(&input.positions) {
        Ok(p) if p.is_empty() => {
            return reload_edit(
                id,
                &row,
                &input.positions,
                "Enter at least one position.",
                token,
            )
        }
        Ok(p) => p,
        Err(e) => return reload_edit(id, &row, &input.positions, &e, token),
    };
    match playbooks::update_positions(&db, id, &positions).await {
        Ok(()) => Redirect::to("/portal/admin/playbooks").into_response(),
        Err(e) => {
            tracing::error!(error = %e, %id, "admin: update playbook positions failed");
            reload_edit(
                id,
                &row,
                &input.positions,
                "Could not save the positions.",
                token,
            )
        }
    }
}

// --- rendering helpers -----------------------------------------------------

async fn render_new(db: &Db, form: &views_playbooks::PlaybookForm<'_>, token: &str) -> Response {
    let entities = entity::Entity::find().all(db).await.unwrap_or_default();
    let choices: Vec<views_playbooks::EntityChoice<'_>> = entities
        .iter()
        .map(|e| views_playbooks::EntityChoice {
            id: e.id,
            name: &e.name,
        })
        .collect();
    views_playbooks::new_form(form, &choices, token).into_response()
}

async fn reload_new(db: &Db, input: &CreateInput, error: &str, token: &str) -> Response {
    render_new(
        db,
        &views_playbooks::PlaybookForm {
            name: &input.name,
            entity_id: Some(input.entity_id),
            positions_text: &input.positions,
            error: Some(error),
        },
        token,
    )
    .await
}

fn reload_edit(
    id: Uuid,
    row: &playbook::Model,
    positions_text: &str,
    error: &str,
    token: &str,
) -> Response {
    views_playbooks::edit_form(
        id,
        &views_playbooks::PlaybookForm {
            name: &row.name,
            entity_id: Some(row.entity_id),
            positions_text,
            error: Some(error),
        },
        &row.name,
        token,
    )
    .into_response()
}

async fn entity_name_map(db: &Db) -> HashMap<Uuid, String> {
    entity::Entity::find()
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|e| (e.id, e.name))
        .collect()
}

// --- positions <-> textarea ------------------------------------------------

/// Render a position set back into the pipe-delimited textarea form.
#[must_use]
pub fn positions_to_text(positions: &[Position]) -> String {
    positions
        .iter()
        .map(|p| {
            format!(
                "{} | {} | {} | {} | {}",
                p.topic, p.preferred, p.fallback, p.walkaway, p.severity
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse the textarea into a position set. One position per non-blank line,
/// five `|`-separated fields, the last a valid severity. Returns a
/// user-facing error string naming the offending line.
///
/// # Errors
///
/// A line without exactly five fields, an empty topic, or an unrecognised
/// severity.
pub fn parse_positions(text: &str) -> Result<Vec<Position>, String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('|').map(str::trim).collect();
        if parts.len() != 5 {
            return Err(format!(
                "Line {}: expected 5 fields separated by '|' (topic | preferred | fallback | \
                 walk-away | severity), got {}.",
                i + 1,
                parts.len()
            ));
        }
        if parts[0].is_empty() {
            return Err(format!("Line {}: the topic is required.", i + 1));
        }
        let severity = parts[4].to_lowercase();
        if ![SEVERITY_LOW, SEVERITY_MEDIUM, SEVERITY_HIGH].contains(&severity.as_str()) {
            return Err(format!(
                "Line {}: severity must be low, medium, or high (got \"{}\").",
                i + 1,
                parts[4]
            ));
        }
        out.push(Position {
            topic: parts[0].to_string(),
            preferred: parts[1].to_string(),
            fallback: parts[2].to_string(),
            walkaway: parts[3].to_string(),
            severity,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{parse_positions, positions_to_text};
    use store::playbooks::{Position, SEVERITY_HIGH};

    #[test]
    fn parses_well_formed_lines_and_normalises_severity() {
        let text = "Liability | mutual cap | 2x fees | uncapped | HIGH\n\
                    Governing law | Nevada | Delaware | no nexus | medium";
        let positions = parse_positions(text).unwrap();
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0].topic, "Liability");
        assert_eq!(positions[0].walkaway, "uncapped");
        assert_eq!(positions[0].severity, SEVERITY_HIGH);
        assert_eq!(positions[1].severity, "medium");
    }

    #[test]
    fn blank_lines_are_skipped() {
        let text = "\nLiability | a | b | c | low\n\n";
        assert_eq!(parse_positions(text).unwrap().len(), 1);
    }

    #[test]
    fn wrong_field_count_is_rejected_with_line_number() {
        let err = parse_positions("Liability | a | b | high").unwrap_err();
        assert!(err.contains("Line 1"));
        assert!(err.contains("5 fields"));
    }

    #[test]
    fn unknown_severity_is_rejected() {
        let err = parse_positions("Liability | a | b | c | critical").unwrap_err();
        assert!(err.contains("severity must be"));
    }

    #[test]
    fn round_trips_through_text() {
        let positions = vec![Position {
            topic: "Term".into(),
            preferred: "1 year".into(),
            fallback: "2 years".into(),
            walkaway: "perpetual".into(),
            severity: "medium".into(),
        }];
        let text = positions_to_text(&positions);
        assert_eq!(parse_positions(&text).unwrap(), positions);
    }
}
