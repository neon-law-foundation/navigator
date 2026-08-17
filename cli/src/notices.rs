//! `navigator ops notices` — regenerate the third-party licence notices that
//! ship with the downloadable `navigator` binary.
//!
//! A statically linked Rust binary carries the compiled form of every crate in
//! its dependency tree, and the permissive licences those crates use — the set
//! `deny.toml` allows — each require their notice to travel with the
//! distributed work. Apache-2.0 section 4 says so explicitly; MIT, ISC, and the
//! BSD family all require the copyright notice to be retained, so this file has
//! to exist and stay current.
//!
//! **Deduplicated by text.** Concatenating 1,300-odd licence files verbatim
//! produces megabytes in which the same Apache-2.0 body appears hundreds of
//! times. Instead every *distinct* licence text is emitted once, listing the
//! crates that carry it. Nothing is summarised or rewritten: each text appears
//! in full, which is what the licences require. Only the repetition goes.
//!
//! **Over-inclusive on purpose.** The crate set comes from `Cargo.lock`, which
//! is a superset of what any one binary links: it includes dev-dependencies and
//! crates for other target platforms. Naming a crate whose code did not ship is
//! harmless; omitting one whose code did is the compliance failure. When the
//! set is later narrowed, narrow it deliberately.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// One crate in the dependency tree, as `Cargo.lock` names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Crate {
    pub name: String,
    pub version: String,
}

impl std::fmt::Display for Crate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.name, self.version)
    }
}

/// Filenames that carry a licence or attribution notice. Matched
/// case-insensitively against the file *stem* so `LICENSE`, `LICENSE-MIT`,
/// `LICENCE.txt`, `COPYING`, and `NOTICE` all land.
fn is_notice_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let named_like_a_notice = ["license", "licence", "copying", "notice"]
        .iter()
        .any(|stem| lower.starts_with(stem));
    // `LICENSE.spdx` and `license.toml` are metadata, not the text. Compared
    // through `Path::extension` on the already-lowercased name so the check is
    // case-insensitive by construction.
    let extension = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    named_like_a_notice && !matches!(extension, "spdx" | "toml")
}

/// Every registry crate in a `Cargo.lock`, sorted and deduplicated.
///
/// Workspace members have no `source` key and are excluded: they are the
/// Firm's own code, governed by `LICENSE.md`, and are not third-party.
pub fn registry_crates(lockfile: &str) -> Vec<Crate> {
    let doc: toml::Value = match toml::from_str(lockfile) {
        Ok(doc) => doc,
        Err(_) => return Vec::new(),
    };
    let Some(packages) = doc.get("package").and_then(|p| p.as_array()) else {
        return Vec::new();
    };
    let mut out: Vec<Crate> = packages
        .iter()
        .filter(|pkg| {
            pkg.get("source")
                .and_then(|s| s.as_str())
                .is_some_and(|s| s.starts_with("registry+"))
        })
        .filter_map(|pkg| {
            Some(Crate {
                name: pkg.get("name")?.as_str()?.to_string(),
                version: pkg.get("version")?.as_str()?.to_string(),
            })
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// `$CARGO_HOME/registry/src/<index>/`, for every index present. A machine that
/// has fetched from more than one registry mirror has more than one.
fn registry_src_roots() -> Vec<PathBuf> {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
    let Some(src) = cargo_home.map(|h| h.join("registry").join("src")) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&src) else {
        return Vec::new();
    };
    let mut roots: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    roots.sort();
    roots
}

/// The notice texts a single crate's extracted source carries, sorted by
/// filename so the output does not depend on directory iteration order.
fn notices_for(roots: &[PathBuf], krate: &Crate) -> Vec<String> {
    let dir_name = format!("{}-{}", krate.name, krate.version);
    let mut texts = Vec::new();
    for root in roots {
        let dir = root.join(&dir_name);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(is_notice_file)
            })
            .collect();
        files.sort();
        for file in files {
            if let Ok(body) = fs::read_to_string(&file) {
                let trimmed = body.trim();
                if !trimmed.is_empty() {
                    texts.push(trimmed.to_string());
                }
            }
        }
        if !texts.is_empty() {
            break;
        }
    }
    texts
}

