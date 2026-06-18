//! `M054` — inline links whose visible text equals the URL should be
//! written as autolinks (`<url>`) instead of `[url](url)`.
//! Mirrors MD054's `url_inline` default behavior.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M054LinkImageStyle;

impl M054LinkImageStyle {
    pub const CODE: &'static str = "M054";
}

fn inline_links_with_text_eq_dest(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip image markers: caller checks links only.
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
            let text = line[i + 1..j].trim().to_string();
            if bytes.get(j + 1) == Some(&b'(') {
                let start = j + 2;
                let mut k = start;
                let mut d = 1;
                while k < bytes.len() && d > 0 {
                    match bytes[k] {
                        b'(' => d += 1,
                        b')' => {
                            d -= 1;
                            if d == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                if d == 0 {
                    let raw = line[start..k].trim();
                    let dest = raw
                        .strip_prefix('<')
                        .and_then(|s| s.strip_suffix('>'))
                        .unwrap_or(raw);
                    if !text.is_empty() && text == dest {
                        out.push(dest.to_string());
                    }
                    i = k + 1;
                    continue;
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

impl Rule for M054LinkImageStyle {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut violations = Vec::new();
        for (idx, line) in file.contents.lines().enumerate() {
            for url in inline_links_with_text_eq_dest(line) {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: idx + 1,
                    range: line_byte_range(&file.contents, idx + 1),
                    message: format!(
                        "Link text duplicates URL `{url}` — use an autolink `<{url}>` instead"
                    ),
                });
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M054LinkImageStyle;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_distinct_link_text_and_url() {
        assert!(M054LinkImageStyle
            .lint(&f("See [home](https://x).\n"))
            .is_empty());
    }
    #[test]
    fn flags_text_equal_to_destination() {
        let v = M054LinkImageStyle.lint(&f("See [https://x](https://x).\n"));
        assert_eq!(v.len(), 1);
    }
}
