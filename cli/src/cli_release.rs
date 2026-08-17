//! `navigator ops cli-release upload` — publish the tagged CLI archives into
//! one deployment's private bucket.
//!
//! The Firm ships no external binary, so `/app/team` is how a firm person
//! gets the CLI. That page lists what is present under `cli-releases/<tag>/` in
//! the deployment's own bucket; this command is what puts it there.
//!
//! **Per deployment, not once globally.** Each deployment reads its own bucket
//! with its own credentials, exactly as it does for documents, so the archives
//! are uploaded once per deployment during that deployment's roll. A single
//! shared copy would mean one project's service account reading another's
//! bucket.
//!
//! **The private bucket.** These archives are handed out behind a role gate.
//! The public assets bucket (photos, fonts) would serve them to anyone with the
//! URL, turning this into a public download page — a thing the Firm does not
//! run. The software is open source; the distribution channel is still the
//! Firm's to choose.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The key prefix `/app/team` lists. Must match
/// `portal::cli_downloads::RELEASE_PREFIX`; the guard test below pins the two
/// together, because they are two crates that never reference each other.
const RELEASE_PREFIX: &str = "cli-releases";

/// Archives are opaque bytes to a browser; the page sets the download filename
/// itself, so the stored type only has to stop a bucket from guessing.
const CONTENT_TYPE: &str = "application/octet-stream";

/// Which files in `dir` are this tag's archives.
///
/// Matched on the exact `navigator-<tag>-` stem rather than a glob, so a
/// directory holding two tags' output cannot publish the wrong one under this
/// tag's prefix — `dist/` is a build directory and has held stale artifacts.
fn archives_for(dir: &Path, tag: &str) -> std::io::Result<Vec<PathBuf>> {
    let stem = format!("navigator-{tag}-");
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&stem))
        })
        .collect();
    found.sort();
    Ok(found)
}

/// Entry point for `ops cli-release upload`.
pub fn run_upload(dir: &Path, tag: &str) -> ExitCode {
    if tag.trim().is_empty() {
        eprintln!("navigator: cli-release upload: --tag must not be empty");
        return ExitCode::from(2);
    }

    let archives = match archives_for(dir, tag) {
        Ok(found) if found.is_empty() => {
            // Publishing nothing silently is how a deployment ends up with a
            // page that says "no release published" after a green roll.
            eprintln!(
                "navigator: cli-release upload: no `navigator-{tag}-*` archives in {}",
                dir.display()
            );
            return ExitCode::from(2);
        }
        Ok(found) => found,
        Err(e) => {
            eprintln!("navigator: cli-release upload: read {}: {e}", dir.display());
            return ExitCode::from(2);
        }
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("navigator: cli-release upload: tokio runtime: {e}");
            return ExitCode::from(2);
        }
    };

    runtime.block_on(async move {
        // `from_env` rather than a GCS client built here: it is the same
        // resolution `web` uses for its own storage, so the archives land in
        // exactly the bucket the running app reads. It also means this command
        // works against the local Garage tier, which is what lets the page be
        // exercised end to end before a release ever cuts.
        let storage = match cloud::from_env().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("navigator: cli-release upload: open storage: {e}");
                return ExitCode::from(2);
            }
        };

        for path in &archives {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let bytes = match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    eprintln!(
                        "navigator: cli-release upload: read {}: {e}",
                        path.display()
                    );
                    return ExitCode::from(2);
                }
            };
            let key = format!("{RELEASE_PREFIX}/{tag}/{name}");
            if let Err(e) = storage.put(&key, &bytes, CONTENT_TYPE).await {
                eprintln!("navigator: cli-release upload: put {key}: {e}");
                return ExitCode::from(2);
            }
            println!("navigator: uploaded {key} ({} bytes)", bytes.len());
        }

        println!(
            "navigator: published {} archive(s) for {tag} under {RELEASE_PREFIX}/{tag}/",
            archives.len()
        );
        ExitCode::SUCCESS
    })
}

#[cfg(test)]
mod tests {
    use super::{archives_for, RELEASE_PREFIX};

    /// The prefix this command writes must be the prefix `/app/team` reads.
    /// Two crates, no shared constant, so the string is pinned on both sides.
    #[test]
    fn the_prefix_matches_what_the_portal_lists() {
        assert_eq!(RELEASE_PREFIX, portal::cli_downloads::RELEASE_PREFIX);
    }

    /// A `dist/` holding two tags publishes only the one asked for. This is the
    /// failure that would put a previous release's bytes behind the current
    /// tag's prefix, where the page would serve them as the current version.
    #[test]
    fn only_the_named_tags_archives_are_selected() {
        let dir = tempfile::tempdir().expect("tempdir");
        for name in [
            "navigator-26.7.27-linux.tar.gz",
            "navigator-26.7.27-windows.zip",
            "navigator-26.6.1-linux.tar.gz",
            "notes.txt",
        ] {
            std::fs::write(dir.path().join(name), b"x").expect("write");
        }

        let found = archives_for(dir.path(), "26.7.27").expect("read dir");
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            names,
            vec![
                "navigator-26.7.27-linux.tar.gz",
                "navigator-26.7.27-windows.zip"
            ],
            "only the asked-for tag's archives may publish under its prefix"
        );
    }

    #[test]
    fn a_directory_without_this_tag_selects_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("navigator-26.6.1-linux.tar.gz"), b"x").expect("write");

        assert!(
            archives_for(dir.path(), "26.7.27")
                .expect("read dir")
                .is_empty(),
            "a missing tag must select nothing, so the caller can fail loudly"
        );
    }
}
