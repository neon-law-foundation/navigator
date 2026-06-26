//! Extraction grounding test: every `navigator …` command the workshop docs
//! and the `cli` crate README publish must parse against the real binary.
//!
//! `web/content/workshops/navigator/{README,DEPLOY}.md` and `cli/README.md`
//! teach concrete `navigator` command sequences. That prose is a public
//! promise, and nothing stops it drifting from the binary — a renamed flag, a
//! dropped subcommand, or a command written with the wrong shape (a positional
//! host where `login` requires `--host`). This test pulls every `navigator`
//! invocation out of the fenced ` ```bash ` blocks and asserts each one parses.
//!
//! Validation is parse-only and side-effect-free: each extracted command is
//! invoked with `--help` appended, which makes clap short-circuit before any
//! network or browser I/O while still rejecting an unknown flag or an
//! unexpected positional argument. Angle-bracket placeholders (`<notation-id>`,
//! `<your-host>`) are normalized to a nil UUID so a typed positional (the
//! `NOTATION_ID` UUID) parses as a value rather than failing validation.
//!
//! The complementary `workshop_llc_grounding.rs` pins the *semantics* of the
//! LLC section (template code, staff-gated filing step); this test pins the
//! *syntax* of every published command across both workshop pages.

use std::path::Path;
use std::process::Command;

/// A nil UUID standing in for any `<…>` placeholder, so a typed positional
/// such as `<notation-id>` parses as a value instead of failing UUID
/// validation.
const PLACEHOLDER: &str = "00000000-0000-0000-0000-000000000000";

/// The docs whose `navigator` commands are grounded against the binary.
const DOCS: &[&str] = &[
    "web/content/workshops/navigator/README.md",
    "web/content/workshops/navigator/DEPLOY.md",
    "cli/README.md",
];

/// Read a repo-root file relative to this crate (`cli/` → workspace root is
/// one level up).
fn repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {} — {e}", path.display()))
}

/// Every `navigator …` invocation inside a fenced ` ```bash ` block, with
/// backslash-continued lines joined and any trailing ` # …` comment dropped.
fn navigator_commands(md: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_bash = false;
    let mut pending = String::new();
    for raw in md.lines() {
        let fence = raw.trim();
        if fence.starts_with("```") {
            in_bash = fence == "```bash";
            pending.clear();
            continue;
        }
        if !in_bash {
            continue;
        }
        let line = raw.trim_end();
        if let Some(prefix) = line.strip_suffix('\\') {
            pending.push_str(prefix.trim_start());
            pending.push(' ');
            continue;
        }
        let full = if pending.is_empty() {
            line.trim_start().to_string()
        } else {
            let joined = format!("{pending}{}", line.trim_start());
            pending.clear();
            joined
        };
        if let Some(rest) = strip_comment(&full).trim().strip_prefix("navigator ") {
            commands.push(rest.trim().to_string());
        }
    }
    commands
}

/// Drop a trailing ` # comment`. The published commands carry no `#` inside a
/// value, so the first ` #` begins the comment.
fn strip_comment(line: &str) -> &str {
    match line.find(" #") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// Split a shell-ish argument string on whitespace, honoring single and double
/// quotes, and normalize each `<…>` placeholder to the nil UUID.
fn tokenize(args: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in args.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                cur.push(c);
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
            started = true;
        } else if c.is_whitespace() {
            if started {
                tokens.push(normalize(&std::mem::take(&mut cur)));
                started = false;
            }
        } else {
            cur.push(c);
            started = true;
        }
    }
    if started {
        tokens.push(normalize(&cur));
    }
    tokens
}

/// Replace an angle-bracket placeholder (`<notation-id>`) with the nil UUID.
fn normalize(token: &str) -> String {
    if token.starts_with('<') && token.ends_with('>') {
        PLACEHOLDER.to_string()
    } else {
        token.to_string()
    }
}

/// True when the documented command is a meta-placeholder rather than a
/// runnable invocation — `navigator <COMMAND>` (subcommand normalized to the
/// placeholder UUID) or a bare `navigator --help` / `--version`.
fn is_meta(tokens: &[String]) -> bool {
    match tokens.first() {
        None => true,
        Some(first) => first == PLACEHOLDER || first.starts_with('-'),
    }
}

/// Run `navigator <tokens> --help` and return `(parsed_ok, output)`. `--help`
/// short-circuits clap before any network or browser I/O, but clap still
/// rejects an unknown flag or an unexpected positional first.
fn parses(tokens: &[String]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_navigator"))
        .args(tokens)
        .arg("--help")
        .output()
        .expect("run navigator --help");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    (out.status.success(), text)
}

#[test]
fn every_published_navigator_command_parses() {
    let mut checked = 0_usize;
    let mut failures = Vec::new();
    for doc in DOCS {
        for command in navigator_commands(&repo_file(doc)) {
            let tokens = tokenize(&command);
            if is_meta(&tokens) {
                continue;
            }
            checked += 1;
            let (ok, output) = parses(&tokens);
            if !ok {
                failures.push(format!("{doc}: `navigator {command}`\n{}", output.trim()));
            }
        }
    }
    assert!(
        checked > 0,
        "extracted no navigator commands — the extractor or the docs changed shape",
    );
    assert!(
        failures.is_empty(),
        "{} published navigator command(s) no longer parse against the binary:\n\n{}",
        failures.len(),
        failures.join("\n\n"),
    );
}

#[test]
fn the_deploy_workshop_promises_no_phantom_bin_wrapper() {
    // DEPLOY.md once told readers to `export PATH="$PWD/bin:$PATH"` and call a
    // `bin/navigator` wrapper that never shipped — there is no `bin/` dir, and a
    // shell wrapper would violate the Rust-only invariant anyway. The
    // skip-install path is `cargo run -p cli -- <args>`.
    let deploy = repo_file("web/content/workshops/navigator/DEPLOY.md");
    assert!(
        !deploy.contains("$PWD/bin"),
        "DEPLOY.md points readers at a `$PWD/bin` wrapper that does not ship — \
         use `cargo run -p cli -- <args>` for the skip-install path",
    );
}
