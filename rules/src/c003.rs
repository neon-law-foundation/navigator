//! `C003` — a blog post's filename must be `YYYYMMDD_slug.md`.
//!
//! `web::blog` derives the publish date from the `YYYYMMDD` filename
//! prefix and the URL slug from everything after the first underscore. A
//! file whose prefix is not a valid date is **silently skipped** by the
//! loader — it never appears on the site and never errors. That silent
//! drop is the failure this rule makes loud: a misnamed post is caught at
//! authoring time instead of vanishing in production.

use chrono::NaiveDate;

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct C003BlogFilename;

impl C003BlogFilename {
    pub const CODE: &'static str = "C003";
}

/// True when `stem` is `YYYYMMDD_slug`: a real calendar date prefix, an
/// underscore, and a non-empty slug.
///
/// The date is validated with the *same* strict parse the loader uses —
/// `web::blog::parse_post_filename` does `NaiveDate::parse_from_str(.., "%Y%m%d")`
/// — so an impossible date (Feb 31, Apr 31, Feb 29 off a leap year) is
/// rejected here too, rather than passing lint and then being silently
/// dropped by the loader, which is the exact failure this rule prevents.
fn is_dated_post_stem(stem: &str) -> bool {
    let Some((date_part, slug)) = stem.split_once('_') else {
        return false;
    };
    !slug.is_empty() && NaiveDate::parse_from_str(date_part, "%Y%m%d").is_ok()
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
    fn rejects_an_impossible_calendar_date() {
        // These pass naive month/day range checks but are not real dates,
        // and `web::blog`'s strict parse would silently drop them — so the
        // rule must reject them. (Greptile P1 on #206.)
        assert!(!is_dated_post_stem("20260231_post"), "Feb 31");
        assert!(!is_dated_post_stem("20260431_post"), "Apr 31");
        assert!(
            !is_dated_post_stem("20270229_post"),
            "Feb 29 in a non-leap year"
        );
        // A real leap day still passes.
        assert!(is_dated_post_stem("20240229_post"), "Feb 29 in a leap year");
    }

    #[test]
    fn rejects_an_empty_slug() {
        assert!(!is_dated_post_stem("20260625_"));
    }
}
