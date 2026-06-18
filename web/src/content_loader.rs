//! Shared error type for the static-content loaders (`workshops`,
//! `marketing`).
//!
//! Each module reads a directory of `.md` files into an in-memory
//! index at boot. They all surface the same two failure modes, so
//! they share one error type rather than three carbon-copy enums.

use std::io;

/// Failure modes for the static-content loaders.
///
/// - `Io` covers the directory-read, file-read, and entry-iterate
///   errors that any of the loaders can hit.
/// - `MissingFrontmatter` is raised by loaders that require a
///   `---`-delimited YAML head on every file (marketing). The
///   workshops loader uses a static manifest and never returns this
///   variant.
#[derive(Debug, thiserror::Error)]
pub enum ContentLoadError {
    #[error("io error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("content file at {path} is missing front-matter")]
    MissingFrontmatter { path: String },
}
