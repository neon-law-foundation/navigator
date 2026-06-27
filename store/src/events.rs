//! Event existence + registration, synced from the Markdown source.
//!
//! The Markdown files under `web/content/events/` are the display source
//! of truth. This module is the narrow seam between that Markdown and the
//! database: [`sync_from_markdown`] reconciles the `events` table to the
//! current Markdown set (upsert by `slug`, hard-delete what left), and
//! [`register`] appends a registrant email to a published event.
//!
//! Two data-minimization rules are load-bearing here:
//!
//! - A sync **never** overwrites an event's `registrations` — the
//!   Markdown does not carry registrants, so an update preserves them.
//! - [`register`] stores **only** the email, and only on a published
//!   (non-draft) event.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};
use std::collections::HashMap;

use crate::entity::event;

/// Kind of event. Maps to the CHECK-constrained `events.event_type`
/// text column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventType {
    ShowAndTell,
}

impl EventType {
    /// The database string for this variant.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShowAndTell => "show_and_tell",
        }
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for EventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "show_and_tell" => Ok(Self::ShowAndTell),
            other => Err(format!("unknown event_type: {other}")),
        }
    }
}

/// One event's facts, as parsed from its Markdown file. The natural key
/// is `slug`. `registrations` is intentionally absent — the sync never
/// touches it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventSyncInput {
    pub slug: String,
    pub public_slug: String,
    pub event_type: EventType,
    pub starts_at: chrono::NaiveDateTime,
    pub ends_at: chrono::NaiveDateTime,
    pub timezone: String,
    pub draft: bool,
}

/// What [`sync_from_markdown`] did, for logging / reporting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
}

/// Reconcile the `events` table to `inputs`.
///
/// Each input is upserted by `slug`: a new slug is inserted (with an
/// empty `registrations`), an existing slug is updated in place while its
/// `registrations` are **preserved**. Every row whose slug is absent from
/// `inputs` is **hard-deleted** — we do not keep events that left the
/// Markdown (data minimization).
pub async fn sync_from_markdown(
    db: &impl ConnectionTrait,
    inputs: &[EventSyncInput],
) -> Result<SyncReport, sea_orm::DbErr> {
    let mut by_slug: HashMap<String, event::Model> = event::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|m| (m.slug.clone(), m))
        .collect();

    let mut report = SyncReport::default();

    for input in inputs {
        if let Some(existing) = by_slug.remove(&input.slug) {
            // Update in place. Preserve `registrations` by leaving that
            // field Unchanged. Let the behavior macro bump `updated_at`.
            let mut active: event::ActiveModel = existing.into();
            active.public_slug = ActiveValue::Set(input.public_slug.clone());
            active.event_type = ActiveValue::Set(input.event_type.as_str().to_owned());
            active.starts_at = ActiveValue::Set(input.starts_at);
            active.ends_at = ActiveValue::Set(input.ends_at);
            active.timezone = ActiveValue::Set(input.timezone.clone());
            active.draft = ActiveValue::Set(input.draft);
            active.updated_at = ActiveValue::NotSet;
            active.update(db).await?;
            report.updated += 1;
        } else {
            event::ActiveModel {
                slug: ActiveValue::Set(input.slug.clone()),
                public_slug: ActiveValue::Set(input.public_slug.clone()),
                event_type: ActiveValue::Set(input.event_type.as_str().to_owned()),
                starts_at: ActiveValue::Set(input.starts_at),
                ends_at: ActiveValue::Set(input.ends_at),
                timezone: ActiveValue::Set(input.timezone.clone()),
                draft: ActiveValue::Set(input.draft),
                registrations: ActiveValue::Set(Vec::new()),
                ..Default::default()
            }
            .insert(db)
            .await?;
            report.created += 1;
        }
    }

    // Whatever remains never appeared in `inputs`: hard-delete it.
    for (_slug, stale) in by_slug {
        event::Entity::delete_by_id(stale.id).exec(db).await?;
        report.deleted += 1;
    }

    Ok(report)
}

/// Outcome of a [`register`] call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterOutcome {
    Registered,
    AlreadyRegistered,
    EventNotFound,
}

/// Append `email` to a published event's `registrations`, deduped.
///
/// Only a published (`draft = false`) event accepts registration; a draft
/// or missing event returns [`RegisterOutcome::EventNotFound`]. We store
/// **only** the email (data minimization).
pub async fn register(
    db: &impl ConnectionTrait,
    public_slug: &str,
    email: &str,
) -> Result<RegisterOutcome, sea_orm::DbErr> {
    let Some(existing) = event::Entity::find()
        .filter(event::Column::PublicSlug.eq(public_slug))
        .filter(event::Column::Draft.eq(false))
        .one(db)
        .await?
    else {
        return Ok(RegisterOutcome::EventNotFound);
    };

    if existing.registrations.iter().any(|e| e == email) {
        return Ok(RegisterOutcome::AlreadyRegistered);
    }

    let mut registrations = existing.registrations.clone();
    registrations.push(email.to_owned());

    let mut active: event::ActiveModel = existing.into();
    active.registrations = ActiveValue::Set(registrations);
    active.updated_at = ActiveValue::NotSet;
    active.update(db).await?;

    Ok(RegisterOutcome::Registered)
}

