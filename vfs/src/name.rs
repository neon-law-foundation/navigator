//! Deriving a filename that is legal on every device the firm supports.
//!
//! An asset's `filename` arrives from wherever the bytes did — a portal
//! upload, an email attachment, a court download — so it may contain path
//! separators, control characters, or a name Windows cannot open at all.
//! The mounted folder has to render on macOS and Windows at once, so the
//! rules here are the **intersection** of both platforms' constraints
//! rather than the host's:
//!
//! - Windows rejects `\ / : * ? " < > |`, and silently strips trailing dots
//!   and spaces — a file saved as `Motion.` becomes one nothing can reopen.
//! - Windows still reserves the DOS device names (`CON`, `NUL`, `COM1`, …),
//!   matched on the stem and case-insensitively, so `nul.pdf` is refused.
//! - Both platforms cap a single component at 255 bytes.
//!
//! Sanitizing is the floor, not the firm's naming convention: it makes a
//! name openable, and says nothing about whether it is a *good* name. The
//! canonical scheme (date prefix, document kind, filed-status marker) is a
//! separate layer that composes on top of this one.

/// The name given to an asset whose own filename survives sanitizing as
/// nothing at all — an empty string, `..`, or pure punctuation.
pub const FALLBACK: &str = "unnamed";

/// Maximum bytes in one path component on APFS, HFS+, and NTFS alike.
pub const MAX_NAME_BYTES: usize = 255;

/// Longest extension worth preserving when a name has to be truncated.
/// Past this, the trailing segment is more likely part of the name than a
/// real suffix, so the whole string is cut instead.
const MAX_KEPT_EXTENSION_BYTES: usize = 16;

/// The DOS device names Windows still refuses, matched case-insensitively
/// against the stem. `COM0` and `LPT0` are included: they are accepted by
/// some Windows versions and refused by others, and a name that works on
/// only some devices is worse than one that is escaped everywhere.
const RESERVED_STEMS: [&str; 24] = [
    "con", "prn", "aux", "nul", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
    "com8", "com9", "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Characters no path component may contain on Windows. `/` is added for
/// Unix, where it is the separator; both platforms also reject NUL, which
/// [`char::is_control`] covers along with the rest of C0.
const FORBIDDEN: [char; 9] = ['<', '>', ':', '"', '|', '?', '*', '\\', '/'];

/// Rewrite `raw` into a single path component that opens on macOS and
/// Windows. Always returns a non-empty name of at most [`MAX_NAME_BYTES`]
/// bytes, and is a pure function of its input, so the same asset renders
/// under the same name on every device.
pub fn sanitize(raw: &str) -> String {
    let replaced: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || FORBIDDEN.contains(&c) {
                '_'
            } else {
                c
            }
        })
        .collect();

    // A trailing dot or space is legal to *write* on Windows and then
    // impossible to open, so it is trimmed rather than escaped.
    let trimmed = replaced.trim().trim_end_matches(['.', ' ']).trim_end();

    if trimmed.is_empty() {
        return FALLBACK.to_string();
    }

    fit(escape_reserved_stem(trimmed))
}

/// Prefix a DOS device name with `_` so it becomes an ordinary file.
fn escape_reserved_stem(name: &str) -> String {
    let stem = name.split('.').next().unwrap_or(name);

    if RESERVED_STEMS
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        format!("_{name}")
    } else {
        name.to_string()
    }
}

/// Cut `name` to [`MAX_NAME_BYTES`], keeping a short extension so the file
/// still opens in the application that owns it.
fn fit(name: String) -> String {
    if name.len() <= MAX_NAME_BYTES {
        return name;
    }

    let extension = name
        .rfind('.')
        .filter(|dot| *dot > 0)
        .map(|dot| &name[dot..])
        .filter(|ext| ext.len() <= MAX_KEPT_EXTENSION_BYTES);

    let Some(extension) = extension else {
        return truncate_on_boundary(&name, MAX_NAME_BYTES).to_string();
    };

    let stem = truncate_on_boundary(&name[..name.len() - extension.len()], {
        MAX_NAME_BYTES - extension.len()
    })
    // Truncation can expose a trailing dot or space that Windows would
    // strip, so the same trim the whole name got applies to the cut stem.
    .trim_end_matches(['.', ' '])
    .trim_end();

    if stem.is_empty() {
        return truncate_on_boundary(&name, MAX_NAME_BYTES).to_string();
    }

    format!("{stem}{extension}")
}

