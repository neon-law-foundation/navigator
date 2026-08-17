#![allow(clippy::doc_markdown)]
//! Pure presentation helpers shared by server-side routes.

pub mod code;
pub mod sort_spec;

pub use code::{code_block, highlight};
pub use sort_spec::{SortDirection, SortError, SortField, SortSpec};
