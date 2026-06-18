//! `blobs`, `documents` — opaque byte storage and the named
//! references that point at it. See glossary terms
//! [Blob](../../../docs/glossary.md#blob) and
//! [Document](../../../docs/glossary.md#document).

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Blobs::Table)
                    .if_not_exists()
                    .comment(
                        "Blob — opaque byte reference (content type + storage key); \
                         the bytes live in object storage. See docs/glossary.md#blob.",
                    )
                    .col(
                        ColumnDef::new(Blobs::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Blob."),
                    )
                    .col(
                        ColumnDef::new(Blobs::StorageKey)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("Object-storage key returned by `cloud::StorageService`."),
                    )
                    .col(
                        ColumnDef::new(Blobs::ContentType)
                            .string()
                            .not_null()
                            .comment("MIME content type (e.g., `application/pdf`)."),
                    )
                    .col(
                        ColumnDef::new(Blobs::ByteSize)
                            .big_integer()
                            .not_null()
                            .comment("Size in bytes."),
                    )
                    .col(
                        ColumnDef::new(Blobs::Sha256Hex)
                            .string()
                            .not_null()
                            .comment("Lowercase hex SHA-256 of the byte content."),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Documents::Table)
                    .if_not_exists()
                    .comment(
                        "Document — a named, project-scoped reference to a Blob. \
                         See docs/glossary.md#document.",
                    )
                    .col(
                        ColumnDef::new(Documents::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Document."),
                    )
                    .col(
                        ColumnDef::new(Documents::ProjectId).uuid().null().comment(
                            "FK → Project (`projects.id`), nullable for unscoped Documents.",
                        ),
                    )
                    .col(
                        ColumnDef::new(Documents::BlobId)
                            .uuid()
                            .not_null()
                            .comment("FK → Blob (`blobs.id`) holding the actual bytes."),
                    )
                    .col(
                        ColumnDef::new(Documents::Filename)
                            .string()
                            .not_null()
                            .comment("Caller-visible filename."),
                    )
                    .col(
                        ColumnDef::new(Documents::Kind)
                            .string()
                            .not_null()
                            .comment("Document classification (e.g., `retainer`, `invoice`)."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_documents_blob")
                            .from(Documents::Table, Documents::BlobId)
                            .to(Blobs::Table, Blobs::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_documents_project")
                            .from(Documents::Table, Documents::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Documents::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Blobs::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Blobs {
    Table,
    Id,
    StorageKey,
    ContentType,
    ByteSize,
    Sha256Hex,
}

#[derive(DeriveIden)]
enum Documents {
    Table,
    Id,
    ProjectId,
    BlobId,
    Filename,
    Kind,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}
