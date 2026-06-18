//! `playbooks` + `contract_reviews` — the first review-*in* matter
//! (fractional-GC inbound contract review).
//!
//! Every other Navigator workflow is template-*out*: a template body
//! compiles into a document the client signs. Contract review is the
//! mirror — the client uploads a third party's contract and the binding
//! artifact the firm produces is a *memo about someone else's document*.
//! Two tables back it:
//!
//! - `playbooks` holds a client's stored negotiating positions, scoped to
//!   the client **Entity** (the company), so one playbook serves every
//!   matter for that client. `positions` is a JSONB array of
//!   `{topic, preferred, fallback, walkaway, severity}` — read-mostly,
//!   edited as a whole through the admin playbook surface. `(entity_id,
//!   name)` is unique so a client's "SaaS vendor MSA" playbook is a stable
//!   natural key.
//! - `contract_reviews` is the per-notation work-product satellite (the
//!   same shape `review_documents` plays for the estate matter): the
//!   `findings` JSONB array (`{clause_ref, deviation, severity,
//!   suggested_redline, attorney_note, accepted}`) is the deviation report
//!   the analysis step produces and the reviewing attorney edits before
//!   approving. `document_id` points at the filed inbound-contract
//!   `documents` row; `risk_summary` is filled once analyzed. The matter's
//!   audit trail (who accepted which finding) lives in `notation_events`,
//!   not here — this row is the editable working copy.

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
                    .table(Playbooks::Table)
                    .if_not_exists()
                    .comment(
                        "Playbook — a client Entity's stored contract-negotiation \
                         positions, applied when reviewing that client's inbound \
                         contracts. One playbook serves every matter for the Entity.",
                    )
                    .col(
                        ColumnDef::new(Playbooks::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this playbook (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(Playbooks::EntityId)
                            .uuid()
                            .not_null()
                            .comment("FK → Entity (`entities.id`) — the client company this playbook belongs to."),
                    )
                    .col(
                        ColumnDef::new(Playbooks::Name)
                            .string()
                            .not_null()
                            .comment("Human label for the playbook (e.g. `SaaS vendor MSA`)."),
                    )
                    .col(
                        ColumnDef::new(Playbooks::Positions)
                            .json_binary()
                            .not_null()
                            .comment(
                                "JSONB array of positions: \
                                 `{topic, preferred, fallback, walkaway, severity}`. \
                                 Read-mostly; edited as a whole via the admin surface.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Playbooks::Active)
                            .boolean()
                            .not_null()
                            .default(true)
                            .comment("Whether this playbook is currently applied to new reviews."),
                    )
                    .col(
                        ColumnDef::new(Playbooks::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(Playbooks::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_playbooks_entity")
                            .from(Playbooks::Table, Playbooks::EntityId)
                            .to(Entities::Table, Entities::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_playbooks_entity_name")
                    .table(Playbooks::Table)
                    .col(Playbooks::EntityId)
                    .col(Playbooks::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ContractReviews::Table)
                    .if_not_exists()
                    .comment(
                        "ContractReview — one inbound-contract review: the deviation \
                         findings the analysis step produced (and the attorney edits) \
                         for a notation, measured against a playbook. The review-in \
                         work-product satellite, mirroring ReviewDocument for estate.",
                    )
                    .col(
                        ColumnDef::new(ContractReviews::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this review (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(ContractReviews::NotationId)
                            .uuid()
                            .not_null()
                            .comment("FK → Notation (`notations.id`) — the review matter."),
                    )
                    .col(
                        ColumnDef::new(ContractReviews::PlaybookId)
                            .uuid()
                            .not_null()
                            .comment("FK → Playbook (`playbooks.id`) — positions the contract was measured against."),
                    )
                    .col(
                        ColumnDef::new(ContractReviews::DocumentId)
                            .uuid()
                            .comment(
                                "FK → Document (`documents.id`) — the filed inbound \
                                 contract. `None` until the contract is uploaded.",
                            ),
                    )
                    .col(
                        ColumnDef::new(ContractReviews::Status)
                            .string()
                            .not_null()
                            .comment(
                                "`pending` (created, not analyzed), `analyzed` (findings \
                                 produced, awaiting attorney), `approved`, or `rejected`.",
                            ),
                    )
                    .col(
                        ColumnDef::new(ContractReviews::RiskSummary)
                            .text()
                            .comment("Plain-language risk summary; `None` until analyzed."),
                    )
                    .col(
                        ColumnDef::new(ContractReviews::Findings)
                            .json_binary()
                            .not_null()
                            .comment(
                                "JSONB array of findings: `{clause_ref, deviation, \
                                 severity, suggested_redline, attorney_note, accepted}`. \
                                 Produced by analysis, edited by the reviewing attorney.",
                            ),
                    )
                    .col(
                        ColumnDef::new(ContractReviews::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(ContractReviews::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_contract_reviews_notation")
                            .from(ContractReviews::Table, ContractReviews::NotationId)
                            .to(Notations::Table, Notations::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_contract_reviews_playbook")
                            .from(ContractReviews::Table, ContractReviews::PlaybookId)
                            .to(Playbooks::Table, Playbooks::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_contract_reviews_document")
                            .from(ContractReviews::Table, ContractReviews::DocumentId)
                            .to(Documents::Table, Documents::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_contract_reviews_notation_id")
                    .table(ContractReviews::Table)
                    .col(ContractReviews::NotationId)
                    .col(ContractReviews::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_contract_reviews_notation_id")
                    .table(ContractReviews::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(ContractReviews::Table).to_owned())
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_playbooks_entity_name")
                    .table(Playbooks::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Playbooks::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Playbooks {
    Table,
    Id,
    EntityId,
    Name,
    Positions,
    Active,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ContractReviews {
    Table,
    Id,
    NotationId,
    PlaybookId,
    DocumentId,
    Status,
    RiskSummary,
    Findings,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Entities {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Documents {
    Table,
    Id,
}