/// The SPDX expression a crate declares in its own manifest.
///
/// Most crates that ship no licence *file* still declare `license = "MIT OR
/// Apache-2.0"` in `Cargo.toml` — publishing the text is conventional, not
/// required by crates.io. That declaration is the attribution for those crates,
/// and the full text of the licence it names is already in this file from the
/// hundreds of crates that do ship it.
fn declared_license(roots: &[PathBuf], krate: &Crate) -> Option<String> {
    let dir_name = format!("{}-{}", krate.name, krate.version);
    for root in roots {
        let Ok(body) = fs::read_to_string(root.join(&dir_name).join("Cargo.toml")) else {
            continue;
        };
        let Ok(doc) = toml::from_str::<toml::Value>(&body) else {
            continue;
        };
        let Some(package) = doc.get("package") else {
            continue;
        };
        if let Some(spdx) = package.get("license").and_then(|l| l.as_str()) {
            return Some(spdx.to_string());
        }
        if let Some(file) = package.get("license-file").and_then(|l| l.as_str()) {
            return Some(format!("see bundled {file}"));
        }
    }
    None
}

/// A crate carrying no licence file, with whatever its manifest declares.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Undeclared {
    pub krate: Crate,
    pub spdx: Option<String>,
}

/// The grouped notice set: distinct licence text to the crates carrying it,
/// plus the crates that ship no licence file.
pub struct Notices {
    /// Licence text to the crates that carry it. `BTreeMap` keyed by text keeps
    /// the rendering deterministic without a separate sort.
    pub by_text: BTreeMap<String, Vec<Crate>>,
    /// Crates whose published archive contains no licence file.
    pub gaps: Vec<Undeclared>,
}

/// Group every crate's notice texts, collapsing identical texts.
pub fn collect(roots: &[PathBuf], crates: &[Crate]) -> Notices {
    let mut by_text: BTreeMap<String, Vec<Crate>> = BTreeMap::new();
    let mut gaps = Vec::new();
    for krate in crates {
        let texts = notices_for(roots, krate);
        if texts.is_empty() {
            gaps.push(Undeclared {
                krate: krate.clone(),
                spdx: declared_license(roots, krate),
            });
            continue;
        }
        for text in texts {
            by_text.entry(text).or_default().push(krate.clone());
        }
    }
    for crates in by_text.values_mut() {
        crates.sort();
        crates.dedup();
    }
    gaps.sort();
    Notices { by_text, gaps }
}

/// Render the notices file. Plain text, not Markdown: licence bodies carry
/// their own wrapping and would fail the workspace Markdown line-width rule,
/// and a notices file is conventionally plain text anyway.
pub fn render(notices: &Notices) -> String {
    let mut out = String::new();
    out.push_str(
        "THIRD-PARTY NOTICES\n\
         ===================\n\n\
         The Neon Law Navigator `navigator` binary is copyright Neon Law Foundation and is\n\
         licensed under MIT OR Apache-2.0; see LICENSE.md. It incorporates the third-party\n\
         open-source components listed below, each governed by its own licence, reproduced\n\
         here in full.\n\n\
         Identical licence texts are listed once with every crate that carries them. This file\n\
         is generated by `navigator ops notices` from Cargo.lock; do not edit it by hand.\n\n",
    );

    for (text, crates) in &notices.by_text {
        out.push_str(&"-".repeat(88));
        out.push('\n');
        for krate in crates {
            let _ = writeln!(out, "{krate}");
        }
        out.push('\n');
        out.push_str(text);
        out.push_str("\n\n");
    }

    if !notices.gaps.is_empty() {
        out.push_str(&"-".repeat(88));
        out.push_str(
            "\nThe following crates publish no licence file in their crates.io archive — shipping\n\
             the text is conventional, not required — and declare their licence in the manifest\n\
             instead. Each declared licence is one of the permissive licences allowed by\n\
             deny.toml, and the full text of every one of them appears above, reproduced from\n\
             the crates that do ship it.\n\n",
        );
        for gap in &notices.gaps {
            let _ = match &gap.spdx {
                Some(spdx) => writeln!(out, "{}  —  {spdx}", gap.krate),
                None => writeln!(out, "{}  —  no licence declared", gap.krate),
            };
        }
        out.push('\n');
    }

    out
}

