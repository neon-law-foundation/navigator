//! The `validate-i18n` static key-check — the `i18n-tasks` analog, in
//! Rust.
//!
//! The English catalog (`views::i18n`, the `DOMAINS` table) is the single
//! source of UI copy. Every catalog key should be wired up at exactly the
//! places that render it, and every render/test site should name a key
//! that exists. Two ways that agreement can rot, each its own gate
//! failure:
//!
//! - **Missing** — a call site names a key with no catalog entry (a typo
//!   or a deleted key). `t` silently renders the key string in
//!   production; `t_strict` panics. Either way the page is wrong; the
//!   check catches it before it ships.
//! - **Unused** — a catalog key nothing references. Dead copy that drifts
//!   from the page and bloats the es-parity surface.
//!
//! This is the same "catalog/code agreement enforced by a check" pattern
//! as the `every_localized_en_key_is_translated_in_es_or_explicitly_waived`
//! guard in `views/src/i18n.rs`, pointed the other way: code ↔ catalog
//! rather than en ↔ es. It is a `navigator` subcommand (not a fourth CI
//! workflow), wired into the `ci` gate next to `validate` / `validate-yaml`.
//!
//! ## What counts as a reference
//!
//! Keys are only named from Rust, so the audit scans `.rs` under
//! [`SOURCE_ROOTS`]. A key is **referenced** when it appears as the key
//! argument of a recognized call form:
//!
//! - `t(locale, "key")`, `t_args(locale, "key", …)`, `t_strict(locale,
//!   "key")` — the key is the first string literal after the locale.
//!   Filtered to genuine i18n calls by requiring the first argument to
//!   look like a locale (contains `ocale`), so an unrelated `t(...)` is
//!   ignored.
//! - `assert_renders!(body, "key")` / `assert_renders!(body, locale,
//!   "key")` and `assert_absent!(…)` — the key is the last string literal
//!   (`body`/`locale` are never literals).
//!
//! A key resolved through a `code → "products.desc_*"` style match (e.g.
//! `web::product_description_key`) or a `label → "nav.*"` map (e.g.
//! `views::i18n::nav_label`) still carries the destination key as a string
//! literal in that source, so the **unused** check — which asks only
//! whether the quoted key literal appears anywhere in the scanned
//! sources — sees it. The **missing** check looks only at the call forms
//! above, where the key is named inline.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::palette;

/// Source roots scanned for key references, relative to the workspace
/// root. Keys are only ever named from Rust.
const SOURCE_ROOTS: &[&str] = &["views/src", "web/src", "web/tests"];

/// Keys deliberately named while absent from the catalog: the sentinels
/// the resolver's own unit tests use to exercise the missing-key fallback
/// (`t`) and panic (`t_strict`) paths. They are not typos, so the
/// **missing** check exempts them — the same "explicit waiver, never a
/// silent pass" shape as the es-parity waiver in `views/src/i18n.rs`. Add
/// a key here ONLY for that reason.
const INTENTIONALLY_ABSENT: &[&str] = &["does.not.exist"];

/// One key named at a call site: the dotted key and where it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRef {
    pub key: String,
    pub path: PathBuf,
    pub line: usize,
}

/// The reconciliation of catalog ↔ call sites.
#[derive(Debug, Default)]
pub struct Audit {
    /// Keys named at a call site but absent from the English catalog.
    pub missing: Vec<KeyRef>,
    /// Catalog keys whose literal appears at no scanned call site.
    pub unused: Vec<String>,
    pub files_scanned: usize,
    pub keys_in_catalog: usize,
}

/// Run the audit over `root`, print the report, and return a process exit
/// code: `SUCCESS` when clean, `1` on drift (missing or unused keys), `2`
/// on an I/O error walking or reading a source.
pub fn run(root: &Path) -> ExitCode {
    match audit_and_report(root) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(err) => {
            eprintln!("navigator: {err}");
            ExitCode::from(2)
        }
    }
}

