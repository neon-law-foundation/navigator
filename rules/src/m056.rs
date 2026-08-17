//! `M056` — every body row of a table must have the same number of
//! cells as the header. Mirrors MD056.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M056TableColumnCount;

impl M056TableColumnCount {
    pub const CODE: &'static str = "M056";
}

fn cell_count(line: &str) -> usize {
    let t = line.trim().trim_start_matches('|').trim_end_matches('|');
    if t.is_empty() {
        return 0;
    }
    // Count `|` that are not escaped.
    let mut count = 1;
    let mut prev = b'\0';
    for &b in t.as_bytes() {
        if b == b'|' && prev != b'\\' {
            count += 1;
        }
        prev = b;
    }
    count
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

impl Rule for M056TableColumnCount {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let lines: Vec<&str> = file.contents.lines().collect();
        let mut violations = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let next = lines.get(i + 1).copied().unwrap_or("");
            if is_table_row(lines[i]) && is_separator(next) {
                let expected = cell_count(lines[i]);
                // Skip header + separator.
                let mut j = i + 2;
                while j < lines.len() && is_table_row(lines[j]) {
                    let got = cell_count(lines[j]);
                    if got != expected {
                        violations.push(Violation {
                            code: Self::CODE,
                            path: file.path.clone(),
                            line: j + 1,
                            range: line_byte_range(&file.contents, j + 1),
                            message: format!("Table row has {got} cell(s); expected {expected}"),
                        });
                    }
                    j += 1;
                }
                i = j;
            } else {
                i += 1;
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M056TableColumnCount;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_consistent_columns() {
        let s = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";
        assert!(M056TableColumnCount.lint(&f(s)).is_empty());
    }
    #[test]
    fn flags_short_body_row() {
        let s = "| a | b |\n|---|---|\n| 1 |\n";
        let v = M056TableColumnCount.lint(&f(s));
        assert_eq!(v.len(), 1);
    }
}