#[cfg(test)]
mod tests {
    use super::{register, sync_from_markdown, EventSyncInput, EventType, RegisterOutcome};
    use crate::entity::event;
    use crate::test_support::pg;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    fn naive(s: &str) -> chrono::NaiveDateTime {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("parse naive datetime")
    }

    fn input(slug: &str, draft: bool) -> EventSyncInput {
        EventSyncInput {
            slug: slug.to_owned(),
            public_slug: format!("{slug}-public"),
            event_type: EventType::ShowAndTell,
            starts_at: naive("2026-07-01T18:00:00"),
            ends_at: naive("2026-07-01T19:00:00"),
            timezone: "America/Los_Angeles".to_owned(),
            draft,
        }
    }

    async fn fetch(db: &crate::Db, slug: &str) -> event::Model {
        event::Entity::find()
            .filter(event::Column::Slug.eq(slug))
            .one(db)
            .await
            .expect("query event")
            .expect("event row exists")
    }

    #[test]
    fn event_type_round_trips() {
        assert_eq!(EventType::ShowAndTell.as_str(), "show_and_tell");
        assert_eq!(
            "show_and_tell".parse::<EventType>().unwrap(),
            EventType::ShowAndTell
        );
        assert!("nope".parse::<EventType>().is_err());
    }

    #[tokio::test]
    async fn sync_creates_updates_preserving_registrations_and_deletes() {
        let db = pg().await;

        // First sync: two brand-new events.
        let report = sync_from_markdown(&db, &[input("alpha", false), input("beta", false)])
            .await
            .unwrap();
        assert_eq!(report.created, 2);
        assert_eq!(report.updated, 0);
        assert_eq!(report.deleted, 0);

        // Register someone on alpha so we can prove sync preserves it.
        assert_eq!(
            register(&db, "alpha-public", "fan@example.com")
                .await
                .unwrap(),
            RegisterOutcome::Registered
        );

        // Second sync: alpha changes (timezone), beta is gone, gamma is new.
        let mut alpha_changed = input("alpha", false);
        alpha_changed.timezone = "America/New_York".to_owned();
        let report = sync_from_markdown(&db, &[alpha_changed, input("gamma", false)])
            .await
            .unwrap();
        assert_eq!(report.created, 1, "gamma is new");
        assert_eq!(report.updated, 1, "alpha is updated");
        assert_eq!(report.deleted, 1, "beta left the markdown");

        let alpha = fetch(&db, "alpha").await;
        assert_eq!(alpha.timezone, "America/New_York", "update applied");
        assert_eq!(
            alpha.registrations,
            vec!["fan@example.com".to_owned()],
            "registrations preserved across update"
        );

        // beta is hard-deleted; gamma exists.
        assert!(event::Entity::find()
            .filter(event::Column::Slug.eq("beta"))
            .one(&db)
            .await
            .unwrap()
            .is_none());
        let _ = fetch(&db, "gamma").await;
    }

    #[tokio::test]
    async fn register_appends_dedupes_and_guards_draft_and_unknown() {
        let db = pg().await;
        sync_from_markdown(&db, &[input("live", false), input("hidden", true)])
            .await
            .unwrap();

        // First registration succeeds and stores only the email.
        assert_eq!(
            register(&db, "live-public", "a@example.com").await.unwrap(),
            RegisterOutcome::Registered
        );
        assert_eq!(
            fetch(&db, "live").await.registrations,
            vec!["a@example.com".to_owned()]
        );

        // Repeat is idempotent.
        assert_eq!(
            register(&db, "live-public", "a@example.com").await.unwrap(),
            RegisterOutcome::AlreadyRegistered
        );
        assert_eq!(
            fetch(&db, "live").await.registrations.len(),
            1,
            "no duplicate appended"
        );

        // A second distinct email appends.
        assert_eq!(
            register(&db, "live-public", "b@example.com").await.unwrap(),
            RegisterOutcome::Registered
        );
        assert_eq!(fetch(&db, "live").await.registrations.len(), 2);

        // A draft event refuses registration.
        assert_eq!(
            register(&db, "hidden-public", "c@example.com")
                .await
                .unwrap(),
            RegisterOutcome::EventNotFound
        );

        // An unknown slug refuses registration.
        assert_eq!(
            register(&db, "nope-public", "d@example.com").await.unwrap(),
            RegisterOutcome::EventNotFound
        );
    }
}
