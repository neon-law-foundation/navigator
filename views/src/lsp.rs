//! The prebuilt-`navigator-lsp` target registry — the single source of
//! truth shared by the publisher (`cli lsp publish`) and the download
//! buttons on the [`/lsp`](crate::pages::lsp) page.
//!
//! A target is a Rust triple plus the reader-facing executable name and
//! platform label. The object key a binary lands at in the public assets bucket
//! ([`lsp_binary_key`]) is derived from the triple, so the upload path
//! and the download link can never drift — exactly the
//! [`GALLERY`](crate::assets) / [`WIDTHS`](crate::assets) pattern, one
//! tier down. The download URL itself is
//! `asset_url(&lsp_binary_key(target))`, which resolves to the public
//! `<project>-assets` bucket in production and to `/public` in dev.

/// One distributable platform for the `navigator-lsp` binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspTarget {
    /// Rust target triple — `cargo build --release --target <triple>`
    /// emits the matching binary, and it forms the object key.
    pub triple: &'static str,
    /// The compiled executable name for this platform.
    pub binary_name: &'static str,
    /// Reader-facing platform label for the download button.
    pub label: &'static str,
}

/// The platforms a Zed (or any LSP-aware editor) user runs on. A
/// `navigator lsp publish` pushes whichever of these it finds built; the
/// page renders a download button for each.
pub const LSP_TARGETS: &[LspTarget] = &[
    LspTarget {
        triple: "aarch64-apple-darwin",
        binary_name: "navigator-lsp",
        label: "macOS · Apple Silicon",
    },
    LspTarget {
        triple: "x86_64-apple-darwin",
        binary_name: "navigator-lsp",
        label: "macOS · Intel",
    },
    LspTarget {
        triple: "x86_64-unknown-linux-gnu",
        binary_name: "navigator-lsp",
        label: "Linux · x86-64",
    },
    LspTarget {
        triple: "aarch64-unknown-linux-gnu",
        binary_name: "navigator-lsp",
        label: "Linux · ARM64",
    },
    LspTarget {
        triple: "x86_64-pc-windows-msvc",
        binary_name: "navigator-lsp.exe",
        label: "Windows · x86-64",
    },
];

/// The object key (and `/public`-relative asset path) a target's binary
/// lives at: `lsp/<triple>/<binary_name>`. Stable "latest" path — a
/// re-publish overwrites it, so the publisher stamps a *bounded*
/// `Cache-Control`, never `immutable`.
#[must_use]
pub fn lsp_binary_key(target: LspTarget) -> String {
    format!("lsp/{}/{}", target.triple, target.binary_name)
}

#[cfg(test)]
mod tests {
    use super::{lsp_binary_key, LSP_TARGETS};

    #[test]
    fn key_is_triple_scoped_under_lsp() {
        let mac = LSP_TARGETS
            .iter()
            .find(|target| target.triple == "aarch64-apple-darwin")
            .copied()
            .expect("mac target");
        assert_eq!(
            lsp_binary_key(mac),
            "lsp/aarch64-apple-darwin/navigator-lsp"
        );
    }

    #[test]
    fn windows_key_uses_exe_suffix() {
        let windows = LSP_TARGETS
            .iter()
            .find(|target| target.triple == "x86_64-pc-windows-msvc")
            .copied()
            .expect("windows target");
        assert_eq!(
            lsp_binary_key(windows),
            "lsp/x86_64-pc-windows-msvc/navigator-lsp.exe"
        );
    }

    #[test]
    fn registry_covers_mac_linux_and_windows() {
        let triples: Vec<_> = LSP_TARGETS.iter().map(|t| t.triple).collect();
        for expected in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
        ] {
            assert!(triples.contains(&expected), "missing target {expected}");
        }
    }

    #[test]
    fn triples_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for t in LSP_TARGETS {
            assert!(seen.insert(t.triple), "duplicate triple {}", t.triple);
        }
    }
}
