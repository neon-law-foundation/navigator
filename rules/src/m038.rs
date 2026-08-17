//! `M038` — no space inside inline code spans. Mirrors
//! markdownlint MD038. Flags `` ` foo ` ``.

use crate::{line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M038NoSpaceInCode;

impl M038NoSpaceInCode {
    pub const CODE: &'static str = "M038";
}

/// Rebuild `line` with the inner padding of every code span removed
/// (`` ` foo ` `` → `` `foo` ``). A span is left untouched when trimming
/// would push a backtick up against the fence (the markdownlint
/// backtick-padding exception, e.g. ``` `` ` `` ```), or when the span is
/// all whitespace.
fn strip_code_padding(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            let ch = line[i..]
                .chars()
                .next()
                .expect("byte index on char boundary");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let ticks = bytes[i..].iter().take_while(|&&b| b == b'`').count();
        let start = i + ticks;
        // Find the matching closing run of the same length.
        let mut j = start;
        let mut close = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let run = bytes[j..].iter().take_while(|&&b| b == b'`').count();
                if run == ticks {
                    close = Some(j);
                    break;
                }
                j += run;
                continue;
            }
            j += 1;
        }
        let Some(j) = close else {
            // Unclosed run — copy the ticks and move on.
            out.push_str(&line[i..start]);
            i = start;
            continue;
        };
        let inner = &line[start..j];
        let trimmed = inner.trim_matches(' ');
        let safe = !trimmed.starts_with('`') && !trimmed.ends_with('`');
        if trimmed != inner && !trimmed.is_empty() && safe {
            out.push_str(&line[i..start]);
            out.push_str(trimmed);
            out.push_str(&line[j..j + ticks]);
        } else {
            out.push_str(&line[i..j + ticks]);
        }
        i = j + ticks;
    }
    out
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

    fn fix(&self, file: &SourceFile, violation: &Violation) -> Option<TextEdit> {
        let line = &file.contents[violation.range.clone()];
        let fixed = strip_code_padding(line);
        (fixed != *line).then_some(TextEdit {
            range: violation.range.clone(),
            new_text: fixed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{strip_code_padding, M038NoSpaceInCode};
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

    fn fixed(body: &str) -> String {
        let file = f(body);
        let v = M038NoSpaceInCode.lint(&file);
        let edit = M038NoSpaceInCode.fix(&file, &v[0]).expect("a fix");
        let mut out = file.contents.clone();
        out.replace_range(edit.range, &edit.new_text);
        out
    }

    #[test]
    fn fix_trims_code_span_padding() {
        assert_eq!(fixed("Use ` foo ` here.\n"), "Use `foo` here.\n");
        assert_eq!(fixed("`left ` and ` right`\n"), "`left` and `right`\n");
    }

    #[test]
    fn fix_is_idempotent() {
        let once = fixed("Use ` foo ` here.\n");
        assert!(M038NoSpaceInCode.lint(&f(&once)).is_empty());
    }

    #[test]
    fn fix_refuses_when_trimming_would_abut_a_backtick() {
        // A span padded so its content is a literal backtick must keep
        // its spaces — collapsing them would merge fences. `strip` is a
        // no-op here, so `fix()` yields nothing to change.
        let line = "before `` ` `` after";
        assert_eq!(strip_code_padding(line), line);
    }
}