/// Cut `value` to at most `max` bytes without splitting a UTF-8 character.
pub(crate) fn truncate_on_boundary(value: &str, max: usize) -> &str {
    if value.len() <= max {
        return value;
    }

    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }

    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_are_untouched() {
        assert_eq!(sanitize("Motion to Dismiss.pdf"), "Motion to Dismiss.pdf");
    }

    #[test]
    fn path_separators_cannot_escape_the_directory() {
        assert_eq!(sanitize("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(sanitize(r"..\..\Windows"), ".._.._Windows");
    }

    #[test]
    fn windows_forbidden_characters_are_replaced() {
        assert_eq!(
            sanitize(r#"Smith v. Jones <2:26-cv-1> "draft"?.pdf"#),
            "Smith v. Jones _2_26-cv-1_ _draft__.pdf"
        );
    }

    #[test]
    fn control_characters_are_replaced() {
        assert_eq!(sanitize("Order\u{0}\u{1f}\n.pdf"), "Order___.pdf");
    }

    #[test]
    fn trailing_dots_and_spaces_are_trimmed() {
        // Windows silently drops both, producing a file it cannot reopen.
        assert_eq!(sanitize("Retainer.pdf. "), "Retainer.pdf");
        assert_eq!(sanitize("Exhibit A   "), "Exhibit A");
    }

    #[test]
    fn names_that_sanitize_to_nothing_get_the_fallback() {
        assert_eq!(sanitize(""), FALLBACK);
        assert_eq!(sanitize("   "), FALLBACK);
        assert_eq!(sanitize("."), FALLBACK);
        assert_eq!(sanitize(".."), FALLBACK);
    }

    #[test]
    fn leading_dot_files_are_preserved() {
        // A dotfile is legitimate on the agent-facing side of the tree.
        assert_eq!(sanitize(".gitignore"), ".gitignore");
    }

    #[test]
    fn dos_device_names_are_escaped() {
        assert_eq!(sanitize("NUL"), "_NUL");
        assert_eq!(sanitize("nul.pdf"), "_nul.pdf");
        assert_eq!(sanitize("CoM1.txt"), "_CoM1.txt");
        assert_eq!(sanitize("LPT9"), "_LPT9");
    }

    #[test]
    fn names_merely_starting_with_a_device_name_are_left_alone() {
        assert_eq!(sanitize("connection notes.pdf"), "connection notes.pdf");
        assert_eq!(sanitize("nullity.pdf"), "nullity.pdf");
    }

    #[test]
    fn long_names_are_truncated_but_keep_their_extension() {
        let long = format!("{}.pdf", "a".repeat(300));
        let out = sanitize(&long);

        assert_eq!(out.len(), MAX_NAME_BYTES);
        assert!(out.ends_with(".pdf"), "extension survives truncation");
    }

    #[test]
    fn truncation_never_splits_a_character() {
        // Three bytes each, so the 255-byte cut lands mid-character.
        let long = format!("{}.pdf", "é".repeat(200));
        let out = sanitize(&long);

        assert!(out.len() <= MAX_NAME_BYTES);
        assert!(out.ends_with(".pdf"));
        // Round-tripping proves no partial code point survived.
        assert_eq!(out, String::from_utf8(out.clone().into_bytes()).unwrap());
    }

    #[test]
    fn a_long_trailing_segment_is_not_treated_as_an_extension() {
        let long = format!("{}.{}", "a".repeat(300), "b".repeat(40));
        let out = sanitize(&long);

        assert_eq!(out.len(), MAX_NAME_BYTES);
        assert!(!out.contains('.'), "the cut lands inside the stem");
    }

    #[test]
    fn sanitizing_is_idempotent() {
        // The mount re-derives names constantly; a name that changed on a
        // second pass would rename the user's file behind their back.
        for raw in [
            "Motion to Dismiss.pdf",
            "../../etc/passwd",
            "nul.pdf",
            "Retainer.pdf. ",
            "",
            &"a".repeat(300),
        ] {
            let once = sanitize(raw);
            assert_eq!(sanitize(&once), once, "not idempotent for {raw:?}");
        }
    }

    #[test]
    fn output_is_always_a_usable_component() {
        for raw in ["", "..", "\u{0}", "NUL", &"z".repeat(400), "  . . "] {
            let out = sanitize(raw);

            assert!(!out.is_empty());
            assert!(out.len() <= MAX_NAME_BYTES);
            assert!(!out.contains('/') && !out.contains('\\'));
            assert!(!out.ends_with('.') && !out.ends_with(' '));
        }
    }
}
