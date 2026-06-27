//! `events` — public events (show-and-tells) and their registrations.
//!
//! The Markdown files under `web/content/events/` remain the source of
//! truth for display. This table tracks event *existence* (keyed by the
//! Markdown `slug`) plus the registrant emails collected for each event,
//! so registration can move off Luma without changing the display story.
//!
//! # Conventions
//!
//! - `event_type` is CHECK-constrained text, not a native PG enum
//!   (house style — mirrors `relationship_edges.from_type`).
//! - `starts_at` / `ends_at` are `timestamp` **without** time zone: a
//!   show-and-tell happens at a wall-clock time in `timezone`, so we
//!   store the local wall time and the zone, never a UTC instant.
//! - `registrations` is a Postgres `text[]` of registrant emails. We
//!   store **only** the email (data minimization) and dedupe on append.
//! - Timestamps follow the workspace convention: `inserted_at` /
//!   `updated_at` RFC 3339 text set by `uuid_active_model_behavior!`.
//!   `created_at` is forbidden (see `store/tests/timestamp_convention.rs`).

use sea_orm_migration::{prelude::*, sea_orm::sea_query::Expr};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // One declarative table builder; the length is column defs + comments,
    // not branching logic.
    #[allow(clippy::too_many_lines)]
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Events::Table)
                    .if_not_exists()
                    .comment(
                        "Event — a public event (show-and-tell) tracked for registration. \
                         The Markdown file under web/content/events/ stays the display \
                         source of truth; this row carries existence + registrant emails.",
                    )
                    .col(
                        ColumnDef::new(Events::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Event."),
                    )
                    .col(
                        ColumnDef::new(Events::Slug)
                            .text()
                            .not_null()
                            .unique_key()
                            .comment(
                                "Markdown file slug — the natural key the Markdown sync \
                                 upserts on. Unique across all events.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Events::PublicSlug)
                            .text()
                            .not_null()
                            .comment("Public-facing slug used in the registration URL."),
                    )
                    .col(
                        ColumnDef::new(Events::EventType)
                            .text()
                            .not_null()
                            .comment("Event kind. CHECK-constrained to ('show_and_tell')."),
                    )
                    .col(
                        ColumnDef::new(Events::StartsAt)
                            .timestamp()
                            .not_null()
                            .comment("Local wall-clock start time (no tz); zone is `timezone`."),
                    )
                    .col(
                        ColumnDef::new(Events::EndsAt)
                            .timestamp()
                            .not_null()
                            .comment("Local wall-clock end time (no tz); zone is `timezone`."),
                    )
                    .col(
                        ColumnDef::new(Events::Timezone)
                            .text()
                            .not_null()
                            .comment("IANA timezone name the start/end wall times are in."),
                    )
                    .col(
                        ColumnDef::new(Events::Draft)
                            .boolean()
                            .not_null()
                            .default(false)
                            .comment(
                                "When true the event is unpublished and rejects registration.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Events::Registrations)
                            .array(ColumnType::Text)
                            .not_null()
                            .default(Expr::cust("'{}'::text[]"))
                            .comment(
                                "Registrant emails (text[]). Stores only the email \
                                 (data minimization); deduped on append.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Events::InsertedAt)
                            .text()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(Events::UpdatedAt)
                            .text()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was last updated."),
                    )
                    .check(Expr::col(Events::EventType).is_in(["show_and_tell"]))
                    .to_owned(),
            )
            .await?;

        // The unique constraint on `slug` already backs lookups, but an
        // explicit named index keeps the sync's per-slug fetch cheap and
        // self-documenting.
        manager
            .create_index(
                Index::create()
                    .name("idx_events_slug")
                    .table(Events::Table)
                    .col(Events::Slug)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Events::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Events {
    Table,
    Id,
    Slug,
    PublicSlug,
    EventType,
    StartsAt,
    EndsAt,
    Timezone,
    Draft,
    Registrations,
    InsertedAt,
    UpdatedAt,
}
