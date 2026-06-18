//! `M035` — horizontal rule style consistency. MD035.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M035HRStyle;

impl M035HRStyle {
    pub const CODE: &'static str = "M035";
}

fn classify_hr(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.len() < 3 {
        return None;
    }
    if t.chars().all(|c| c == '-') {
        Some("---")
    } else if t.chars().all(|c| c == '*') {
        Some("***")
    } else if t.chars().all(|c| c == '_') {
        Some("___")
    } else {
        None
    }
}

impl Rule for M035HRStyle {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut established: Option<&str> = None;
        let mut violations = Vec::new();
        for (idx, line) in file.contents.lines().enumerate() {
            let Some(style) = classify_hr(line) else {
                continue;
            };
            match established {
                None => established = Some(style),
                Some(prev) if prev != style => {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: format!(
                            "Inconsistent horizontal rule style (saw `{prev}` earlier, now `{style}`)"
                        ),
                    });
                }
                _ => {}
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M035HRStyle;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_consistent_hr_style() {
        assert!(M035HRStyle.lint(&f("---\nbody\n\n---\n")).is_empty());
    }
    #[test]
    fn flags_mixed_hr_styles() {
        let v = M035HRStyle.lint(&f("---\nbody\n\n***\n"));
        assert_eq!(v.len(), 1);
    }
}
