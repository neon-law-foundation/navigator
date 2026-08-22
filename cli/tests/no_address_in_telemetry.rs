//! The no-address-in-telemetry gate — a static scan keeping email addresses
//! out of `tracing` structured fields, enforced by the workspace test suite.
//!
//! ## The invariant
//!
//! Telemetry leaves the firm's trust boundary and an email address is
//! client-identifying content, so the standing order in `telemetry/src/lib.rs`
//! is that an address must never reach a log field. What those fields actually
//! needed the address *for* is carried instead by a kind (did this caller
//! authenticate) and an opaque id (`person_id`).
//!
//! ## Why a scan rather than review
//!
//! Line coverage cannot see this. Every call site that emits a field executes
//! inside the existing suites, so coverage is satisfied while the invariant is
//! completely unasserted — a change that reintroduces an address leaves the
//! suites green. Reading cannot see it reliably either: a manual sweep of these
//! sites missed two, one crate apart, and both were found mechanically in
//! seconds.
//!
//! So the control is mechanical and lives in the required workspace test job.
//!
//! ## What it flags
//!
//! A `tracing::{trace,debug,info,warn,error}!` field whose **value expression**
//! mentions `email`. The field name is not the signal — `principal_kind` is
//! fine, and a field innocently named `recipient` carrying `p.email` is not —
//! so the scan reads the right-hand side.
//!
//! ## What it deliberately does not flag
//!
//! - **The macro's message string.** Prose mentioning the word is not a field.
//! - **Anything outside a `tracing!` invocation.** A struct field, a variable,
//!   or a database column named `email` is ordinary code; only the telemetry
//!   boundary is in scope.
//! - **Test sources.** They carry synthetic fixtures by design, reviewed by
//!   humans rather than by this scan.

use std::path::{Path, PathBuf};

/// The `tracing` macros that emit structured fields.
const TRACING_MACROS: &[&str] = &[
    "tracing::trace!",
    "tracing::debug!",
    "tracing::info!",
    "tracing::warn!",
    "tracing::error!",
];

/// Directories under the workspace root that are not shipped source.
const SKIPPED_DIRS: &[&str] = &[
    "target",
    ".git",
    ".worktrees",
    "node_modules",
    "vendor",
    "tests",
    "examples",
];

/// Sites that emit an address today and are known.
///
/// This list exists so the gate can land without a tree-wide cleanup, and it is
/// a ratchet rather than a permission: a **new** site fails the gate the moment
/// it appears. Every entry is a defect, so an entry being deleted is the
/// expected direction of travel and the list going empty is the goal.
///
/// - `portal/src/a2a.rs` — the `agent_action_authorization` record with
///   `decision = "proposed"` logs the caller's raw address. It is removed by
///   the change that adds `person_id` to that trail; delete this entry with it.
/// - `portal/src/google_oauth.rs` — logs the address the identity provider
///   returned during sign-in.
/// - `portal/src/inbound_email.rs` — logs an inbound message's `from` and `to`.
///
/// The last two are addresses in a log field by the same definition as the
/// first, and whether either is justified is a decision rather than an
/// oversight. Listing them records that the gate sees them; it does not
/// endorse them.
const KNOWN_SITES: &[&str] = &[
    "portal/src/a2a.rs",
    "portal/src/google_oauth.rs",
    "portal/src/inbound_email.rs",
];

/// One flagged field, located for a human.
#[derive(Debug)]
struct Finding {
    file: String,
    line: usize,
    text: String,
}

/// Does this line assign a `tracing` field from an expression mentioning an
/// address?
///
/// Shape: `name = <expr>`, where `<expr>` may carry a `%` or `?` sigil. The
/// message string is rejected because it is not an assignment, and `==` is
/// rejected because it is a comparison rather than a field.
fn flags_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with("//") {
        return false;
    }
    let Some((name, value)) = trimmed.split_once('=') else {
        return false;
    };
    // `==`, `!=`, `>=`, `<=` are comparisons; a field name is a bare ident.
    if value.starts_with('=') || name.ends_with('!') || name.ends_with('>') || name.ends_with('<') {
        return false;
    }
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
    {
        return false;
    }
    let value = value.to_ascii_lowercase();
    if !value.contains("email") {
        return false;
    }
    // `email` in the expression is necessary but not sufficient: a struct named
    // `email` has fields that are not addresses, and an env-var name is a
    // string. Exclude the expressions that demonstrably reduce to something
    // else, so the gate flags the address itself rather than every mention of
    // the word.
    !NON_ADDRESS_MARKERS
        .iter()
        .any(|marker| value.contains(marker))
}

