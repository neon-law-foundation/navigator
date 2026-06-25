//! `relationship_edges` — the canonical two-sided graph edge that the
//! pre-matter conflict check traverses.
//!
//! # Why a new table, not `relationship_logs`
//!
//! `relationship_logs` is a one-sided **audit trail**: one actor
//! (`actor_person_id`) took one `action` against one subject
//! (`subject_type` + `subject_id`), with a free-form `detail` string.
//! It answers "what changed when," not "who is connected to whom."
//!
//! A conflict check needs a graph whose every edge has a **person or
//! entity on each end** and a typed relationship between them
//! (`manages`, `owns`, `married_to`, `adverse_to`, …). This table is
//! that graph. Postgres stays the source of truth; `store::conflicts`
//! loads these rows (plus the already-structured `person_entity_roles`)
//! into an in-memory petgraph per check — no second datastore, no
//! Apache AGE (Cloud SQL forbids the extension), no Neo4j until scale
//! demands it.
//!
//! # Provenance is load-bearing
//!
//! Both endpoints are typed (`from_type`/`to_type` ∈ {`person`,
//! `entity`}). Each edge records where it came from (`source_kind` +
//! nullable `source_id`) and how much we trust it (`confidence_pct`,
//! 0–100). An edge an LLM parsed out of `relationship_logs.detail`
//! lands here at lower confidence than one a human asserted, and the
//! conflict report shows the source so staff can judge a finding rather
//! than trust a guess.

use sea_orm_migration::{prelude::*, sea_orm::sea_query::Expr};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    #[allow(clippy::too_many_lines)]
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RelationshipEdges::Table)
                    .if_not_exists()
                    .comment(
                        "Relationship Edge — a typed graph edge with a Person or Entity \
                         on each end, traversed by the pre-matter conflict check. \
                         See docs/glossary.md#conflict-check-graph.",
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Relationship Edge."),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::FromType)
                            .string()
                            .not_null()
                            .comment("Node kind of the `from` endpoint (`person` or `entity`)."),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::FromId)
                            .uuid()
                            .not_null()
                            .comment(
                                "UUID of the `from` endpoint in the table named by `from_type`.",
                            ),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::ToType)
                            .string()
                            .not_null()
                            .comment("Node kind of the `to` endpoint (`person` or `entity`)."),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::ToId)
                            .uuid()
                            .not_null()
                            .comment("UUID of the `to` endpoint in the table named by `to_type`."),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::Kind)
                            .string()
                            .not_null()
                            .comment(
                                "Relationship kind (`manages`, `owns`, `married_to`, \
                                 `adverse_to`, `related_party`, …). `adverse_to` and \
                                 `related_party` drive conflict findings.",
                            ),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::ConfidencePct)
                            .integer()
                            .not_null()
                            .default(100)
                            .comment(
                                "Confidence this edge is real, 0–100. Human-asserted edges \
                                 are 100; LLM-parsed edges land lower. The conflict check \
                                 multiplies confidence along a path and floors weak paths.",
                            ),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::SourceKind)
                            .string()
                            .not_null()
                            .comment(
                                "Where this edge came from (`manual`, `disclosure`, \
                                 `relationship_log`, `llm`). Shown in conflict findings.",
                            ),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::SourceId)
                            .uuid()
                            .null()
                            .comment(
                                "Optional FK-by-value to the originating row (e.g. the \
                                 `relationship_logs.id` an `llm` edge was parsed from); \
                                 null for `manual` edges.",
                            ),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::Detail)
                            .text()
                            .not_null()
                            .default("")
                            .comment(
                                "Free-form note (e.g. the phrase an `llm` edge was parsed from).",
                            ),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(RelationshipEdges::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was last updated."),
                    )
                    .check(Expr::col(RelationshipEdges::FromType).is_in(["person", "entity"]))
                    .check(Expr::col(RelationshipEdges::ToType).is_in(["person", "entity"]))
                    .to_owned(),
            )
            .await?;

        // Two endpoint indexes: the graph builder loads edges by either
        // end, so both directions need to be cheap.
        manager
            .create_index(
                Index::create()
                    .name("idx_relationship_edges_from")
                    .table(RelationshipEdges::Table)
                    .col(RelationshipEdges::FromType)
                    .col(RelationshipEdges::FromId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_relationship_edges_to")
                    .table(RelationshipEdges::Table)
                    .col(RelationshipEdges::ToType)
                    .col(RelationshipEdges::ToId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_relationship_edges_unique_tuple")
                    .table(RelationshipEdges::Table)
                    .col(RelationshipEdges::FromType)
                    .col(RelationshipEdges::FromId)
                    .col(RelationshipEdges::ToType)
                    .col(RelationshipEdges::ToId)
                    .col(RelationshipEdges::Kind)
                    .unique()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(RelationshipEdges::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum RelationshipEdges {
    Table,
    Id,
    FromType,
    FromId,
    ToType,
    ToId,
    Kind,
    ConfidencePct,
    SourceKind,
    SourceId,
    Detail,
    InsertedAt,
    UpdatedAt,
}
