//! `M022` — headings must be surrounded by blank lines.
//! Mirrors markdownlint MD022.

use std::collections::HashSet;

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct M022BlanksAroundHeadings;

impl M022BlanksAroundHeadings {
    pub const CODE: &'static str = "M022";
}

fn is_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return false;
    }
    trimmed.as_bytes().get(hashes).is_some_and(|&b| b == b' ')
}

impl Rule for M022BlanksAroundHeadings {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let lines: Vec<&str> = file.contents.lines().collect();
        // Only consider lines that body_lines yields — that is,
        // outside frontmatter and outside fenced code blocks.
        let body: HashSet<usize> = frontmatter::body_lines(&file.contents)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        let mut violations = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            let line_no = idx + 1;
            if !body.contains(&line_no) {
                continue;
            }
            if !is_heading(line) {
                continue;
            }
            if idx > 0 && !lines[idx - 1].trim().is_empty() {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: line_no,
                    range: line_byte_range(&file.contents, line_no),
                    message: "Heading must be preceded by a blank line".to_string(),
                });
            }
            if let Some(next) = lines.get(idx + 1) {
                if !next.trim().is_empty() {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: line_no,
                        range: line_byte_range(&file.contents, line_no),
                        message: "Heading must be followed by a blank line".to_string(),
                    });
                }
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M022BlanksAroundHeadings;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_blanks_around_headings() {
        assert!(M022BlanksAroundHeadings
            .lint(&f("# H1\n\nbody\n\n## H2\n\nbody2\n"))
            .is_empty());
    }
    #[test]
    fn flags_heading_without_preceding_blank() {
        let v = M022BlanksAroundHeadings.lint(&f("text\n# H1\n\nok\n"));
        assert!(v.iter().any(|x| x.message.contains("preceded")));
    }
    #[test]
    fn flags_heading_without_following_blank() {
        let v = M022BlanksAroundHeadings.lint(&f("# H1\ntext\n"));
        assert!(v.iter().any(|x| x.message.contains("followed")));
    }
}
