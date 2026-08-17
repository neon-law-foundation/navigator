//! `M053` — reference-link definitions (`[label]: dest`) must be
//! used by at least one reference or collapsed link. Mirrors MD053.

use crate::m052::{self};
use crate::{line_byte_range, Rule, SourceFile, Violation};
use std::collections::HashSet;

pub struct M053LinkImageReferenceDefinitions;

impl M053LinkImageReferenceDefinitions {
    pub const CODE: &'static str = "M053";
}

fn parse_definition(line: &str) -> Option<(String, String)> {
    let t = line.trim_start();
    let rest = t.strip_prefix('[')?;
    let end = rest.find("]:")?;
    let label = rest[..end].trim().to_string();
    let raw = rest[..end].to_string();
    if label.is_empty() {
        return None;
    }
    Some((raw, normalize_local(&label)))
}

fn normalize_local(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for ch in s.trim().chars().map(|c| c.to_ascii_lowercase()) {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}

impl Rule for M053LinkImageReferenceDefinitions {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        // Collect every label used as a reference/collapsed link in the document.
        let mut used: HashSet<String> = HashSet::new();
        for line in file.contents.lines() {
            for label in m052::used_labels_public(line) {
                used.insert(label);
            }
        }
        let mut violations = Vec::new();
        for (idx, line) in file.contents.lines().enumerate() {
            if let Some((raw, normalized)) = parse_definition(line) {
                if !used.contains(&normalized) {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: format!("Reference-link definition `[{raw}]` is unused"),
                    });
                }
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M053LinkImageReferenceDefinitions;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_when_definition_is_used() {
        assert!(M053LinkImageReferenceDefinitions
            .lint(&f("See [home][hp].\n\n[hp]: https://x\n"))
            .is_empty());
    }
    #[test]
    fn flags_unused_definition() {
        let v = M053LinkImageReferenceDefinitions.lint(&f("Body.\n\n[stale]: https://x\n"));
        assert_eq!(v.len(), 1);
    }
}