/// Audit `root` against the English catalog and print the report. Returns
/// `Ok(true)` when the catalog and the call sites agree, `Ok(false)` when
/// anything is missing or unused, and `Err` only on an I/O failure.
fn audit_and_report(root: &Path) -> std::io::Result<bool> {
    let catalog = views::i18n::en_catalog_keys();
    let audit = audit_workspace(root, &catalog)?;

    for r in &audit.missing {
        crate::print_violation(
            &r.path.display().to_string(),
            r.line,
            "I18N-MISSING",
            &format!(
                "{:?} is named here but has no catalog entry — add it to \
                 views/locales/en/<domain>.yml or fix the key",
                r.key
            ),
        );
    }
    for key in &audit.unused {
        // No file:line — an unused key is an absence, located in the
        // catalog rather than at a call site.
        let message = format!(
            "{key:?} is in the catalog but referenced nowhere under {} — \
             wire it up or delete it",
            SOURCE_ROOTS.join(", "),
        );
        println!(
            "{} {}: {message}",
            palette::dim("views/locales/en/<domain>.yml"),
            palette::highlight("I18N-UNUSED"),
        );
    }

    println!(
        "{}",
        palette::dim(format!(
            "Scanned {} catalog key(s) against {} source file(s): {} missing, {} unused",
            audit.keys_in_catalog,
            audit.files_scanned,
            audit.missing.len(),
            audit.unused.len(),
        ))
    );

    Ok(audit.missing.is_empty() && audit.unused.is_empty())
}

/// Walk [`SOURCE_ROOTS`] under `root`, collect every referenced key, and
/// reconcile against `catalog`.
fn audit_workspace(root: &Path, catalog: &BTreeSet<String>) -> std::io::Result<Audit> {
    let mut refs: Vec<KeyRef> = Vec::new();
    let mut contents: Vec<String> = Vec::new();
    let mut files_scanned = 0usize;

    for source_root in SOURCE_ROOTS {
        let dir = root.join(source_root);
        if !dir.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&dir) {
            let entry = entry?;
            if !entry.file_type().is_file() || !is_rust_path(entry.path()) {
                continue;
            }
            files_scanned += 1;
            let src = std::fs::read_to_string(entry.path())?;
            let rel = entry.path().to_path_buf();
            for (line, key) in extract_refs(&src) {
                refs.push(KeyRef {
                    key,
                    path: rel.clone(),
                    line,
                });
            }
            // The unused check substring-searches `contents`; strip comments
            // here too so both directions see the same comment-free source.
            // Otherwise a key named only in a comment (`// TODO: "x.y"`) would
            // count as "used" and hide a genuinely unused key.
            contents.push(strip_comments(&src));
        }
    }

    let mut missing: Vec<KeyRef> = refs
        .into_iter()
        .filter(|r| !catalog.contains(&r.key) && !INTENTIONALLY_ABSENT.contains(&r.key.as_str()))
        .collect();
    missing.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then_with(|| a.key.cmp(&b.key))
    });

    let unused: Vec<String> = catalog
        .iter()
        .filter(|key| {
            let quoted = format!("\"{key}\"");
            !contents.iter().any(|c| c.contains(&quoted))
        })
        .cloned()
        .collect();

    Ok(Audit {
        missing,
        unused,
        files_scanned,
        keys_in_catalog: catalog.len(),
    })
}

