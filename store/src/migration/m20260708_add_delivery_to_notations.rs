//! Add `delivery` to `notations`.
//!
//! How the client receives a notation sent for e-signature:
//!
//! - `embedded` (the default) — the client is a **captive** DocuSign
//!   recipient who signs inside Neon Law Navigator via the embedded recipient
//!   view (`web::esign_view`). DocuSign does not email them; they sign on
//!   a Neon Law Navigator screen (in-office, or a logged-in portal session). This
//!   preserves the historical retainer-walk behavior for every existing
//!   notation.
//! - `emailed` — the client is a **non-captive** recipient; DocuSign
//!   emails them a signing link they open from their own inbox. This is
//!   the matter-open path: an admin opens a brand-new client's matter and
//!   the retainer goes out by email, because that client is not in the
//!   room and has no portal session yet.
//!
//! The choice is per-notation and read once, when the workflow reaches
//! `sent_for_signature__pending` and builds the signature manifest
//! (`web::retainer_walk::assemble_and_send`). It selects how the *single*
//! send path addresses the client recipient — it is not a second send
//! path. `NOT NULL DEFAULT 'embedded'` so every existing notation keeps
//! its captive behavior with no backfill.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Notations::Table)
                    .add_column(
                        ColumnDef::new(Notations::Delivery)
                            .string()
                            .not_null()
                            .default("embedded")
                            .comment(
                                "How the client receives a notation sent for signature: \
                                 'embedded' (captive — signs inside Neon Law Navigator, no email) or \
                                 'emailed' (DocuSign emails a signing link). Read when building \
                                 the signature manifest; selects how the single send path \
                                 addresses the client recipient.",
                            ),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Notations::Table)
                    .drop_column(Notations::Delivery)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    Delivery,
}
