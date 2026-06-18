//! `M034` — no bare URLs in prose. Mirrors MD034.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct M034NoBareUrls;

impl M034NoBareUrls {
    pub const CODE: &'static str = "M034";
}

fn line_has_bare_url(line: &str) -> bool {
    for proto in ["http://", "https://"] {
        let Some(idx) = line.find(proto) else {
            continue;
        };
        // Skip if URL is already wrapped: angle-bracket form, or
        // preceded by `(` (markdown link destination).
        let before = idx.saturating_sub(1);
        let prev = line.as_bytes().get(before).copied().unwrap_or(0);
        if prev == b'<' || prev == b'(' {
            continue;
        }
        return true;
    }
    false
}

impl Rule for M034NoBareUrls {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut violations = Vec::new();
        for (line_no, line) in frontmatter::body_lines(&file.contents) {
            // Mask backtick-delimited code spans so URLs documented
            // as code (e.g. `http://keycloak:8080`) don't trip the
            // "bare URL" check — the backticks already mark them.
            let masked = frontmatter::mask_code_spans(line);
            if line_has_bare_url(&masked) {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: line_no,
                    range: line_byte_range(&file.contents, line_no),
                    message: "Bare URL — wrap in `<...>` or use `[text](url)` form".to_string(),
                });
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M034NoBareUrls;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_wrapped_urls() {
        assert!(M034NoBareUrls
            .lint(&f("See [home](https://x).\nOr <https://x>.\n"))
            .is_empty());
    }
    #[test]
    fn flags_bare_url() {
        let v = M034NoBareUrls.lint(&f("Visit https://example.com today.\n"));
        assert_eq!(v.len(), 1);
    }
    #[test]
    fn ignores_url_inside_fenced_code_block() {
        assert!(M034NoBareUrls
            .lint(&f("```text\nhttps://example.com\n```\n"))
            .is_empty());
    }

    #[test]
    fn ignores_url_inside_inline_code_span() {
        assert!(M034NoBareUrls
            .lint(&f("The pod sees `http://keycloak:8080` internally.\n"))
            .is_empty());
    }
}
