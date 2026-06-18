//! `M055` — table rows must use a consistent leading/trailing pipe
//! style across the file. Mirrors MD055.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M055TablePipeStyle;

impl M055TablePipeStyle {
    pub const CODE: &'static str = "M055";
}

fn is_table_row(line: &str) -> bool {
    line.trim().contains('|')
}

fn is_separator(line: &str) -> bool {
    let t = line.trim();
    if !t.contains('|') {
        return false;
    }
    t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) && t.contains('-')
}

fn pipes(line: &str) -> (bool, bool) {
    let t = line.trim();
    (t.starts_with('|'), t.ends_with('|'))
}

impl Rule for M055TablePipeStyle {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let lines: Vec<&str> = file.contents.lines().collect();
        // Find the first table header: a row immediately followed by a separator row.
        let mut expected: Option<(bool, bool)> = None;
        let mut in_table = false;
        let mut violations = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            let next = lines.get(idx + 1).copied().unwrap_or("");
            if !in_table {
                if is_table_row(line) && is_separator(next) {
                    let p = pipes(line);
                    if expected.is_none() {
                        expected = Some(p);
                    } else if expected != Some(p) {
                        violations.push(Violation {
                            code: Self::CODE,
                            path: file.path.clone(),
                            line: idx + 1,
                            range: line_byte_range(&file.contents, idx + 1),
                            message: "Table pipe style does not match the document's first table"
                                .to_string(),
                        });
                    }
                    in_table = true;
                }
                continue;
            }
            if !is_table_row(line) {
                in_table = false;
                continue;
            }
            if Some(pipes(line)) != expected {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: idx + 1,
                    range: line_byte_range(&file.contents, idx + 1),
                    message: "Table pipe style does not match the document's first table"
                        .to_string(),
                });
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M055TablePipeStyle;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_consistent_pipe_style() {
        let s = "| h |\n|---|\n| a |\n";
        assert!(M055TablePipeStyle.lint(&f(s)).is_empty());
    }
    #[test]
    fn flags_inconsistent_pipe_style_between_tables() {
        let s = "| h |\n|---|\n| a |\n\nh |\n--|\nb |\n";
        let v = M055TablePipeStyle.lint(&f(s));
        assert!(!v.is_empty());
    }
}
