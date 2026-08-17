//! The presentation tree behind a mounted matter folder.
//!
//! A mounted Project folder is not a listing of an object-storage prefix.
//! [`cloud::StorageService`] stores every asset content-addressed at
//! `blobs/<sha256>` (see `store::documents::ingest_bytes`), so the bucket
//! carries no hierarchy and no human-readable name at all. The hierarchy
//! lives in the `assets` table: `project_id` groups, `filename` names, and
//! `slug` identifies a document across its revisions.
//!
//! This crate turns those rows into the directory a client's operating
//! system can actually render, and it is deliberately free of both the
//! database and the mount protocol:
//!
//! - the caller reads rows (already authorization-filtered — a client sees
//!   only client-visible revisions on their own matters, per
//!   `docs/access-model.md`) and hands them over as [`tree::Document`];
//! - [`tree::directory`] returns names that are legal, stable, and unique
//!   on macOS **and** Windows simultaneously;
//! - [`layout::matter`] splits those rows into the folder's two halves and
//!   their subfolders, and names each directory independently;
//! - the sync materializer renders that same output.
//!
//! The two halves are the authorization boundary, not a filing
//! convention: `assets.visibility` alone decides whether a row lands in
//! `client/` or `internal/`, and [`layout`] refuses to guess for a value
//! outside that closed vocabulary.
//!
//! Two properties make the mount tractable. Blobs are immutable, so a local
//! content cache keyed by `sha256_hex` is correct without any invalidation
//! protocol. And names are derived, never stored, so the same matter mounts
//! identically on every device.

pub mod layout;
pub mod name;
pub mod tree;
