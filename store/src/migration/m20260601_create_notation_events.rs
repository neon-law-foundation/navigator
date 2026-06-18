//! `notation_events` — see glossary term
//! [Notation Event](../../../docs/glossary.md#notation-event).
//!
//! Append-only journal of every state-machine transition for every
//! Notation. Mirrors the `workflows::WorkflowEvent` runtime type.
//!
//! One row per transition; no row is ever updated. The "current
//! state" of a `(notation_id, machine_kind)` machine is the
//! `to_state` of the latest row ordered by `id` — with UUIDv7
//! that's the time-of-generation order.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(NotationEvents::Table)
                    .if_not_exists()
                    .comment(
                        "Notation Event — append-only journal row for a Notation's \
                         state-machine transition. See docs/glossary.md#notation-event.",
                    )
                    .col(
                        ColumnDef::new(NotationEvents::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Notation Event (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(NotationEvents::NotationId)
                            .uuid()
                            .not_null()
                            .comment("FK → Notation (`notations.id`)."),
                    )
                    .col(
                        ColumnDef::new(NotationEvents::MachineKind)
                            .string()
                            .not_null()
                            .comment("Machine kind token — `questionnaire` or `workflow`."),
                    )
                    .col(
                        ColumnDef::new(NotationEvents::FromState)
                            .string()
                            .not_null()
                            .comment("State name the Notation transitioned from."),
                    )
                    .col(
                        ColumnDef::new(NotationEvents::ToState)
                            .string()
                            .not_null()
                            .comment("State name the Notation transitioned to."),
                    )
                    .col(
                        ColumnDef::new(NotationEvents::Condition)
                            .string()
                            .not_null()
                            .comment(
                                "Condition that fired the transition (e.g., `_`, `approved`).",
                            ),
                    )
                    .col(
                        ColumnDef::new(NotationEvents::Payload)
                            .text()
                            .null()
                            .comment(
                                "Opaque JSON payload — questionnaire signals carry \
                                 `{\"answer_value\": \"…\"}`; workflow signals are null.",
                            ),
                    )
                    .col(
                        ColumnDef::new(NotationEvents::RecordedAt)
                            .string()
                            .not_null()
                            .comment(
                                "RFC 3339 / ISO 8601 timestamp when the transition was recorded.",
                            ),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_notation_events_notation")
                            .from(NotationEvents::Table, NotationEvents::NotationId)
                            .to(Notations::Table, Notations::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // Composite index supports the "latest event for
        // (notation_id, machine_kind)" projection query — the
        // single hottest read on this table.
        manager
            .create_index(
                Index::create()
                    .name("idx_notation_events_notation_kind_id")
                    .table(NotationEvents::Table)
                    .col(NotationEvents::NotationId)
                    .col(NotationEvents::MachineKind)
                    .col(NotationEvents::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_notation_events_notation_kind_id")
                    .table(NotationEvents::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(NotationEvents::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum NotationEvents {
    Table,
    Id,
    NotationId,
    MachineKind,
    FromState,
    ToState,
    Condition,
    Payload,
    RecordedAt,
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    Id,
}
