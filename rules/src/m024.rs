//! `M024` — no two headings may have the same trimmed text.
//! Mirrors markdownlint MD024 (default `siblings_only=false`).

use std::collections::HashSet;

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M024NoDuplicateHeading;

impl M024NoDuplicateHeading {
    pub const CODE: &'static str = "M024";
}

impl Rule for M024NoDuplicateHeading {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut violations = Vec::new();
        for (idx, line) in file.contents.lines().enumerate() {
            let trimmed = line.trim_start();
            let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
            if hashes == 0 || hashes > 6 {
                continue;
            }
            if trimmed.as_bytes().get(hashes) != Some(&b' ') {
                continue;
            }
            let text = trimmed[hashes + 1..]
                .trim_end_matches('#')
                .trim()
                .to_string();
            if !seen.insert(text.clone()) {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: idx + 1,
                    range: line_byte_range(&file.contents, idx + 1),
                    message: format!("Duplicate heading text `{text}`"),
                });
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M024NoDuplicateHeading;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_unique_headings() {
        assert!(M024NoDuplicateHeading
            .lint(&f("# A\n## B\n### C\n"))
            .is_empty());
    }
    #[test]
    fn flags_duplicate_heading_text_across_levels() {
        let v = M024NoDuplicateHeading.lint(&f("# Intro\nbody\n## Intro\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 3);
    }
}
