//! Whole-page view functions.
//!
//! Each submodule exposes a single `pub fn <name>() -> Markup` that
//! returns a fully-rendered page (with the [`PageLayout`] shell
//! wrapped around it). Route handlers in the `web` crate are thin
//! wrappers that call these functions.

pub mod admin;
pub mod blog;
pub mod contact;
pub mod design;
pub mod docs;
pub mod events;
pub mod home;
pub mod lsp;
pub mod mission;
pub mod navigator;
pub mod notation_templates;
pub mod package;
pub mod policy;
pub mod portal;
pub mod privacy;
pub mod products;
pub mod service;
pub mod statutes;
pub mod templates;
pub mod terms;
pub mod workshops;
