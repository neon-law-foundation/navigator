//! `M050` — strong emphasis (`**x**` vs. `__x__`) must use a
//! consistent marker. Mirrors MD050.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct M050StrongStyle;

impl M050StrongStyle {
    pub const CODE: &'static str = "M050";
}

fn find_strong(line: &str) -> Vec<char> {
    let mut markers = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let c = bytes[i];
        if (c == b'*' || c == b'_') && bytes[i + 1] == c {
            // Look for matching `cc` somewhere later.
            let mut j = i + 2;
            while j + 1 < bytes.len() {
                if bytes[j] == c && bytes[j + 1] == c {
                    break;
                }
                j += 1;
            }
            if j + 1 < bytes.len() && bytes[j] == c && bytes[j + 1] == c {
                let inside = &bytes[i + 2..j];
                if !inside.is_empty() && inside[0] != b' ' && *inside.last().unwrap() != b' ' {
                    markers.push(c as char);
                    i = j + 2;
                    continue;
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    markers
}

impl Rule for M050StrongStyle {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut expected: Option<char> = None;
        let mut violations = Vec::new();
        for (line_no, line) in frontmatter::body_lines(&file.contents) {
            let masked = frontmatter::mask_code_spans(line);
            for marker in find_strong(&masked) {
                match expected {
                    None => expected = Some(marker),
                    Some(prev) if prev != marker => {
                        violations.push(Violation {
                            code: Self::CODE,
                            path: file.path.clone(),
                            line: line_no,
                            range: line_byte_range(&file.contents, line_no),
                            message: format!(
                                "Strong marker `{marker}{marker}` does not match first-use marker `{prev}{prev}`"
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
    use super::M050StrongStyle;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_consistent_asterisk_strong() {
        assert!(M050StrongStyle
            .lint(&f("**hi** and **there**\n"))
            .is_empty());
    }
    #[test]
    fn flags_underscore_strong_after_asterisk() {
        let v = M050StrongStyle.lint(&f("**hi** then __yo__\n"));
        assert_eq!(v.len(), 1);
    }
}
