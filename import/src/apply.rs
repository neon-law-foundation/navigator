//! Apply a validated [`Payload`] to the database — find-or-create the
//! organizations as `entities`, the people as `persons`, and the
//! `person_entity_roles` links between them. Every step is idempotent
//! and reported per row; a referenced `entity_type` or `jurisdiction`
//! that does not exist fails only that row, never the whole batch.

use std::collections::HashMap;

use anyhow::anyhow;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use serde::Serialize;
use uuid::Uuid;

use crate::contract::Payload;
use crate::validate::{canonical_url, validate, Diagnostic, Severity};
use store::entity::{entity, entity_type, jurisdiction, person, person_entity_role};

/// What happened to one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// A new row was inserted.
    Created,
    /// An existing row was found and at least one field changed.
    Updated,
    /// An existing row was found and nothing changed.
    Unchanged,
    /// The row could not be applied (reason in `detail`).
    Failed,
}

/// The result of applying one organization or person.
#[derive(Debug, Clone, Serialize)]
pub struct RowOutcome {
    /// The payload `key` this outcome is for.
    pub key: String,
    pub status: Outcome,
    /// The database id, when the row was created/updated/unchanged.
    pub id: Option<Uuid>,
    /// A failure reason, or a note (e.g. a skipped link).
    pub detail: Option<String>,
}

/// The whole import result. Returned even when structural validation
/// rejects the payload (then `organizations`/`people` are empty and the
/// reason is in `diagnostics`).
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub diagnostics: Vec<Diagnostic>,
    pub organizations: Vec<RowOutcome>,
    pub people: Vec<RowOutcome>,
}

