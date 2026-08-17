//! Keep the browser renderer stack dependency-free beyond Dioxus itself.
//!
//! The browser UI is Dioxus, generated PDFs are Typst, and transactional email
//! uses direct string templates. A retired HTML renderer must not return
//! through a leaf manifest, a renamed dependency, or a stale lockfile package.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn cargo_manifests() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !matches!(
                    name.as_ref(),
                    "target" | ".git" | ".worktrees" | "node_modules"
                ) {
                    walk(&path, out);
                }
            } else if name == "Cargo.toml" {
                out.push(path);
            }
        }
    }

    let mut manifests = Vec::new();
    walk(&repo_root(), &mut manifests);
    manifests.sort();
    manifests
}

fn dependency_tables<'a>(
    value: &'a toml::Value,
    path: &mut Vec<String>,
    out: &mut Vec<(String, &'a toml::value::Table)>,
) {
    let Some(table) = value.as_table() else {
        return;
    };
    for (key, child) in table {
        path.push(key.clone());
        if matches!(
            key.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        ) {
            if let Some(dependencies) = child.as_table() {
                out.push((path.join("."), dependencies));
            }
        }
        dependency_tables(child, path, out);
        path.pop();
    }
}

#[test]
fn retired_html_renderer_cannot_reenter_the_dependency_graph() {
    let forbidden = ["ma", "ud"].concat();
    let mut offenders = Vec::new();

    for manifest in cargo_manifests() {
        let raw = fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));
        let parsed: toml::Value =
            toml::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", manifest.display()));
        let mut tables = Vec::new();
        dependency_tables(&parsed, &mut Vec::new(), &mut tables);
        for (section, dependencies) in tables {
            for (alias, dependency) in dependencies {
                let package = dependency
                    .as_table()
                    .and_then(|spec| spec.get("package"))
                    .and_then(toml::Value::as_str)
                    .unwrap_or(alias);
                if alias.eq_ignore_ascii_case(&forbidden)
                    || package.eq_ignore_ascii_case(&forbidden)
                {
                    offenders.push(format!("{} [{section}] {alias}", manifest.display()));
                }
            }
        }
    }

    let lock_path = repo_root().join("Cargo.lock");
    let lock_raw = fs::read_to_string(&lock_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", lock_path.display()));
    let lock: toml::Value = toml::from_str(&lock_raw).expect("parse Cargo.lock");
    if lock
        .get("package")
        .and_then(toml::Value::as_array)
        .is_some_and(|packages| {
            packages.iter().any(|package| {
                package
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case(&forbidden))
            })
        })
    {
        offenders.push(lock_path.display().to_string());
    }

    assert!(
        offenders.is_empty(),
        "the retired HTML renderer is present in the dependency graph:\n  {}",
        offenders.join("\n  ")
    );
}