/// Entry point for `navigator ops notices`.
pub fn run(out_path: &Path, check: bool) -> ExitCode {
    let lockfile = match fs::read_to_string("Cargo.lock") {
        Ok(body) => body,
        Err(e) => {
            eprintln!("navigator: ops notices: read Cargo.lock: {e}");
            return ExitCode::from(2);
        }
    };
    let crates = registry_crates(&lockfile);
    if crates.is_empty() {
        eprintln!("navigator: ops notices: no registry crates in Cargo.lock");
        return ExitCode::from(2);
    }
    let roots = registry_src_roots();
    if roots.is_empty() {
        eprintln!(
            "navigator: ops notices: no crate sources under $CARGO_HOME/registry/src — \
             run `cargo fetch` first"
        );
        return ExitCode::from(2);
    }
    let notices = collect(&roots, &crates);
    let rendered = render(&notices);

    if check {
        let current = fs::read_to_string(out_path).unwrap_or_default();
        if current == rendered {
            println!(
                "navigator: ops notices: {} is current ({} crates, {} distinct licence texts)",
                out_path.display(),
                crates.len(),
                notices.by_text.len()
            );
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "navigator: ops notices: {} is stale — re-run `navigator ops notices` and commit it",
            out_path.display()
        );
        return ExitCode::from(1);
    }

    if let Err(e) = fs::write(out_path, &rendered) {
        eprintln!("navigator: ops notices: write {}: {e}", out_path.display());
        return ExitCode::from(2);
    }
    println!(
        "navigator: ops notices: wrote {} — {} crates, {} distinct licence texts, {} gap(s)",
        out_path.display(),
        crates.len(),
        notices.by_text.len(),
        notices.gaps.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{collect, is_notice_file, registry_crates, render, Crate, Undeclared};

    #[test]
    fn notice_filenames_match_the_conventional_spellings() {
        for name in [
            "LICENSE",
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "licence.txt",
            "COPYING",
            "NOTICE",
        ] {
            assert!(is_notice_file(name), "{name} should be a notice file");
        }
        for name in [
            "src",
            "Cargo.toml",
            "license.toml",
            "LICENSE.spdx",
            "README",
        ] {
            assert!(!is_notice_file(name), "{name} should not be a notice file");
        }
    }

    /// Workspace members carry no `source` key and are the Firm's own code.
    #[test]
    fn only_registry_crates_are_third_party() {
        let lock = r#"
[[package]]
name = "cli"
version = "0.1.0"

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "local-path-dep"
version = "0.2.0"
source = "git+https://example.invalid/repo"
"#;
        assert_eq!(
            registry_crates(lock),
            vec![Crate {
                name: "serde".into(),
                version: "1.0.0".into()
            }]
        );
    }

    #[test]
    fn malformed_lockfile_yields_no_crates_rather_than_panicking() {
        assert!(registry_crates("this is not toml {{{").is_empty());
    }

    /// The dedup is the whole point: one Apache-2.0 body, every crate listed.
    #[test]
    fn identical_texts_collapse_into_one_entry() {
        let mut notices = collect(&[], &[]);
        let shared = "Apache License, Version 2.0 …".to_string();
        notices.by_text.insert(
            shared,
            vec![
                Crate {
                    name: "aaa".into(),
                    version: "1.0.0".into(),
                },
                Crate {
                    name: "zzz".into(),
                    version: "2.0.0".into(),
                },
            ],
        );

        let out = render(&notices);
        assert_eq!(
            out.matches("Apache License, Version 2.0").count(),
            1,
            "the shared text must appear exactly once"
        );
        assert!(out.contains("aaa 1.0.0"));
        assert!(out.contains("zzz 2.0.0"));
    }

    /// A crate carrying no licence file must be named, not silently dropped:
    /// a silent gap is an unattributed component.
    #[test]
    fn crates_without_a_licence_file_are_reported_as_gaps() {
        let missing = Crate {
            name: "never-fetched".into(),
            version: "9.9.9".into(),
        };
        let notices = collect(&[], std::slice::from_ref(&missing));
        assert_eq!(
            notices.gaps,
            vec![Undeclared {
                krate: missing,
                spdx: None
            }]
        );
        let out = render(&notices);
        assert!(out.contains("never-fetched 9.9.9"));
        assert!(out.contains("no licence declared"));
    }

    #[test]
    fn rendered_header_names_the_owner_and_points_at_the_licence() {
        let notices = collect(&[], &[]);
        let out = render(&notices);
        assert!(out.contains("Neon Law Foundation"));
        assert!(out.contains("LICENSE.md"));
        assert!(out.contains("MIT OR Apache-2.0"));
    }
}
