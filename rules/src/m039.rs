//! `M039` — no leading/trailing whitespace inside link text.
//! Mirrors markdownlint MD039. Flags `[ foo ](url)`.

use crate::{line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M039NoSpaceInLinks;

impl M039NoSpaceInLinks {
    pub const CODE: &'static str = "M039";
}

/// Rebuild `line` with the inner whitespace of every padded inline
/// link text (`[ foo ](url)` → `[foo](url)`) removed. Non-link `[`
/// runs and already-tight link text are copied verbatim.
fn strip_link_text_padding(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(rel) = bytes[i + 1..].iter().position(|&b| b == b']') {
                let close = i + 1 + rel;
                if bytes.get(close + 1) == Some(&b'(') {
                    let inner = &line[i + 1..close];
                    let trimmed = inner.trim_matches(' ');
                    if trimmed != inner && !trimmed.is_empty() {
                        out.push('[');
                        out.push_str(trimmed);
                        out.push(']');
                        i = close + 1;
                        continue;
                    }
                }
            }
            out.push('[');
            i += 1;
            continue;
        }
        let ch = line[i..]
            .chars()
            .next()
            .expect("byte index on char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
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

    fn fix(&self, file: &SourceFile, violation: &Violation) -> Option<TextEdit> {
        let line = &file.contents[violation.range.clone()];
        let fixed = strip_link_text_padding(line);
        (fixed != *line).then_some(TextEdit {
            range: violation.range.clone(),
            new_text: fixed,
        })
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

    fn fixed(body: &str) -> String {
        let file = f(body);
        let v = M039NoSpaceInLinks.lint(&file);
        let edit = M039NoSpaceInLinks.fix(&file, &v[0]).expect("a fix");
        let mut out = file.contents.clone();
        out.replace_range(edit.range, &edit.new_text);
        out
    }

    #[test]
    fn fix_trims_link_text_padding() {
        assert_eq!(
            fixed("See [ home ](https://example.com).\n"),
            "See [home](https://example.com).\n"
        );
        assert_eq!(
            fixed("[left ](u) and [ right](v)\n"),
            "[left](u) and [right](v)\n"
        );
    }

    #[test]
    fn fix_is_idempotent() {
        let once = fixed("See [ home ](u).\n");
        assert!(M039NoSpaceInLinks.lint(&f(&once)).is_empty());
    }
}
