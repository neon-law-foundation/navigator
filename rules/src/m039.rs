//! `M039` — no leading/trailing whitespace inside link text.
//! Mirrors markdownlint MD039. Flags `[ foo ](url)`.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M039NoSpaceInLinks;

impl M039NoSpaceInLinks {
    pub const CODE: &'static str = "M039";
}

impl Rule for M039NoSpaceInLinks {
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
                while i + 2 < bytes.len() {
                    if bytes[i] != b'[' {
                        i += 1;
                        continue;
                    }
                    let Some(end) = bytes[i + 1..].iter().position(|&b| b == b']') else {
                        break;
                    };
                    let close = i + 1 + end;
                    if bytes.get(close + 1) == Some(&b'(') {
                        let inner = &bytes[i + 1..close];
                        if !inner.is_empty()
                            && (inner.first() == Some(&b' ') || inner.last() == Some(&b' '))
                        {
                            return Some(Violation {
                                code: Self::CODE,
                                path: file.path.clone(),
                                line: idx + 1,
                                range: line_byte_range(&file.contents, idx + 1),
                                message: "Link text must not have leading or trailing whitespace"
                                    .to_string(),
                            });
                        }
                    }
                    i = close + 1;
                }
                None
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::M039NoSpaceInLinks;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_tight_link_text() {
        assert!(M039NoSpaceInLinks
            .lint(&f("See [home](https://example.com).\n"))
            .is_empty());
    }
    #[test]
    fn flags_space_in_link_text() {
        let v = M039NoSpaceInLinks.lint(&f("See [ home ](https://example.com).\n"));
        assert_eq!(v.len(), 1);
    }
}