impl ImportReport {
    /// `true` if any structural error blocked the apply, or any row failed.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
            || self
                .organizations
                .iter()
                .chain(&self.people)
                .any(|r| r.status == Outcome::Failed)
    }

    /// Count rows (orgs + people) with the given outcome.
    #[must_use]
    pub fn count(&self, status: Outcome) -> usize {
        self.organizations
            .iter()
            .chain(&self.people)
            .filter(|r| r.status == status)
            .count()
    }

    /// One-line tally for logs and CLI output.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} created, {} updated, {} unchanged, {} failed",
            self.count(Outcome::Created),
            self.count(Outcome::Updated),
            self.count(Outcome::Unchanged),
            self.count(Outcome::Failed),
        )
    }

    /// A human-readable, multi-line list of everything that went wrong or
    /// is worth flagging — every structural [`Diagnostic`] (errors AND
    /// warnings) followed by every per-row `detail` (a failure reason or a
    /// note like a skipped link) — or `None` when the import was wholly
    /// clean.
    ///
    /// This exists because the structured `diagnostics` / `RowOutcome.detail`
    /// fields are invisible on surfaces that render only text: the
    /// `aida_bulk_import` MCP/A2A `content` Part (Gemini Enterprise shows
    /// that text and drops the structured payload) and the CLI. Folding the
    /// detail into one block is what lets a caller see *why* `0 created`
    /// instead of a silent, message-less non-result.
    #[must_use]
    pub fn problem_lines(&self) -> Option<String> {
        let mut lines = Vec::new();
        for d in &self.diagnostics {
            let label = match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            lines.push(format!("• {} ({label}): {}", d.pointer, d.message));
        }
        for (kind, rows) in [
            ("organization", &self.organizations),
            ("person", &self.people),
        ] {
            for row in rows {
                match (&row.status, &row.detail) {
                    (Outcome::Failed, detail) => lines.push(format!(
                        "• {kind} `{}` failed: {}",
                        row.key,
                        detail.as_deref().unwrap_or("unknown reason")
                    )),
                    // A non-failed row can still carry a note (e.g. a
                    // person created but whose org link was skipped).
                    (_, Some(note)) => lines.push(format!("• {kind} `{}`: {note}", row.key)),
                    (_, None) => {}
                }
            }
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

/// Validate, then apply. If structural validation finds any error,
/// nothing is written and the diagnostics are returned. Otherwise every
/// organization and person is find-or-created and a per-row report comes
/// back. Each row's outcome is also emitted as an OTel/tracing event so
/// the import history lands in telemetry.
pub async fn apply(db: &DatabaseConnection, payload: &Payload) -> anyhow::Result<ImportReport> {
    let diagnostics = validate(payload);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        tracing::warn!(
            target: "import",
            errors = diagnostics.iter().filter(|d| d.severity == Severity::Error).count(),
            "bulk_import rejected: validation errors",
        );
        return Ok(ImportReport {
            diagnostics,
            organizations: Vec::new(),
            people: Vec::new(),
        });
    }

    let mut organizations = Vec::with_capacity(payload.organizations.len());
    let mut entity_by_key: HashMap<&str, Uuid> = HashMap::new();
    for org in &payload.organizations {
        let outcome = match upsert_entity(db, org).await {
            Ok((id, status)) => {
                entity_by_key.insert(org.key.as_str(), id);
                RowOutcome {
                    key: org.key.clone(),
                    status,
                    id: Some(id),
                    detail: None,
                }
            }
            Err(e) => RowOutcome {
                key: org.key.clone(),
                status: Outcome::Failed,
                id: None,
                detail: Some(e.to_string()),
            },
        };
        tracing::info!(
            target: "import",
            kind = "organization",
            key = %outcome.key,
            status = ?outcome.status,
            id = ?outcome.id,
            "bulk_import row",
        );
        organizations.push(outcome);
    }

    let mut people = Vec::with_capacity(payload.people.len());
    for record in &payload.people {
        let outcome = match upsert_person(db, record).await {
            Ok((person_id, status)) => {
                let detail = match entity_by_key.get(record.organization.as_str()) {
                    Some(&entity_id) => {
                        link_person_entity(db, person_id, entity_id, &record.entity_role)
                            .await
                            .err()
                            .map(|e| format!("link failed: {e}"))
                    }
                    None => Some(format!(
                        "organization `{}` was not created; link skipped",
                        record.organization
                    )),
                };
                RowOutcome {
                    key: record.key.clone(),
                    status,
                    id: Some(person_id),
                    detail,
                }
            }
            Err(e) => RowOutcome {
                key: record.key.clone(),
                status: Outcome::Failed,
                id: None,
                detail: Some(e.to_string()),
            },
        };
        tracing::info!(
            target: "import",
            kind = "person",
            key = %outcome.key,
            status = ?outcome.status,
            id = ?outcome.id,
            "bulk_import row",
        );
        people.push(outcome);
    }

    let report = ImportReport {
        diagnostics,
        organizations,
        people,
    };
    tracing::info!(
        target: "import",
        source = payload.source.as_deref().unwrap_or("(none)"),
        summary = %report.summary(),
        "bulk_import complete",
    );
    Ok(report)
}

/// Find-or-create one `entities` row, keyed on
/// `(name, entity_type_id, jurisdiction_id)`. Resolves the entity-type
/// name and jurisdiction code to their ids; an unknown one is an error
/// for this row.
async fn upsert_entity(
    db: &DatabaseConnection,
    org: &crate::contract::OrgRecord,
) -> anyhow::Result<(Uuid, Outcome)> {
    let entity_type = entity_type::Entity::find()
        .filter(entity_type::Column::Name.eq(org.entity_type.trim()))
        .one(db)
        .await?
        .ok_or_else(|| anyhow!("unknown entity_type `{}`", org.entity_type.trim()))?;

    let code = org.jurisdiction.trim().to_ascii_uppercase();
    let jurisdiction = jurisdiction::Entity::find()
        .filter(jurisdiction::Column::Code.eq(code.as_str()))
        .one(db)
        .await?
        .ok_or_else(|| anyhow!("unknown jurisdiction code `{code}`"))?;

    let url = match &org.url {
        Some(raw) => Some(canonical_url(raw).map_err(|e| anyhow!(e))?),
        None => None,
    };
    let phone = clean(org.phone.as_deref());
    let name = org.name.trim();

    let existing = entity::Entity::find()
        .filter(entity::Column::Name.eq(name))
        .filter(entity::Column::EntityTypeId.eq(entity_type.id))
        .filter(entity::Column::JurisdictionId.eq(jurisdiction.id))
        .one(db)
        .await?;

    if let Some(row) = existing {
        let mut active: entity::ActiveModel = row.clone().into();
        let mut changed = false;
        // Payload wins when it carries a value; an absent field never
        // erases what's already stored.
        let next_phone = phone.or_else(|| row.phone.clone());
        if row.phone != next_phone {
            active.phone = Set(next_phone);
            changed = true;
        }
        let next_url = url.or_else(|| row.url.clone());
        if row.url != next_url {
            active.url = Set(next_url);
            changed = true;
        }
        if changed {
            let updated = active.update(db).await?;
            Ok((updated.id, Outcome::Updated))
        } else {
            Ok((row.id, Outcome::Unchanged))
        }
    } else {
        let inserted = entity::ActiveModel {
            name: Set(name.to_string()),
            entity_type_id: Set(entity_type.id),
            jurisdiction_id: Set(jurisdiction.id),
            phone: Set(phone),
            url: Set(url),
            ..Default::default()
        }
        .insert(db)
        .await?;
        Ok((inserted.id, Outcome::Created))
    }
}

/// Find-or-create one `persons` row, keyed on the unique `email`. On a
/// re-import the payload is authoritative for `name`/`title`/`phone`,
/// but `role` is never touched — a person promoted to staff/admin stays
/// promoted. New rows take the database default `role` (`client`).
async fn upsert_person(
    db: &DatabaseConnection,
    record: &crate::contract::PersonRecord,
) -> anyhow::Result<(Uuid, Outcome)> {
    let email = record.email.trim();
    let name = record.name.trim();
    let title = clean(record.title.as_deref());
    let phone = clean(record.phone.as_deref());

    let existing = person::Entity::find()
        .filter(person::Column::Email.eq(email))
        .one(db)
        .await?;

    if let Some(row) = existing {
        let mut active: person::ActiveModel = row.clone().into();
        let mut changed = false;
        if row.name != name {
            active.name = Set(name.to_string());
            changed = true;
        }
        let next_title = title.or_else(|| row.title.clone());
        if row.title != next_title {
            active.title = Set(next_title);
            changed = true;
        }
        let next_phone = phone.or_else(|| row.phone.clone());
        if row.phone != next_phone {
            active.phone = Set(next_phone);
            changed = true;
        }
        if changed {
            let updated = active.update(db).await?;
            Ok((updated.id, Outcome::Updated))
        } else {
            Ok((row.id, Outcome::Unchanged))
        }
    } else {
        let inserted = person::ActiveModel {
            name: Set(name.to_string()),
            email: Set(email.to_string()),
            title: Set(title),
            phone: Set(phone),
            ..Default::default()
        }
        .insert(db)
        .await?;
        Ok((inserted.id, Outcome::Created))
    }
}

/// Find-or-create the `person_entity_roles` link. Returns `Ok(())`
/// whether the link already existed or was inserted — the table has no
/// unique constraint on the triple, so the engine enforces it.
async fn link_person_entity(
    db: &DatabaseConnection,
    person_id: Uuid,
    entity_id: Uuid,
    role: &str,
) -> anyhow::Result<()> {
    let role = role.trim();
    let already = person_entity_role::Entity::find()
        .filter(person_entity_role::Column::PersonId.eq(person_id))
        .filter(person_entity_role::Column::EntityId.eq(entity_id))
        .filter(person_entity_role::Column::Role.eq(role))
        .one(db)
        .await?
        .is_some();
    if already {
        return Ok(());
    }
    person_entity_role::ActiveModel {
        person_id: Set(person_id),
        entity_id: Set(entity_id),
        role: Set(role.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

/// Trim an optional string and treat empty as absent.
fn clean(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::{ImportReport, Outcome, RowOutcome};
    use crate::validate::{Diagnostic, Severity};
    use uuid::Uuid;

    #[test]
    fn problem_lines_is_none_for_a_clean_report() {
        let report = ImportReport {
            diagnostics: Vec::new(),
            organizations: vec![RowOutcome {
                key: "njp".into(),
                status: Outcome::Created,
                id: Some(Uuid::nil()),
                detail: None,
            }],
            people: Vec::new(),
        };
        assert!(report.problem_lines().is_none());
    }

    #[test]
    fn problem_lines_surfaces_diagnostics_failures_and_notes() {
        // The exact shape Gemini Enterprise would otherwise drop: a
        // structural error, a warning, a failed row, and a created row
        // that still carries a note. All four must appear in the text.
        let report = ImportReport {
            diagnostics: vec![
                Diagnostic {
                    severity: Severity::Error,
                    pointer: "people[0].email".into(),
                    message: "`bob@` is not a valid email address".into(),
                },
                Diagnostic {
                    severity: Severity::Warning,
                    pointer: "organizations[0].url".into(),
                    message: "url canonicalized to `https://njp.org`".into(),
                },
            ],
            organizations: vec![RowOutcome {
                key: "njp".into(),
                status: Outcome::Failed,
                id: None,
                detail: Some("unknown jurisdiction code `XX`".into()),
            }],
            people: vec![RowOutcome {
                key: "abigail".into(),
                status: Outcome::Created,
                id: Some(Uuid::nil()),
                detail: Some("organization `njp` was not created; link skipped".into()),
            }],
        };
        let text = report.problem_lines().expect("problems present");
        assert!(
            text.contains("people[0].email (error): `bob@` is not a valid email address"),
            "missing email error: {text}"
        );
        assert!(
            text.contains("organizations[0].url (warning): url canonicalized"),
            "missing url warning: {text}"
        );
        assert!(
            text.contains("organization `njp` failed: unknown jurisdiction code `XX`"),
            "missing row failure: {text}"
        );
        assert!(
            text.contains("person `abigail`: organization `njp` was not created; link skipped"),
            "missing row note: {text}"
        );
    }
}
