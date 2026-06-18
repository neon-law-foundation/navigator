//! `M051` — same-document link fragments (`[t](#frag)`) must match a
//! heading slug somewhere in the same file. Mirrors MD051.

use crate::{line_byte_range, Rule, SourceFile, Violation};
use std::collections::HashSet;

pub struct M051LinkFragments;

impl M051LinkFragments {
    pub const CODE: &'static str = "M051";
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.to_lowercase().chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('-');
        }
    }
    out
}

fn heading_text(line: &str) -> Option<String> {
    let t = line.trim_start();
    if !t.starts_with('#') {
        return None;
    }
    let after = t.trim_start_matches('#');
    if !after.starts_with(' ') && !after.is_empty() {
        return None;
    }
    Some(after.trim_start().trim_end_matches(['#', ' ']).to_string())
}

fn collect_slugs(contents: &str) -> HashSet<String> {
    let mut slugs = HashSet::new();
    for line in contents.lines() {
        if let Some(text) = heading_text(line) {
            slugs.insert(slugify(&text));
        }
    }
    slugs
}

fn fragments_in_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b'(' {
            let start = i + 2;
            let mut j = start;
            let mut depth = 1;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if depth == 0 && j <= bytes.len() {
                let dest = line[start..j].trim();
                if let Some(frag) = same_doc_fragment(dest) {
                    out.push(frag);
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn same_doc_fragment(dest: &str) -> Option<String> {
    let trimmed = dest
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(dest);
    let hash = trimmed.find('#')?;
    if hash != 0 {
        return None;
    }
    Some(trimmed[1..].to_string())
}

impl Rule for M051LinkFragments {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let slugs = collect_slugs(&file.contents);
        let mut violations = Vec::new();
        for (idx, line) in file.contents.lines().enumerate() {
            for fragment in fragments_in_line(line) {
                let trimmed = fragment.trim();
                if trimmed.is_empty() || trimmed == "top" {
                    continue;
                }
                let slug = slugify(trimmed);
                if !slugs.contains(&slug) {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: format!("Link fragment `#{fragment}` does not match any heading"),
                    });
                }
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M051LinkFragments;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_when_fragment_matches_heading() {
        assert!(M051LinkFragments
            .lint(&f("# My Heading\n\nSee [home](#my-heading).\n"))
            .is_empty());
    }
    #[test]
    fn flags_fragment_with_no_matching_heading() {
        let v = M051LinkFragments.lint(&f("# Real\n\nSee [bad](#nope).\n"));
        assert_eq!(v.len(), 1);
    }
    #[test]
    fn ignores_cross_document_fragment() {
        assert!(M051LinkFragments
            .lint(&f("See [other](other.md#missing).\n"))
            .is_empty());
    }
    #[test]
    fn allows_top_anchor() {
        assert!(M051LinkFragments.lint(&f("See [up](#top).\n")).is_empty());
    }
}
