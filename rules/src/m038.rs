//! `M038` — no space inside inline code spans. Mirrors
//! markdownlint MD038. Flags `` ` foo ` ``.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M038NoSpaceInCode;

impl M038NoSpaceInCode {
    pub const CODE: &'static str = "M038";
}

impl Rule for M038NoSpaceInCode {
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
                while i < bytes.len() {
                    if bytes[i] != b'`' {
                        i += 1;
                        continue;
                    }
                    let backticks = bytes[i..].iter().take_while(|&&b| b == b'`').count();
                    let start = i + backticks;
                    // Find matching closing run of the same length.
                    let mut j = start;
                    while j < bytes.len() {
                        if bytes[j] == b'`' {
                            let run = bytes[j..].iter().take_while(|&&b| b == b'`').count();
                            if run == backticks {
                                break;
                            }
                            j += run;
                            continue;
                        }
                        j += 1;
                    }
                    if j >= bytes.len() {
                        i = start;
                        continue;
                    }
                    let inner = &bytes[start..j];
                    if !inner.is_empty()
                        && (inner.first() == Some(&b' ') || inner.last() == Some(&b' '))
                        && inner.iter().any(|&b| b != b' ')
                    {
                        return Some(Violation {
                            code: Self::CODE,
                            path: file.path.clone(),
                            line: idx + 1,
                            range: line_byte_range(&file.contents, idx + 1),
                            message:
                                "Inline code span must not have leading or trailing whitespace"
                                    .to_string(),
                        });
                    }
                    i = j + backticks;
                }
                None
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::M038NoSpaceInCode;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_tight_code_span() {
        assert!(M038NoSpaceInCode.lint(&f("Use `foo` here.\n")).is_empty());
    }
    #[test]
    fn flags_space_inside_code_span() {
        let v = M038NoSpaceInCode.lint(&f("Use ` foo ` here.\n"));
        assert_eq!(v.len(), 1);
    }
}