/// Substrings proving a value expression is not an address, even though it
/// mentions one.
///
/// Each entry earned its place against a real site in this workspace:
/// `email.person_id` and `email.template_slug` read other fields off a message
/// struct, `inbound_email_secret.is_some()` is a boolean, `email.dkim` is a
/// verification result, `email.attachments.len()` is a count, and
/// `env::var("NAVIGATOR_EMAIL_BACKEND")` names a variable rather than a person.
const NON_ADDRESS_MARKERS: &[&str] = &[
    "_id",
    "_slug",
    "_secret",
    "dkim",
    "env::var",
    ".len()",
    ".count()",
    ".is_some()",
    ".is_none()",
];

/// Scan one file's `tracing!` invocations.
///
/// Tracks parenthesis depth from the macro name so the walk ends at the real
/// close rather than at the first `)` inside an argument.
fn scan_source(rel: &str, body: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut depth: i64 = 0;
    let mut inside = false;

    for (idx, line) in body.lines().enumerate() {
        if !inside && TRACING_MACROS.iter().any(|m| line.contains(m)) {
            inside = true;
            depth = 0;
        }
        if !inside {
            continue;
        }
        if flags_line(line) {
            findings.push(Finding {
                file: rel.to_string(),
                line: idx + 1,
                text: line.trim().to_string(),
            });
        }
        depth += line.matches('(').count() as i64 - line.matches(')').count() as i64;
        if depth <= 0 {
            inside = false;
        }
    }
    findings
}

/// Walk the workspace's shipped Rust sources.
fn scan(root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if !SKIPPED_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(body) = std::fs::read_to_string(&path) else {
                continue;
            };
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            findings.extend(scan_source(&rel, &body));
        }
    }
    findings
}

/// The workspace root, resolved from this crate's manifest directory so the
/// gate scans the real tree no matter which cwd the test runner uses.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli/ has a parent workspace root")
        .to_path_buf()
}

fn describe(findings: &[&Finding]) -> String {
    findings
        .iter()
        .map(|f| format!("  {}:{} — {}", f.file, f.line, f.text))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn tracing_fields_carry_no_email_address() {
    let root = workspace_root();
    let findings = scan(&root);
    let unexpected: Vec<&Finding> = findings
        .iter()
        .filter(|f| !KNOWN_SITES.contains(&f.file.as_str()))
        .collect();

    assert!(
        unexpected.is_empty(),
        "{} tracing field(s) carry an email address. Telemetry leaves the \
         firm's trust boundary, so an address must never reach a log field \
         (telemetry/src/lib.rs). Log whether the caller authenticated, and \
         `person_id` for who they are:\n{}",
        unexpected.len(),
        describe(&unexpected)
    );
}

/// The ratchet must not rust shut: an entry that no longer flags anything is a
/// fixed site, and leaving it listed would silently re-permit a regression
/// there.
#[test]
fn known_sites_are_still_flagged() {
    let root = workspace_root();
    let findings = scan(&root);
    for site in KNOWN_SITES {
        assert!(
            findings.iter().any(|f| f.file == *site),
            "{site} is listed in KNOWN_SITES but flags nothing — it has been \
             fixed, so delete the entry rather than leaving the gate open on \
             that file"
        );
    }
}

mod line_shapes {
    use super::flags_line;

    #[test]
    fn flags_an_address_bearing_field() {
        assert!(flags_line("            author = author.email,"));
        assert!(flags_line(
            r#"    principal = principal.map_or("<anonymous>", |p| p.email.as_str()),"#
        ));
        assert!(flags_line("    recipient = %p.email,"));
        assert!(flags_line("    who = ?person.email_address,"));
    }

    #[test]
    fn ignores_a_field_that_only_reports_a_kind() {
        assert!(!flags_line("    principal_kind = %principal_kind(addr),"));
        assert!(!flags_line("    person_id = %person_id_field(approver),"));
    }

    /// The macro's message is prose, not a field.
    #[test]
    fn ignores_the_message_string() {
        assert!(!flags_line(
            r#"        "a2a: refusing an email address in a log field","#
        ));
    }

    /// A comparison is not an assignment.
    #[test]
    fn ignores_comparisons_and_comments() {
        assert!(!flags_line("    if principal_email == other.email {"));
        assert!(!flags_line("    // the email never reaches a field"));
    }
}
