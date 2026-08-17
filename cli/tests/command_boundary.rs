//! Tranche 5 of #355 (`Make REST/OpenAPI the command boundary for data writes`):
//! the machine-caller adapter layers route every user- and tool-initiated write
//! through the shared command boundary, never an inline `SeaORM` write.
//!
//! The `navigator` CLI subcommands and the MCP tools in `mcp/src/tools/` are two
//! of the four callers that converge on the command boundary (the Dioxus runtime
//! and A2A are the others). A CLI subcommand or MCP tool either calls an
//! authenticated `/app/api/*` route over HTTP or — where it cannot depend on the
//! `portal` crate — calls the same shared `store` / `workflows` command the
//! `/api` handler calls. Either way the persistence logic lives in one command,
//! not duplicated inline in the adapter.
//!
//! This test ratchets that invariant: neither `cli/src` nor `mcp/src` may
//! construct an entity `ActiveModel` for a write outside the explicit carve-out
//! allowlist below. A new inline write in an adapter fails here, pointing the
//! author at the shared command instead. The carve-outs are the system/internal
//! paths documented in `docs/command-boundary.md` that are allowed to write
//! directly (schema/catalog provisioning), and the allowlist is kept honest by a
//! companion test that fails if a listed file stops writing.

use std::fs;
use std::path::{Path, PathBuf};

/// The workspace root (this test crate is `cli`).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The adapter source trees the command boundary governs — the machine callers.
/// Repo-relative, forward-slashed.
const ADAPTER_ROOTS: &[&str] = &["cli/src", "mcp/src"];

/// Files legitimately allowed to write directly — the system/internal carve-outs
/// documented in `docs/command-boundary.md`. Repo-relative, forward-slashed.
///
/// Empty since ENG-121. The last entry was `cli/src/import.rs`, exempted for one
/// inline write that registered auto-import question-code stubs so the N104
/// validation pass had a populated catalog to check against. `questions` moved to
/// `SurrealDB` in that slice, and the write now routes through
/// `store::questions::find_or_create` like every other command — so the exemption
/// has nothing left to cover, and `every_carve_out_still_writes_and_exists` is
/// what caught that.
const CARVE_OUTS: &[&str] = &[];

/// The ORM write signal this workspace uses in adapters: constructing an entity
/// `ActiveModel` literal, which is only ever `.insert`/`.update`/`.delete`-ed.
/// Reads go through `Entity::find`, so a bare `::ActiveModel {` is a write.
const WRITE_SIGNAL: &str = "::ActiveModel {";

/// Every `.rs` file under `dir`, recursively.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

/// Strip every `#[cfg(test)]`-attributed module body so test-only seeding never
/// counts as an adapter write. Brace-matched rather than truncate-at-first-marker
/// so a write below a test module is still caught.
fn strip_test_modules(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if src[i..].starts_with("#[cfg(test)]") {
            // Find the module's opening brace, then brace-match to its close.
            if let Some(brace_rel) = src[i..].find('{') {
                let mut depth = 0usize;
                let mut j = i + brace_rel;
                while j < bytes.len() {
                    match bytes[j] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                j += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                i = j;
                continue;
            }
        }
        let ch = src[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Repo-relative, forward-slashed path for a scanned file.
fn rel(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The machine adapters carry no inline entity write outside the carve-outs.
#[test]
fn cli_and_mcp_adapters_route_writes_through_the_command_boundary() {
    let mut scanned = 0usize;
    let mut offenders: Vec<String> = Vec::new();

    for root in ADAPTER_ROOTS {
        for path in rust_sources(&repo_root().join(root)) {
            let relpath = rel(&path);
            if CARVE_OUTS.contains(&relpath.as_str()) {
                continue;
            }
            let body = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let code = strip_test_modules(&body);
            scanned += 1;
            for (i, line) in code.lines().enumerate() {
                if line.contains(WRITE_SIGNAL) {
                    offenders.push(format!("{relpath}:{}", i + 1));
                }
            }
        }
    }

    assert!(
        scanned > 40,
        "expected to scan the whole CLI + MCP adapter surface, only saw {scanned} \
         files — the walk is probably rooted wrong"
    );
    assert!(
        offenders.is_empty(),
        "these CLI/MCP adapters construct an entity ActiveModel inline instead of \
         calling a shared store/workflows/API command (see docs/command-boundary.md); \
         route the write through the command boundary, or add the file to CARVE_OUTS \
         with a documented reason if it is genuine system/catalog provisioning:\n  {}",
        offenders.join("\n  ")
    );
}

/// The carve-out allowlist does not rot: every listed file exists and still
/// carries the write it is excused for, so a stale exemption cannot silently
/// widen the hole.
#[test]
fn every_carve_out_still_writes_and_exists() {
    for rel_path in CARVE_OUTS {
        let path = repo_root().join(rel_path);
        let body = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("carve-out {rel_path} is unreadable: {e}"));
        assert!(
            body.contains(WRITE_SIGNAL),
            "carve-out `{rel_path}` no longer constructs an entity ActiveModel — drop \
             it from CARVE_OUTS so the exemption cannot mask a future inline write"
        );
    }
}

/// The stripper actually removes `#[cfg(test)]` bodies and keeps the rest, so a
/// test-module seed never trips the ratchet while real code still does.
#[test]
fn strip_test_modules_removes_only_the_test_body() {
    let src = "\
fn real() { let _ = person::ActiveModel { ..Default::default() }; }
#[cfg(test)]
mod tests {
    fn seed() { let _ = entity::ActiveModel { ..Default::default() }; }
}
fn also_real() {}
";
    let stripped = strip_test_modules(src);
    assert!(
        stripped.contains("person::ActiveModel {"),
        "real-code write must survive stripping"
    );
    assert!(
        !stripped.contains("entity::ActiveModel {"),
        "test-module write must be stripped"
    );
    assert!(
        stripped.contains("fn also_real"),
        "code after the test module must survive stripping"
    );
}
