//! `M060` — tables must use one consistent column style per table:
//! `aligned` (pipes line up), `tight` (single-space cell padding), or
//! `compact` (no padding). Mirrors MD060's `any` mode.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M060TableColumnStyle;

impl M060TableColumnStyle {
    pub const CODE: &'static str = "M060";
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

fn pipe_positions(line: &str) -> Vec<usize> {
    // Use character indices, not byte indices: a row with an em-dash
    // (3 bytes in UTF-8) is still visually aligned with neighbors that
    // have only ASCII content. Comparing byte offsets here would flag
    // every mixed-ASCII/Unicode table as misaligned.
    line.chars()
        .enumerate()
        .filter_map(|(i, c)| (c == '|').then_some(i))
        .collect()
}

fn cells(line: &str) -> Vec<&str> {
    let t = line.trim_end();
    let inner = t.trim_start_matches('|').trim_end_matches('|');
    inner.split('|').collect()
}

fn aligned(rows: &[&str]) -> bool {
    let target = pipe_positions(rows[0]);
    rows.iter().all(|r| pipe_positions(r) == target)
}

fn tight(rows: &[&str]) -> bool {
    rows.iter().all(|r| {
        cells(r).iter().all(|c| {
            if c.trim().is_empty() {
                return true;
            }
            c.starts_with(' ') && c.ends_with(' ') && !c.starts_with("  ") && !c.ends_with("  ")
        })
    })
}

fn compact(rows: &[&str]) -> bool {
    rows.iter().all(|r| {
        cells(r).iter().all(|c| {
            if c.is_empty() {
                return true;
            }
            !c.starts_with(' ') && !c.ends_with(' ')
        })
    })
}

impl Rule for M060TableColumnStyle {
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
                let mut rows: Vec<&str> = vec![lines[i], lines[i + 1]];
                let mut j = i + 2;
                while j < lines.len() && is_table_row(lines[j]) {
                    rows.push(lines[j]);
                    j += 1;
                }
                if !aligned(&rows) && !tight(&rows) && !compact(&rows) {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: i + 1,
                        range: line_byte_range(&file.contents, i + 1),
                        message:
                            "Table does not match any consistent column style (aligned/tight/compact)"
                                .to_string(),
                    });
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
    use super::M060TableColumnStyle;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_tight_table() {
        let s = "| a | b |\n|---|---|\n| 1 | 2 |\n";
        assert!(M060TableColumnStyle.lint(&f(s)).is_empty());
    }
    #[test]
    fn passes_with_compact_table() {
        let s = "|a|b|\n|-|-|\n|1|2|\n";
        assert!(M060TableColumnStyle.lint(&f(s)).is_empty());
    }
    #[test]
    fn flags_mixed_padding() {
        let s = "| a |b|\n|---|---|\n|1 | 2|\n";
        let v = M060TableColumnStyle.lint(&f(s));
        assert!(!v.is_empty());
    }

    #[test]
    fn aligned_table_with_unicode_em_dash_does_not_misalign() {
        // The em-dash (3 bytes in UTF-8) used to shift pipe byte
        // offsets and trip `aligned`. With char-index counting the
        // table reads as aligned.
        let s = "\
| a    | b                |
| ---- | ---------------- |
| ok   | plain ascii      |
| also | with — em-dash   |
";
        assert!(M060TableColumnStyle.lint(&f(s)).is_empty());
    }
}
