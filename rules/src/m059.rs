//! `M059` — link text must describe the destination, not be a
//! generic phrase or the bare URL. Mirrors MD059.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M059DescriptiveLinkText;

impl M059DescriptiveLinkText {
    pub const CODE: &'static str = "M059";
    pub const PROHIBITED: &'static [&'static str] =
        &["click here", "here", "link", "more", "read more"];
}

fn link_texts(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip image links — `![...](...)`.
        if bytes[i] == b'[' && (i == 0 || bytes[i - 1] != b'!') {
            let mut j = i + 1;
            let mut depth = 1;
            while j < bytes.len() {
                match bytes[j] {
                    b'[' => depth += 1,
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    b'\\' if j + 1 < bytes.len() => j += 1,
                    _ => {}
                }
                j += 1;
            }
            if j >= bytes.len() {
                i += 1;
                continue;
            }
            let text = &line[i + 1..j];
            if bytes.get(j + 1) == Some(&b'(') || bytes.get(j + 1) == Some(&b'[') {
                out.push(text.to_string());
                i = j + 2;
                continue;
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn strip_inline_markup(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '`' | '*' | '_'))
        .collect()
}

fn is_url(text: &str) -> bool {
    text.starts_with("http://") || text.starts_with("https://") || text.starts_with("www.")
}

fn is_definition_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('[') && t.contains("]:")
}

impl Rule for M059DescriptiveLinkText {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut violations = Vec::new();
        for (idx, line) in file.contents.lines().enumerate() {
            if is_definition_line(line) {
                continue;
            }
            for raw in link_texts(line) {
                let visible = strip_inline_markup(&raw).trim().to_string();
                if visible.is_empty() {
                    continue;
                }
                let normalized = visible.to_lowercase();
                if Self::PROHIBITED.contains(&normalized.as_str()) {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: format!("Link text is not descriptive: {visible}"),
                    });
                } else if is_url(&normalized) {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: format!("Link text is a bare URL: {visible}"),
                    });
                }
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M059DescriptiveLinkText;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_descriptive_text() {
        assert!(M059DescriptiveLinkText
            .lint(&f("See [our policy](https://x).\n"))
            .is_empty());
    }
    #[test]
    fn flags_click_here() {
        let v = M059DescriptiveLinkText.lint(&f("Click [here](https://x).\n"));
        assert_eq!(v.len(), 1);
    }
    #[test]
    fn flags_bare_url_as_link_text() {
        let v = M059DescriptiveLinkText.lint(&f("See [https://x.com](https://x.com).\n"));
        assert_eq!(v.len(), 1);
    }
}
