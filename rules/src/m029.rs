//! `M029` — ordered list prefixes must be either sequential
//! (1. 2. 3.) or all ones (1. 1. 1.). Mirrors MD029.

use crate::{line_byte_range, Rule, SourceFile, Violation};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Undetermined,
    Ones,
    Ordered { next: u32 },
}

pub struct M029OLPrefix;

impl M029OLPrefix {
    pub const CODE: &'static str = "M029";
}

impl Rule for M029OLPrefix {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut violations = Vec::new();
        let mut style: Option<Style> = None;
        for (idx, line) in file.contents.lines().enumerate() {
            let trimmed = line.trim_start();
            let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() || !trimmed[digits.len()..].starts_with('.') {
                // Non-list, non-continuation line — reset the run.
                if !trimmed.is_empty() && !trimmed.starts_with(' ') {
                    style = None;
                }
                continue;
            }
            let Ok(num) = digits.parse::<u32>() else {
                continue;
            };
            let next_style = match style {
                None if num == 1 => Style::Undetermined,
                None => Style::Ordered { next: num + 1 },
                Some(Style::Undetermined | Style::Ones) if num == 1 => Style::Ones,
                Some(Style::Undetermined) if num == 2 => Style::Ordered { next: 3 },
                Some(Style::Ordered { next }) if num == next => Style::Ordered { next: next + 1 },
                _ => {
                    violations.push(violation(file, idx, num));
                    continue;
                }
            };
            style = Some(next_style);
        }
        violations
    }
}

fn violation(file: &SourceFile, idx: usize, num: u32) -> Violation {
    Violation {
        code: M029OLPrefix::CODE,
        path: file.path.clone(),
        line: idx + 1,
        range: line_byte_range(&file.contents, idx + 1),
        message: format!("Ordered list prefix `{num}` breaks the established style"),
    }
}

#[cfg(test)]
mod tests {
    use super::M029OLPrefix;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_sequential_ordered_list() {
        assert!(M029OLPrefix.lint(&f("1. a\n2. b\n3. c\n")).is_empty());
    }
    #[test]
    fn passes_with_all_ones_style() {
        assert!(M029OLPrefix.lint(&f("1. a\n1. b\n1. c\n")).is_empty());
    }
    #[test]
    fn flags_mixed_styles() {
        let v = M029OLPrefix.lint(&f("1. a\n2. b\n1. c\n"));
        assert!(!v.is_empty());
    }
}
