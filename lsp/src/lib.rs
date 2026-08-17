//! `navigator-lsp` core. Exposes pure functions used by the binary
//! and the integration tests — no I/O beyond what each function
//! takes as input.
//!
//! The binary in `main.rs` wires these together over stdio JSON-RPC.

pub mod diagnostics;
pub mod position;
pub mod state;

pub use diagnostics::{lint_buffer, violation_to_diagnostic};
pub use position::{byte_to_position, range_to_lsp_range};
pub use state::Server;
