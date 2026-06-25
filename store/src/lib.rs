#![allow(clippy::doc_markdown)]
//! Navigator CRM data layer.
//!
//! Owns the SeaORM schema, migrations, entities, and canonical seed.
//! Every workspace crate that touches the database — `web`, `cli`,
//! `mcp` — depends on this crate; nothing here depends on axum,
//! reqwest, or any HTTP machinery.

pub mod attestations;
pub mod blobs;
pub mod communications;
pub mod config;
pub mod conflicts;
pub mod contract_reviews;
pub mod coupons;
pub mod db;
pub mod db_error;
pub mod document_comments;
pub mod documents;
pub mod email_conversations;
pub mod email_tokens;
pub mod entity;
pub mod expunge_records;
pub mod expunge_requests;
pub mod filings;
pub mod git_access_tokens;
pub mod migration;
pub mod notation_clauses;
pub mod notations;
pub mod persons;
pub mod playbooks;
pub mod products;
pub mod projects;
pub mod review_documents;
pub mod seed;
pub mod statutes;
pub mod subscriptions;
pub mod templates;
pub mod testimonials;
pub mod xero_invoices;

pub use db_error::is_unique_violation;
/// Re-exported so downstream crates (e.g. `billing-workflows`) can name
/// the database error type without taking a direct `sea-orm` dependency.
pub use sea_orm::DbErr;

#[cfg(feature = "test-support")]
pub mod test_support;

pub use config::{DbConfig, DbConfigError};
pub use db::{connect, migrate, ping, Db};

/// `impl ActiveModelBehavior` that fills in three things every workspace
/// entity needs and nothing else: a fresh `Uuid::now_v7()` on `id` if the
/// caller hasn't set one, plus `inserted_at` (on insert only) and
/// `updated_at` (on every save) as RFC 3339 timestamps. Lets every entity
/// keep using `..Default::default()` despite UUID PKs having no DB-side
/// default. Invoke once per entity module after the `Model` definition.
///
/// Every entity must carry `inserted_at: String` and `updated_at: String`
/// fields — workspace convention enforced by the global tests in
/// `store/tests/conventions.rs`. Explicit timestamp sets from callers
/// (e.g. test fixtures pinning a fixed string for assertions) are
/// preserved; the macro only fills in `NotSet` values.
#[macro_export]
macro_rules! uuid_active_model_behavior {
    () => {
        #[::async_trait::async_trait]
        impl ::sea_orm::ActiveModelBehavior for ActiveModel {
            async fn before_save<C: ::sea_orm::ConnectionTrait>(
                mut self,
                _db: &C,
                insert: bool,
            ) -> ::std::result::Result<Self, ::sea_orm::DbErr> {
                let now = ::chrono::Utc::now().to_rfc3339();
                if insert {
                    if let ::sea_orm::ActiveValue::NotSet = self.id {
                        self.id = ::sea_orm::ActiveValue::Set(::uuid::Uuid::now_v7());
                    }
                    if let ::sea_orm::ActiveValue::NotSet = self.inserted_at {
                        self.inserted_at = ::sea_orm::ActiveValue::Set(now.clone());
                    }
                    if let ::sea_orm::ActiveValue::NotSet = self.updated_at {
                        self.updated_at = ::sea_orm::ActiveValue::Set(now);
                    }
                } else {
                    // Update path: bump `updated_at` unless the caller
                    // explicitly set it. `inserted_at` is immutable.
                    if let ::sea_orm::ActiveValue::NotSet = self.updated_at {
                        self.updated_at = ::sea_orm::ActiveValue::Set(now);
                    }
                }
                ::std::result::Result::Ok(self)
            }
        }
    };
}