fn is_rust_path(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// One recognized call form: the function/macro name to match and how its
/// key argument is positioned.
struct Marker {
    /// Literal that opens the call, e.g. `t(` or `assert_renders!(`. The
    /// trailing `(` anchors it to a call.
    head: &'static str,
    /// `true` for the `t`-family (key is the first literal *after* the
    /// locale arg, which must look like a locale); `false` for the
    /// assertion macros (key is the last string literal).
    locale_first: bool,
}

const MARKERS: &[Marker] = &[
    Marker {
        head: "t_strict(",
        locale_first: true,
    },
    Marker {
        head: "t_args(",
        locale_first: true,
    },
    Marker {
        head: "t(",
        locale_first: true,
    },
    Marker {
        head: "assert_renders!(",
        locale_first: false,
    },
    Marker {
        head: "assert_absent!(",
        locale_first: false,
    },
];

/// Extract every i18n key named in `src`, paired with its 1-based line.
///
/// Comments are blanked first (preserving line numbers) so a key shown in
/// a doc-comment example — `/// t(locale, "key")` — is never mistaken for
/// a live call site.
fn extract_refs(src: &str) -> Vec<(usize, String)> {
    let stripped = strip_comments(src);
    let src = stripped.as_str();
    let bytes = src.as_bytes();
    let mut out: Vec<(usize, String)> = Vec::new();
    for marker in MARKERS {
        for (start, _) in src.match_indices(marker.head) {
            // Word boundary: the head must begin a fresh identifier, so
            // `t(` does not fire inside `insert(` / the tail of
            // `t_strict(`.
            if start > 0 && is_word_byte(bytes[start - 1]) {
                continue;
            }
            let open = start + marker.head.len() - 1; // index of the '('
            let Some(close) = matching_paren(bytes, open) else {
                continue;
            };
            let inner = &src[open + 1..close];
            if let Some(key) = key_from_args(inner, marker.locale_first) {
                out.push((line_of(src, start), key));
            }
        }
    }
    out
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Blank `//` line comments and `/* */` block comments, replacing their
/// bytes with spaces (newlines kept) so byte offsets and line numbers are
/// unchanged. String and char literals are respected — a `//` inside a
/// string, or a `'"'` char, never starts a comment or a string.
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            out.push(c);
            if c == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        // A char literal (`'x'`, `'\n'`, `'\''`, `'"'`) is copied verbatim
        // so its inner quote never toggles string state; a lifetime (`'a`)
        // falls through as an ordinary `'`.
        if c == b'\'' {
            if let Some(len) = char_literal_len(bytes, i) {
                out.extend_from_slice(&bytes[i..i + len]);
                i += len;
                continue;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(b' ');
                i += 1;
            }
            continue;
        }
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            while i < bytes.len()
                && !(bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/')
            {
                out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                i += 1;
            }
            if i < bytes.len() {
                out.push(b' ');
                out.push(b' ');
                i += 2;
            }
            continue;
        }
        if c == b'"' {
            in_string = true;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// Byte length of a char literal starting at `i` (the opening `'`), or
/// `None` if `i` opens a lifetime instead. Recognizes `'x'`, an escape
/// `'\n'` / `'\''`, and a unicode escape `'\u{1F600}'`.
fn char_literal_len(bytes: &[u8], i: usize) -> Option<usize> {
    if bytes.get(i) != Some(&b'\'') {
        return None;
    }
    if bytes.get(i + 1) == Some(&b'\\') {
        // Escaped: scan to the closing quote within a bounded window.
        let mut j = i + 2;
        while j < bytes.len() && j < i + 12 {
            if bytes[j] == b'\'' {
                return Some(j - i + 1);
            }
            j += 1;
        }
        return None;
    }
    // Unescaped single byte/char followed immediately by a closing quote.
    if bytes.get(i + 2) == Some(&b'\'') {
        return Some(3);
    }
    None
}

/// 1-based line number of byte offset `at`.
fn line_of(src: &str, at: usize) -> usize {
    src[..at].bytes().filter(|&b| b == b'\n').count() + 1
}

/// Index of the `)` that closes the `(` at `open`, skipping over string
/// literals so a `)` inside a string is not mistaken for the close.
fn matching_paren(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            match c {
                b'\\' => i += 1, // skip the escaped byte
                b'"' => in_string = false,
                _ => {}
            }
        } else {
            match c {
                b'"' => in_string = true,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Pick the key literal out of a call's comma-separated `inner` arguments.
///
/// For the `t`-family (`locale_first`), the locale is arg 0 and must look
/// like a locale; the key is the first string-literal arg after it. For
/// the assertion macros, the key is the last string-literal arg.
fn key_from_args(inner: &str, locale_first: bool) -> Option<String> {
    let args = split_top_level(inner);
    if locale_first {
        let locale = args.first()?.trim();
        if !locale.contains("ocale") {
            return None;
        }
        args.iter().skip(1).find_map(|a| string_literal(a))
    } else {
        args.iter().rev().find_map(|a| string_literal(a))
    }
}

/// If `arg` (trimmed) is exactly a `"..."` string literal, return its
/// contents; otherwise `None`.
fn string_literal(arg: &str) -> Option<String> {
    let t = arg.trim();
    let inner = t.strip_prefix('"')?.strip_suffix('"')?;
    // A bare key never contains an escape or an embedded quote; reject
    // anything fancier so we never capture a partial literal.
    if inner.contains('"') || inner.contains('\\') {
        return None;
    }
    Some(inner.to_string())
}

/// Split `inner` on top-level commas — commas inside `()`/`[]`/`{}` or a
/// string literal do not split.
fn split_top_level(inner: &str) -> Vec<&str> {
    let bytes = inner.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            match c {
                b'\\' => i += 1,
                b'"' => in_string = false,
                _ => {}
            }
        } else {
            match c {
                b'"' => in_string = true,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    parts.push(&inner[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }
    if start <= inner.len() {
        parts.push(&inner[start..]);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(src: &str) -> Vec<String> {
        let mut k: Vec<String> = extract_refs(src).into_iter().map(|(_, k)| k).collect();
        k.sort();
        k
    }

    #[test]
    fn extracts_t_family_keys() {
        let src = r#"
            let a = i18n::t(locale, "nav.home");
            let b = i18n::t_args(locale, "cta.email", &[("email", addr)]);
            let c = t_strict(Locale::En, "products.heading");
            let d = i18n::t(self.locale, "auth.portal");
            let e = crate::i18n::t(crate::Locale::En, "cta.consultation");
        "#;
        assert_eq!(
            keys(src),
            [
                "auth.portal",
                "cta.consultation",
                "cta.email",
                "nav.home",
                "products.heading",
            ]
        );
    }

    #[test]
    fn extracts_assertion_macro_keys_with_or_without_locale() {
        let src = r#"
            assert_renders!(&body, "testimonials.home_heading");
            assert_renders!(&body, Locale::Es, "nav.services");
            assert_absent!(&body, "auth.sign_out");
        "#;
        assert_eq!(
            keys(src),
            ["auth.sign_out", "nav.services", "testimonials.home_heading"]
        );
    }

    #[test]
    fn ignores_unrelated_t_calls_and_substrings() {
        // `insert(` ends in "t(" but is not a word-boundary `t(`; a `t(`
        // whose first arg is not a locale is not an i18n call.
        let src = r#"
            map.insert("not.a.key", 1);
            other.t("also.not", x);
            let s = format!("{}", "nope");
        "#;
        assert!(keys(src).is_empty(), "got {:?}", keys(src));
    }

    #[test]
    fn handles_escaped_quotes_and_char_literals() {
        // A string with an escaped quote must not end early; an escaped
        // char literal (`'\''`) is skipped whole; a call whose key string
        // carries a backslash is rejected (not a clean key). The real call
        // after them still parses — proving none of that derailed scanning.
        let src = "let s = \"a \\\" b\"; let c = '\\''; i18n::t(locale, \"x\\\\y\"); \
             let k = i18n::t(locale, \"nav.home\");";
        assert_eq!(keys(src), ["nav.home"]);
    }

    #[test]
    fn char_literal_len_classifies_every_form() {
        assert_eq!(char_literal_len(b"'a'", 0), Some(3)); // plain
        assert_eq!(char_literal_len(b"'\\n'", 0), Some(4)); // escape
        assert_eq!(char_literal_len(b"'a", 0), None); // lifetime: no close
        assert_eq!(char_literal_len(b"x", 0), None); // not a quote (guard)
        assert_eq!(char_literal_len(b"'\\", 0), None); // unterminated escape
    }

    #[test]
    fn reports_line_numbers() {
        let src = "fn f() {\n    i18n::t(locale, \"nav.home\");\n}\n";
        let refs = extract_refs(src);
        assert_eq!(refs, vec![(2, "nav.home".to_string())]);
    }

    #[test]
    fn split_top_level_respects_nesting() {
        let parts = split_top_level(r#"locale, "cta.email", &[("email", addr)]"#);
        assert_eq!(
            parts,
            vec!["locale", " \"cta.email\"", " &[(\"email\", addr)]"]
        );
    }

    #[test]
    fn doc_comment_examples_are_not_references() {
        // The exact shape that tripped the first run: a key shown in a
        // `///` doc-comment example must not count as a live call site.
        let src = "\
            /// `assert_renders!(body, \"key\")` (locale defaults to English)\n\
            // i18n::t(locale, \"also.in.a.comment\")\n\
            let real = i18n::t(locale, \"nav.home\");\n";
        assert_eq!(keys(src), ["nav.home"]);
    }

    #[test]
    fn url_in_string_is_not_a_comment() {
        // The `//` in a URL string literal must not start a comment and
        // swallow a following key on the same logical span.
        let src = "let u = \"https://x\"; let k = i18n::t(locale, \"nav.home\");";
        assert_eq!(keys(src), ["nav.home"]);
    }

    #[test]
    fn char_literal_quote_does_not_break_scanning() {
        let src = "let q = '\"'; let k = i18n::t(locale, \"nav.home\");";
        assert_eq!(keys(src), ["nav.home"]);
    }

    #[test]
    fn block_comment_example_is_not_a_reference() {
        let src = "/* example: i18n::t(locale, \"in.block\") */ i18n::t(locale, \"nav.home\");";
        assert_eq!(keys(src), ["nav.home"]);
    }

    #[test]
    fn malformed_and_degenerate_calls_are_skipped() {
        // Unbalanced — `matching_paren` finds no close, so the marker is
        // skipped (no key, no panic).
        assert!(keys("i18n::t(locale, \"x\"").is_empty());
        // Empty args, and a first arg that isn't a locale → no key.
        assert!(keys("i18n::t()").is_empty());
        assert!(keys("t(xyz, \"x\")").is_empty());
        // An assertion macro with no string literal → no key.
        assert!(keys("assert_renders!(body)").is_empty());
        // An unterminated block comment is blanked to end-of-input, so the
        // call it swallows is never seen.
        assert!(keys("let x = 1; /* unterminated i18n::t(locale, \"y\")").is_empty());
    }

    #[test]
    fn reconciles_missing_and_unused() {
        use std::collections::BTreeSet;
        let catalog: BTreeSet<String> = ["nav.home", "auth.portal", "dead.key"]
            .into_iter()
            .map(String::from)
            .collect();
        let src = "i18n::t(locale, \"nav.home\"); i18n::t(locale, \"auth.portal\"); \
                   i18n::t(locale, \"typo.key\");";
        let refs: Vec<_> = extract_refs(src).into_iter().map(|(_, k)| k).collect();
        let missing: Vec<&String> = refs.iter().filter(|k| !catalog.contains(*k)).collect();
        assert_eq!(missing, vec![&"typo.key".to_string()]);
        let unused: Vec<&String> = catalog
            .iter()
            .filter(|k| !src.contains(&format!("\"{k}\"")))
            .collect();
        assert_eq!(unused, vec![&"dead.key".to_string()]);
    }

    /// Write `body` to `<root>/<rel>` (creating parents).
    fn write_source(root: &std::path::Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn audit_workspace_walks_the_source_roots_and_reconciles() {
        let dir = tempfile::tempdir().unwrap();
        write_source(
            dir.path(),
            "views/src/page.rs",
            "fn r() { let _ = i18n::t(locale, \"known.key\"); i18n::t(locale, \"typo.key\"); }",
        );
        // A non-`.rs` file is skipped (is_rust_path == false), and a missing
        // source root (web/tests here) is simply not walked.
        write_source(
            dir.path(),
            "views/src/notes.txt",
            "i18n::t(locale, \"ignored\")",
        );
        let catalog: BTreeSet<String> = ["known.key", "dead.key"]
            .into_iter()
            .map(String::from)
            .collect();

        let audit = audit_workspace(dir.path(), &catalog).unwrap();

        assert_eq!(
            audit
                .missing
                .iter()
                .map(|r| r.key.as_str())
                .collect::<Vec<_>>(),
            ["typo.key"]
        );
        assert!(audit.missing[0].path.ends_with("page.rs"));
        assert_eq!(audit.unused, ["dead.key"]);
        assert_eq!(audit.files_scanned, 1, "the .txt file is not scanned");
        assert_eq!(audit.keys_in_catalog, 2);
    }

    #[test]
    fn missing_keys_are_sorted_by_path_then_line_then_key() {
        // Forces the `sort_by` comparator through all three tiers: two keys
        // on one line (key tiebreak), a third on a later line (line
        // tiebreak), and one in a second file (path is the primary sort).
        let dir = tempfile::tempdir().unwrap();
        write_source(
            dir.path(),
            "web/src/a.rs",
            "fn f() {\n  i18n::t(locale, \"z.one\"); i18n::t(locale, \"a.one\");\n  \
             i18n::t(locale, \"m.two\");\n}",
        );
        write_source(
            dir.path(),
            "web/tests/b.rs",
            "fn f() { i18n::t(locale, \"n.b\"); }",
        );
        // An empty catalog makes every referenced key missing.
        let audit = audit_workspace(dir.path(), &BTreeSet::new()).unwrap();

        let order: Vec<(&str, usize)> = audit
            .missing
            .iter()
            .map(|r| (r.key.as_str(), r.line))
            .collect();
        // web/src sorts before web/tests; within a file, by line then key.
        assert_eq!(
            order,
            [("a.one", 2), ("z.one", 2), ("m.two", 3), ("n.b", 1)]
        );
    }

    #[test]
    fn a_key_named_only_in_a_comment_is_still_unused() {
        // The unused check must see the SAME comment-free source the
        // extractor does — a key mentioned only in a comment renders
        // nowhere, so it is genuinely unused.
        let dir = tempfile::tempdir().unwrap();
        write_source(
            dir.path(),
            "views/src/c.rs",
            "fn f() { /* see \"dead.key\" */ }",
        );
        let catalog: BTreeSet<String> = ["dead.key"].into_iter().map(String::from).collect();

        let audit = audit_workspace(dir.path(), &catalog).unwrap();

        assert_eq!(audit.unused, ["dead.key"]);
    }

    #[test]
    fn audit_and_report_is_clean_when_every_catalog_key_is_referenced() {
        // A temp source that names every real En catalog key leaves nothing
        // missing and nothing unused.
        let dir = tempfile::tempdir().unwrap();
        let body = views::i18n::en_catalog_keys()
            .iter()
            .map(|k| format!("let _ = i18n::t(locale, \"{k}\");"))
            .collect::<Vec<_>>()
            .join("\n");
        write_source(
            dir.path(),
            "views/src/all.rs",
            &format!("fn r() {{ {body} }}"),
        );

        assert!(audit_and_report(dir.path()).unwrap());
        // The `run` shim maps that to a success exit.
        let _ = run(dir.path());
    }

    #[test]
    fn audit_and_report_flags_drift_in_both_directions() {
        // Names a non-catalog key (missing) and none of the real keys (all
        // unused), so both report paths run.
        let dir = tempfile::tempdir().unwrap();
        write_source(
            dir.path(),
            "web/src/x.rs",
            "fn r() { let _ = i18n::t(locale, \"nope.not.real\"); }",
        );

        assert!(!audit_and_report(dir.path()).unwrap());
        let _ = run(dir.path());
    }

    #[test]
    fn audit_and_report_surfaces_an_unreadable_source() {
        // A non-UTF-8 `.rs` file makes `read_to_string` fail; the error
        // propagates and `run` exits 2.
        let dir = tempfile::tempdir().unwrap();
        write_source(dir.path(), "views/src/ok.rs", "fn r() {}");
        std::fs::write(dir.path().join("views/src/bad.rs"), [0xff, 0xfe, 0x00]).unwrap();

        assert!(audit_and_report(dir.path()).is_err());
        let _ = run(dir.path());
    }
}
