//! `M049` — single-marker emphasis (`*x*` vs. `_x_`) must use a
//! consistent marker. Mirrors MD049 (`style: "consistent"`).

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct M049EmphasisStyle;

impl M049EmphasisStyle {
    pub const CODE: &'static str = "M049";
}

fn find_single_emphasis(line: &str) -> Vec<char> {
    let mut markers = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'*' || c == b'_' {
            // Must be a single marker (next byte is not the same char)
            let run = bytes[i..].iter().take_while(|&&b| b == c).count();
            if run == 1 {
                // Look for a matching closer somewhere later on the same line.
                if let Some(off) = bytes[i + 1..].iter().position(|&b| b == c) {
                    let inside = &bytes[i + 1..i + 1 + off];
                    if !inside.is_empty() && inside[0] != b' ' && *inside.last().unwrap() != b' ' {
                        markers.push(c as char);
                        i += off + 2;
                        continue;
                    }
                }
            }
            i += run;
        } else {
            i += 1;
        }
    }
    markers
}

impl Rule for M049EmphasisStyle {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut expected: Option<char> = None;
        let mut violations = Vec::new();
        for (line_no, line) in frontmatter::body_lines(&file.contents) {
            // Mask both code spans and `[text](url)` URLs so neither
            // backticked content nor link targets trigger false emphasis.
            let masked = frontmatter::mask_link_urls(&frontmatter::mask_code_spans(line));
            for marker in find_single_emphasis(&masked) {
                match expected {
                    None => expected = Some(marker),
                    Some(prev) if prev != marker => {
                        violations.push(Violation {
                            code: Self::CODE,
                            path: file.path.clone(),
                            line: line_no,
                            range: line_byte_range(&file.contents, line_no),
                            message: format!(
                                "Emphasis marker `{marker}` does not match first-use marker `{prev}`"
                            ),
                        });
                    }
                    _ => {}
                }
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M049EmphasisStyle;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_consistent_asterisk_emphasis() {
        assert!(M049EmphasisStyle.lint(&f("*hi* and *there*\n")).is_empty());
    }
    #[test]
    fn flags_underscore_after_asterisk() {
        let v = M049EmphasisStyle.lint(&f("*hi* then _yo_\n"));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn ignores_underscores_in_link_urls() {
        let s = "*ok* — [foo](../path/snake_case_name.rs) is fine\n";
        assert!(M049EmphasisStyle.lint(&f(s)).is_empty());
    }
}
