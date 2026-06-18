//! `M042` — links must not be empty. Mirrors markdownlint MD042.
//! Flags `[]()`, `[text]()`, `[](url)`, and `(#)`-only fragments.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M042NoEmptyLinks;

impl M042NoEmptyLinks {
    pub const CODE: &'static str = "M042";
}

impl Rule for M042NoEmptyLinks {
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
                    if bytes[i] != b'[' {
                        i += 1;
                        continue;
                    }
                    let Some(end) = bytes[i + 1..].iter().position(|&b| b == b']') else {
                        break;
                    };
                    let close_text = i + 1 + end;
                    if bytes.get(close_text + 1) != Some(&b'(') {
                        i = close_text + 1;
                        continue;
                    }
                    let Some(close_url_off) =
                        bytes[close_text + 2..].iter().position(|&b| b == b')')
                    else {
                        break;
                    };
                    let close_url = close_text + 2 + close_url_off;
                    let text = std::str::from_utf8(&bytes[i + 1..close_text])
                        .unwrap_or("")
                        .trim();
                    let url = std::str::from_utf8(&bytes[close_text + 2..close_url])
                        .unwrap_or("")
                        .trim();
                    if text.is_empty() || url.is_empty() || url == "#" {
                        return Some(Violation {
                            code: Self::CODE,
                            path: file.path.clone(),
                            line: idx + 1,
                            range: line_byte_range(&file.contents, idx + 1),
                            message: "Link must have both text and a non-fragment destination"
                                .to_string(),
                        });
                    }
                    i = close_url + 1;
                }
                None
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::M042NoEmptyLinks;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_complete_links() {
        assert!(M042NoEmptyLinks
            .lint(&f("See [home](https://example.com).\n"))
            .is_empty());
    }
    #[test]
    fn flags_link_with_empty_destination() {
        let v = M042NoEmptyLinks.lint(&f("See [home]().\n"));
        assert_eq!(v.len(), 1);
    }
    #[test]
    fn flags_link_with_empty_text() {
        let v = M042NoEmptyLinks.lint(&f("See [](url).\n"));
        assert_eq!(v.len(), 1);
    }
}
