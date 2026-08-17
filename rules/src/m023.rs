//! `M023` — ATX headings must start at the beginning of the line.
//! Mirrors markdownlint MD023 (heading-start-left).

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M023HeadingStartLeft;

impl M023HeadingStartLeft {
    pub const CODE: &'static str = "M023";
}

impl Rule for M023HeadingStartLeft {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        file.contents
            .lines()
            .enumerate()
            .filter_map(|(idx, line)| {
                let trimmed = line.trim_start();
                if line == trimmed {
                    return None;
                }
                if trimmed.starts_with('#') {
                    Some(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: "Heading must start at the beginning of the line".to_string(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::M023HeadingStartLeft;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_when_heading_starts_at_column_zero() {
        assert!(M023HeadingStartLeft
            .lint(&file("# Title\n## Sub\n"))
            .is_empty());
    }

    #[test]
    fn flags_heading_with_leading_whitespace() {
        let v = M023HeadingStartLeft.lint(&file("  # Indented heading\nok\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 1);
        assert_eq!(v[0].code, "M023");
    }

    #[test]
    fn does_not_flag_indented_non_heading_lines() {
        assert!(M023HeadingStartLeft
            .lint(&file("    indented code\n    more code\n"))
            .is_empty());
    }
}
