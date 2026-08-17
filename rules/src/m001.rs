//! `M001` — heading levels must only increment by one. Mirrors
//! markdownlint MD001 (heading-increment).

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct M001HeadingIncrement;

impl M001HeadingIncrement {
    pub const CODE: &'static str = "M001";
}

impl Rule for M001HeadingIncrement {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut prev: Option<usize> = None;
        let mut violations = Vec::new();
        for (line_no, line) in frontmatter::body_lines(&file.contents) {
            let trimmed = line.trim_start();
            let level = trimmed.bytes().take_while(|&b| b == b'#').count();
            if level == 0 || level > 6 {
                continue;
            }
            if trimmed.as_bytes().get(level) != Some(&b' ') {
                continue; // not a real ATX heading (M018 catches this)
            }
            if let Some(p) = prev {
                if level > p + 1 {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: line_no,
                        range: line_byte_range(&file.contents, line_no),
                        message: format!(
                            "Heading level jumped from H{p} to H{level} — increment by one",
                        ),
                    });
                }
            }
            prev = Some(level);
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M001HeadingIncrement;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn f(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_for_sequential_heading_levels() {
        let body = "# h1\n## h2\n### h3\n## h2 again\n# back to h1\n";
        assert!(M001HeadingIncrement.lint(&f(body)).is_empty());
    }

    #[test]
    fn flags_skipping_from_h1_to_h3() {
        let v = M001HeadingIncrement.lint(&f("# h1\n### h3\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 2);
        assert!(v[0].message.contains("H1 to H3"));
    }

    #[test]
    fn allows_first_heading_at_any_level() {
        assert!(M001HeadingIncrement
            .lint(&f("### deep first\n#### deeper\n"))
            .is_empty());
    }
}
