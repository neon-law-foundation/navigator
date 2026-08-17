//! `M045` — images must have alt text. Mirrors MD045.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M045NoAltText;

impl M045NoAltText {
    pub const CODE: &'static str = "M045";
}

impl Rule for M045NoAltText {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        file.contents
            .lines()
            .enumerate()
            .filter_map(|(idx, line)| {
                let bytes = line.as_bytes();
                let mut i = 0;
                while i + 3 < bytes.len() {
                    if bytes[i] == b'!' && bytes[i + 1] == b'[' {
                        let Some(close) = bytes[i + 2..].iter().position(|&b| b == b']') else {
                            break;
                        };
                        let alt = std::str::from_utf8(&bytes[i + 2..i + 2 + close])
                            .unwrap_or("")
                            .trim();
                        if alt.is_empty() {
                            return Some(Violation {
                                code: Self::CODE,
                                path: file.path.clone(),
                                line: idx + 1,
                                range: line_byte_range(&file.contents, idx + 1),
                                message: "Image is missing alt text".to_string(),
                            });
                        }
                        i = i + 2 + close;
                    }
                    i += 1;
                }
                None
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::M045NoAltText;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_alt_text() {
        assert!(M045NoAltText.lint(&f("![Logo](logo.png)\n")).is_empty());
    }
    #[test]
    fn flags_missing_alt_text() {
        let v = M045NoAltText.lint(&f("![](logo.png)\n"));
        assert_eq!(v.len(), 1);
    }
}
