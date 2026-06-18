//! `git_repositories`, `disclosures`, `relationship_logs` — the
//! provenance layer. See glossary terms
//! [Git Repository](../../../docs/glossary.md#git-repository),
//! [Disclosure](../../../docs/glossary.md#disclosure), and
//! [Relationship Log](../../../docs/glossary.md#relationship-log).

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
                    .table(GitRepositories::Table)
                    .if_not_exists()
                    .comment(
                        "Git Repository — a repository that carries imported notation \
                         content (URL hash + last imported SHA). \
                         See docs/glossary.md#git-repository.",
                    )
                    .col(
                        ColumnDef::new(GitRepositories::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Git Repository row."),
                    )
                    .col(
                        ColumnDef::new(GitRepositories::RemoteHash)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("SHA-256 of `git remote get-url origin`."),
                    )
                    .col(
                        ColumnDef::new(GitRepositories::LastCommitSha)
                            .string()
                            .not_null()
                            .comment("Last imported commit SHA (40 hex chars)."),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Disclosures::Table)
                    .if_not_exists()
                    .comment(
                        "Disclosure — a formal disclosure attached to an Entity or \
                         a Project (conflicts, related-party, …). \
                         See docs/glossary.md#disclosure.",
                    )
                    .col(
                        ColumnDef::new(Disclosures::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Disclosure."),
                    )
                    .col(
                        ColumnDef::new(Disclosures::EntityId).uuid().null().comment(
                            "FK → Entity (`entities.id`), nullable when scoped to a Project.",
                        ),
                    )
                    .col(
                        ColumnDef::new(Disclosures::ProjectId)
                            .uuid()
                            .null()
                            .comment(
                                "FK → Project (`projects.id`), nullable when scoped to an Entity.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Disclosures::Kind)
                            .string()
                            .not_null()
                            .comment("Disclosure kind (e.g., `conflict`, `related_party`)."),
                    )
                    .col(
                        ColumnDef::new(Disclosures::Summary)
                            .text()
                            .not_null()
                            .comment("Human-readable summary of the Disclosure."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_disclosures_entity")
                            .from(Disclosures::Table, Disclosures::EntityId)
                            .to(Entities::Table, Entities::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_disclosures_project")
                            .from(Disclosures::Table, Disclosures::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RelationshipLogs::Table)
                    .if_not_exists()
                    .comment(
                        "Relationship Log — append-only audit trail of relationship \
                         changes (`person joined entity`, `project closed`, …). \
                         See docs/glossary.md#relationship-log.",
                    )
                    .col(
                        ColumnDef::new(RelationshipLogs::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Relationship Log entry."),
                    )
                    .col(
                        ColumnDef::new(RelationshipLogs::ActorPersonId)
                            .uuid()
                            .null()
                            .comment("FK → Person (`persons.id`) who took the action; null for system events."),
                    )
                    .col(
                        ColumnDef::new(RelationshipLogs::SubjectType)
                            .string()
                            .not_null()
                            .comment("The type of subject row (`person`, `entity`, `project`, …)."),
                    )
                    .col(
                        ColumnDef::new(RelationshipLogs::SubjectId)
                            .uuid()
                            .not_null()
                            .comment("UUID of the subject row in the table named by `subject_type`."),
                    )
                    .col(
                        ColumnDef::new(RelationshipLogs::Action)
                            .string()
                            .not_null()
                            .comment("Action token (e.g., `joined`, `closed`)."),
                    )
                    .col(
                        ColumnDef::new(RelationshipLogs::Detail)
                            .text()
                            .not_null()
                            .comment("Free-form detail for the audit entry."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_rel_logs_actor")
                            .from(RelationshipLogs::Table, RelationshipLogs::ActorPersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(RelationshipLogs::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Disclosures::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(GitRepositories::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum GitRepositories {
    Table,
    Id,
    RemoteHash,
    LastCommitSha,
}

#[derive(DeriveIden)]
enum Disclosures {
    Table,
    Id,
    EntityId,
    ProjectId,
    Kind,
    Summary,
}

#[derive(DeriveIden)]
enum RelationshipLogs {
    Table,
    Id,
    ActorPersonId,
    SubjectType,
    SubjectId,
    Action,
    Detail,
}

#[derive(DeriveIden)]
enum Entities {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}
