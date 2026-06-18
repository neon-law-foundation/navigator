//! `projects`, `person_entity_roles`, `person_project_roles` —
//! the matter / role tables. See glossary terms
//! [Project](../../../docs/glossary.md#project),
//! [Person–Entity Role](../../../docs/glossary.md#personentity-role),
//! and [Person–Project Role](../../../docs/glossary.md#personproject-role).

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
                    .table(Projects::Table)
                    .if_not_exists()
                    .comment(
                        "Project — a unit of work tracked across Notations and \
                         retainers. See docs/glossary.md#project.",
                    )
                    .col(
                        ColumnDef::new(Projects::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Project."),
                    )
                    .col(
                        ColumnDef::new(Projects::Name)
                            .string()
                            .not_null()
                            .comment("Display name of the Project (matter codename)."),
                    )
                    .col(
                        ColumnDef::new(Projects::Status)
                            .string()
                            .not_null()
                            .comment("`open`, `closed`, or `archived`."),
                    )
                    .col(ColumnDef::new(Projects::EntityId).uuid().null().comment(
                        "FK → Entity (`entities.id`), nullable for individual-client matters.",
                    ))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_projects_entity")
                            .from(Projects::Table, Projects::EntityId)
                            .to(Entities::Table, Entities::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PersonEntityRoles::Table)
                    .if_not_exists()
                    .comment(
                        "Person–Entity Role — a Person's role within an Entity \
                         (manager, member, beneficiary, …). \
                         See docs/glossary.md#personentity-role.",
                    )
                    .col(
                        ColumnDef::new(PersonEntityRoles::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Person–Entity Role."),
                    )
                    .col(
                        ColumnDef::new(PersonEntityRoles::PersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`)."),
                    )
                    .col(
                        ColumnDef::new(PersonEntityRoles::EntityId)
                            .uuid()
                            .not_null()
                            .comment("FK → Entity (`entities.id`)."),
                    )
                    .col(
                        ColumnDef::new(PersonEntityRoles::Role)
                            .string()
                            .not_null()
                            .comment("Role token (e.g., `manager`, `member`, `beneficiary`)."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_entity_roles_person")
                            .from(PersonEntityRoles::Table, PersonEntityRoles::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_entity_roles_entity")
                            .from(PersonEntityRoles::Table, PersonEntityRoles::EntityId)
                            .to(Entities::Table, Entities::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PersonProjectRoles::Table)
                    .if_not_exists()
                    .comment(
                        "Person–Project Role — a Person's role on a Project \
                         (attorney, paralegal, client, …). \
                         See docs/glossary.md#personproject-role.",
                    )
                    .col(
                        ColumnDef::new(PersonProjectRoles::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Person–Project Role."),
                    )
                    .col(
                        ColumnDef::new(PersonProjectRoles::PersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`)."),
                    )
                    .col(
                        ColumnDef::new(PersonProjectRoles::ProjectId)
                            .uuid()
                            .not_null()
                            .comment("FK → Project (`projects.id`)."),
                    )
                    .col(
                        ColumnDef::new(PersonProjectRoles::Role)
                            .string()
                            .not_null()
                            .comment("Role token (e.g., `attorney`, `paralegal`, `client`)."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_project_roles_person")
                            .from(PersonProjectRoles::Table, PersonProjectRoles::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_project_roles_project")
                            .from(PersonProjectRoles::Table, PersonProjectRoles::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PersonProjectRoles::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PersonEntityRoles::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Projects::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
    Name,
    Status,
    EntityId,
}

#[derive(DeriveIden)]
enum PersonEntityRoles {
    Table,
    Id,
    PersonId,
    EntityId,
    Role,
}

#[derive(DeriveIden)]
enum PersonProjectRoles {
    Table,
    Id,
    PersonId,
    ProjectId,
    Role,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Entities {
    Table,
    Id,
}
