//! `C004` — a board-minutes filename must be `YYYY-qN.md`.
//!
//! `web::transparency` loads one markdown file per quarter under
//! `minutes/`, named `YYYY-qN.md`, and derives the public route
//! (`minutes-YYYY-qN`) and the newest-first sort order from that stem. A
//! file that does not match the pattern would not sort or route as a
//! quarterly record, so the rule pins the convention at authoring time.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct C004MinutesFilename;

impl C004MinutesFilename {
    pub const CODE: &'static str = "C004";
}

/// True when `stem` is `YYYY-qN`: a 4-digit year, a hyphen, a literal
/// lowercase `q`, and a quarter digit 1–4. Mirrors the stem
/// `web::transparency` parses into a `year * 10 + quarter` sort key.
fn is_minutes_stem(stem: &str) -> bool {
    let Some((year, quarter)) = stem.split_once('-') else {
        return false;
    };
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    matches!(quarter, "q1" | "q2" | "q3" | "q4")
}

impl Rule for C004MinutesFilename {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        "Board-minutes filenames must be `YYYY-qN.md`."
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let stem = file
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if is_minutes_stem(stem) {
            return Vec::new();
        }
        vec![Violation {
            code: Self::CODE,
            path: file.path.clone(),
            line: 1,
            range: line_byte_range(&file.contents, 1),
            message: format!("Board-minutes filename `{stem}` is not `YYYY-qN` (e.g. `2026-q1`)"),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::{is_minutes_stem, C004MinutesFilename};
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(name: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from(format!("web/content/foundation/minutes/{name}")),
            contents: "---\ntitle: T\ndescription: D\n---\n".to_string(),
        }
    }

    #[test]
    fn accepts_a_quarter_stem() {
        assert!(is_minutes_stem("2021-q1"));
        assert!(C004MinutesFilename.lint(&file("2021-q1.md")).is_empty());
    }

    #[test]
    fn rejects_a_bad_quarter() {
        assert!(!is_minutes_stem("2021-q5"));
        let v = C004MinutesFilename.lint(&file("2021-q5.md"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "C004");
    }

    #[test]
    fn rejects_a_non_year() {
        assert!(!is_minutes_stem("21-q1"));
        assert_eq!(C004MinutesFilename.lint(&file("21-q1.md")).len(), 1);
    }

    #[test]
    fn rejects_a_missing_hyphen() {
        assert!(!is_minutes_stem("2021q1"));
    }
}
