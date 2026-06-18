//! `email_conversations` + `email_conversation_messages` — the threaded
//! support inbox behind `support@neonlaw.com` (the "headless Front").
//!
//! An external party (a client, a prospective client) emails
//! `support@neonlaw.com`; `web` opens one `email_conversations` row keyed
//! by an opaque `token`. That token rides in the `Reply-To`
//! (`c<token>@parse.neonlaw.com`) of every message, so staff and client
//! replies thread back to the same row without any internal address ever
//! leaking. A conversation can be linked to a running matter via
//! `notation_id` — that is what lets an attorney's `@approve` reply fire a
//! Restate workflow signal (the production `staff_review` gate).
//!
//! `email_conversation_messages` is the append-only transcript: one row
//! per hop (inbound from the external party, the notification to staff,
//! the staff reply, the relay back out, or a `system` note). Modeled on
//! `notation_events` — never updated, only appended; `status` on the
//! conversation is a projection of the latest hop. The raw `.eml` for an
//! inbound hop stays in object storage; `raw_storage_key` points at it.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    #[allow(clippy::too_many_lines)]
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(EmailConversations::Table)
                    .if_not_exists()
                    .comment(
                        "EmailConversation — one threaded support exchange behind \
                         support@neonlaw.com, keyed by an opaque token carried in the \
                         Reply-To so replies thread without leaking internal addresses.",
                    )
                    .col(
                        ColumnDef::new(EmailConversations::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this EmailConversation (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(EmailConversations::Token)
                            .string()
                            .not_null()
                            .comment("Opaque, unguessable thread token; the VERP key in Reply-To (c<token>@parse…)."),
                    )
                    .col(
                        ColumnDef::new(EmailConversations::ExternalEmail)
                            .string()
                            .not_null()
                            .comment("The external party's address (client or prospective client)."),
                    )
                    .col(
                        ColumnDef::new(EmailConversations::ExternalName)
                            .string()
                            .null()
                            .comment("The external party's display name, if the inbound message carried one."),
                    )
                    .col(
                        ColumnDef::new(EmailConversations::PersonId)
                            .uuid()
                            .null()
                            .comment("FK → Person (`persons.id`) once the sender is matched; NULL until conflict-checked."),
                    )
                    .col(
                        ColumnDef::new(EmailConversations::Subject)
                            .string()
                            .not_null()
                            .comment("Subject of the originating message; threads keep it for the staff notification."),
                    )
                    .col(ColumnDef::new(EmailConversations::Status).string().not_null().comment(
                        "`open`, `awaiting_staff` (notified, waiting on the attorney), \
                         `awaiting_client` (relayed, waiting on the external party), or `closed`.",
                    ))
                    .col(
                        ColumnDef::new(EmailConversations::NotationId)
                            .uuid()
                            .null()
                            .comment("FK → Notation (`notations.id`) — the matter this thread drives, if any."),
                    )
                    .col(
                        ColumnDef::new(EmailConversations::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(EmailConversations::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_email_conversations_person")
                            .from(EmailConversations::Table, EmailConversations::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_email_conversations_notation")
                            .from(EmailConversations::Table, EmailConversations::NotationId)
                            .to(Notations::Table, Notations::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_email_conversations_token")
                    .table(EmailConversations::Table)
                    .col(EmailConversations::Token)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_email_conversations_external_email")
                    .table(EmailConversations::Table)
                    .col(EmailConversations::ExternalEmail)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(EmailConversationMessages::Table)
                    .if_not_exists()
                    .comment(
                        "EmailConversationMessage — one append-only hop in a support thread \
                         (inbound external, notification to staff, staff reply, relay out, or \
                         a system note). Never updated; the conversation status projects from it.",
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this message (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::ConversationId)
                            .uuid()
                            .not_null()
                            .comment("FK → EmailConversation (`email_conversations.id`) this hop belongs to."),
                    )
                    .col(ColumnDef::new(EmailConversationMessages::Direction).string().not_null().comment(
                        "`from_external`, `to_staff`, `from_staff`, `to_external`, or `system`.",
                    ))
                    .col(
                        ColumnDef::new(EmailConversationMessages::FromAddr)
                            .string()
                            .not_null()
                            .comment("Envelope/header From of this hop."),
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::ToAddr)
                            .string()
                            .not_null()
                            .comment("Envelope/header To of this hop."),
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::Subject)
                            .string()
                            .not_null()
                            .comment("Subject of this hop."),
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::BodyText)
                            .text()
                            .not_null()
                            .comment("Cleaned body (quoted history + signature stripped on staff replies)."),
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::RawStorageKey)
                            .string()
                            .null()
                            .comment("Object-storage key of the raw .eml for inbound hops; NULL for ones we generated."),
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::ProviderMessageId)
                            .string()
                            .null()
                            .comment("SendGrid X-Message-Id (outbound) or inbound Message-ID; join key + dedup."),
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::InReplyTo)
                            .string()
                            .null()
                            .comment("RFC 5322 In-Reply-To of this hop, when present."),
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::CommandPayload)
                            .text()
                            .null()
                            .comment("Parsed staff-reply directives (@approve/@deny/…) as JSON; NULL when none."),
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(EmailConversationMessages::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_email_conversation_messages_conversation")
                            .from(EmailConversationMessages::Table, EmailConversationMessages::ConversationId)
                            .to(EmailConversations::Table, EmailConversations::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_email_conversation_messages_conversation_id")
                    .table(EmailConversationMessages::Table)
                    .col(EmailConversationMessages::ConversationId)
                    .col(EmailConversationMessages::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_email_conversation_messages_conversation_id")
                    .table(EmailConversationMessages::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(EmailConversationMessages::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_email_conversations_external_email")
                    .table(EmailConversations::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_email_conversations_token")
                    .table(EmailConversations::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(EmailConversations::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum EmailConversations {
    Table,
    Id,
    Token,
    ExternalEmail,
    ExternalName,
    PersonId,
    Subject,
    Status,
    NotationId,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum EmailConversationMessages {
    Table,
    Id,
    ConversationId,
    Direction,
    FromAddr,
    ToAddr,
    Subject,
    BodyText,
    RawStorageKey,
    ProviderMessageId,
    InReplyTo,
    CommandPayload,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    Id,
}
