//! Model Context Protocol (MCP) server for Neon Law Navigator.
//!
//! Built for one purpose: let LibreChat-hosted LLMs reach into the
//! Neon Law Navigator CRM database. The transport is MCP's "Streamable HTTP"
//! variant — a single `/mcp` endpoint that speaks JSON-RPC 2.0.
//!
//! Two deployment shapes share the same router:
//!
//! 1. **Embedded** — call [`server::build_router`] and merge it into
//!    the `web` axum router. Same process, same `Db` clone.
//! 2. **Standalone** — run the `mcp` binary on its own port. Useful
//!    when `LibreChat` is on a separate cluster, or when we want to
//!    scale tool-call traffic independently from the public website.
//!
//! Both shapes go through the same data layer: the `web` crate's
//! public `entity`/`db`/`migration` modules. If the dependency
//! surface ever bloats here, the right move is to factor those
//! modules into a `crm` crate that both `web` and `mcp` consume —
//! the boundary is already pretty clean.

pub mod principal;
pub mod protocol;
pub mod server;
pub mod tools;

pub use principal::Principal;
pub use server::{build_router, McpState};
