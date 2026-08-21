//! A server-side commit logs identifiers, never the author's address.
//!
//! The standing order in `telemetry/src/lib.rs` is **identifiers and counts,
//! never content**: a `person_id` may be logged, a person's name or email
//! address may not, because telemetry leaves the firm's trust boundary and
//! client content does not.
//!
//! [`RepoStore::commit_as`] takes an [`repos::Author`] so the commit *object*
//! is attributed to the acting `persons` identity — the matter's audit trail
//! lives in `git log`, and that attribution is the point of the type. What the
//! commit object carries is not what a log line may carry. The address reached
//! a `tracing` field here, and one of the callers
//! (`portal::email_threads::file_attachments`) builds its `Author` from
//! `EmailConversation::external_email` — a **prospective client's** address,
//! the same value that is deliberately kept out of the relay-hold log line.
//!
//! `repo` and `commit` already identify the operation completely, so the
//! address was the one field on that line that named a person and the only one
//! that added nothing. It is gone, and this guard is why it stays gone.
//!
//! ## Why a static scan
//!
//! Asserting on emitted output would need a capturing subscriber, and the
//! nearest crate for that is not a workspace dependency. The invariant is a
//! property of the *source* — no address-shaped expression in a `tracing`
//! field — so the source is what this reads, in the shape
//! `cli/tests/forge_coordinate_retired.rs` established: refuse a forbidden
//! spelling across a named scope, and assert the scope itself is still real so
//! the guard cannot pass by having nothing left to check.

use std::path::{Path, PathBuf};

/// The one file in scope. `repos` is a small crate and the commit path is the
/// only place in it that ever held a person's address.
fn scanned_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")
}

/// Field *values* that resolve to a human's address or name. `author.email` is
/// the spelling that regressed; the rest are the shapes a future edit would
/// reach for to put it back.
const FORBIDDEN_VALUE_FRAGMENTS: &[&str] = &[
    "author.email",
    "author.name",
    ".email",
    "external_email",
    "email.to",
    "email.from",
];

/// Field *keys* that name a person rather than a record.
const FORBIDDEN_KEYS: &[&str] = &[
    "email",
    "recipient",
    "author",
    "sender",
    "principal",
    "external",
    "phone",
];

/// Every `tracing::{info,warn,error,debug,trace}!` invocation in `text`, as
/// (line number, the invocation's source text).
fn tracing_invocations(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let is_macro = ["info", "warn", "error", "debug", "trace"]
            .iter()
            .any(|level| lines[i].contains(&format!("tracing::{level}!")));
        if !is_macro {
            i += 1;
            continue;
        }
        // Accumulate until the invocation's parentheses balance, so a
        // multi-line macro is read whole rather than by its first line.
        let mut depth: i32 = 0;
        let mut buf = String::new();
        let start = i;
        for line in &lines[i..] {
            buf.push_str(line);
            buf.push('\n');
            depth += i32::try_from(line.matches('(').count()).expect("small count")
                - i32::try_from(line.matches(')').count()).expect("small count");
            i += 1;
            if depth <= 0 {
                break;
            }
        }
        found.push((start + 1, buf));
    }
    found
}

/// Strip `//` line comments so a comment *explaining* the rule is not read as
/// a violation of it — this file's own doc comments name `author.email`, and so
/// does the note left at the call site.
fn without_line_comments(invocation: &str) -> String {
    invocation
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn no_tracing_field_in_repos_carries_a_person_address() {
    let path = scanned_file();
    let text = std::fs::read_to_string(&path).expect("repos/src/lib.rs is readable");

    let mut violations = Vec::new();
    for (line, invocation) in tracing_invocations(&text) {
        let code = without_line_comments(&invocation);
        for fragment in FORBIDDEN_VALUE_FRAGMENTS {
            if code.contains(fragment) {
                violations.push(format!(
                    "repos/src/lib.rs:{line}: tracing field reads `{fragment}` — \
                     log an identifier, never a person's address \
                     (telemetry/src/lib.rs)"
                ));
            }
        }
        for key in FORBIDDEN_KEYS {
            // `key = ` or `key,` (the shorthand form) as a structured field.
            if code.contains(&format!("{key} = ")) || code.contains(&format!("%{key},")) {
                violations.push(format!(
                    "repos/src/lib.rs:{line}: tracing field key `{key}` names a person — \
                     log an identifier instead (telemetry/src/lib.rs)"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "server-side commit telemetry must carry identifiers, not addresses:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn the_guard_still_has_something_to_guard() {
    let text = std::fs::read_to_string(scanned_file()).expect("repos/src/lib.rs is readable");

    // The scan is worthless if the line it exists for was renamed or removed
    // and nobody noticed. Both halves must hold: the commit path still logs,
    // and the scanner still finds invocations to inspect.
    assert!(
        text.contains("\"server-side commit\""),
        "the server-side commit log line is gone — retire this guard or point it at the new line"
    );
    let invocations = tracing_invocations(&text);
    assert!(
        !invocations.is_empty(),
        "found no tracing invocations in repos/src/lib.rs — the scanner is broken, not the crate clean"
    );
    assert!(
        invocations
            .iter()
            .any(|(_, body)| body.contains("server-side commit")),
        "the scanner did not reach the server-side commit invocation"
    );
}
