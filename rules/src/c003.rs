//! `C003` — a blog post's filename must be `YYYYMMDD_slug.md`.
//!
//! `web::blog` derives the publish date from the `YYYYMMDD` filename
//! prefix and the URL slug from everything after the first underscore. A
//! file whose prefix is not a valid date is **silently skipped** by the
//! loader — it never appears on the site and never errors. That silent
//! drop is the failure this rule makes loud: a misnamed post is caught at
//! authoring time instead of vanishing in production.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct C003BlogFilename;

impl C003BlogFilename {
    pub const CODE: &'static str = "C003";
}

/// True when `stem` is `YYYYMMDD_slug`: an 8-digit date prefix (with a
/// plausible month and day), an underscore, and a non-empty slug. This
/// mirrors `web::blog::parse_post_filename`, which splits on the first
/// underscore and parses the prefix as `%Y%m%d`.
fn is_dated_post_stem(stem: &str) -> bool {
    let Some((date_part, slug)) = stem.split_once('_') else {
        return false;
    };
    if slug.is_empty() {
        return false;
    }
    if date_part.len() != 8 || !date_part.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let month: u8 = date_part[4..6].parse().unwrap_or(0);
    let day: u8 = date_part[6..8].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

impl Rule for C003BlogFilename {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        "Blog post filenames must be `YYYYMMDD_slug.md`."
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let stem = file
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if is_dated_post_stem(stem) {
            return Vec::new();
        }
        vec![Violation {
            code: Self::CODE,
            path: file.path.clone(),
            line: 1,
            range: line_byte_range(&file.contents, 1),
            message: format!(
                "Blog post filename `{stem}` is not `YYYYMMDD_slug` — the loader \
                 silently skips posts whose date prefix does not parse"
            ),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::{is_dated_post_stem, C003BlogFilename};
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(name: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from(format!("web/content/blog/{name}")),
            contents: "---\ntitle: T\ndescription: D\n---\n".to_string(),
        }
    }

    #[test]
    fn accepts_a_dated_stem() {
        assert!(is_dated_post_stem("20260625_going_all_in_on_rust"));
        assert!(C003BlogFilename
            .lint(&file("20260625_going_all_in_on_rust.md"))
            .is_empty());
    }

    #[test]
    fn rejects_a_missing_underscore() {
        assert!(!is_dated_post_stem("20260625"));
        assert_eq!(C003BlogFilename.lint(&file("20260625.md")).len(), 1);
    }

    #[test]
    fn rejects_a_non_date_prefix() {
        assert!(!is_dated_post_stem("draft_my_post"));
        let v = C003BlogFilename.lint(&file("draft_my_post.md"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "C003");
    }

    #[test]
    fn rejects_an_impossible_month() {
        assert!(!is_dated_post_stem("20261325_post"));
    }

    #[test]
    fn rejects_an_empty_slug() {
        assert!(!is_dated_post_stem("20260625_"));
    }
}
