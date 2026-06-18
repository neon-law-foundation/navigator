//! `M037` — no space inside single-marker emphasis. Mirrors
//! markdownlint MD037. Flags patterns like `* foo *`, `*foo *`, or
//! `* foo*` where the marker run is a *single* `*` or `_` (not `**`
//! or `__` — those are strong, handled by M050).

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct M037NoSpaceInEmphasis;

impl M037NoSpaceInEmphasis {
    pub const CODE: &'static str = "M037";
}

impl Rule for M037NoSpaceInEmphasis {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        frontmatter::body_lines(&file.contents)
            .into_iter()
            .filter_map(|(line_no, line)| {
                let masked = frontmatter::mask_code_spans(line);
                has_space_in_emphasis(masked.as_bytes()).then(|| Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: line_no,
                    range: line_byte_range(&file.contents, line_no),
                    message: "Emphasis must not have leading or trailing whitespace inside"
                        .to_string(),
                })
            })
            .collect()
    }
}

/// True when the line contains a single-marker emphasis run whose
/// inner text starts or ends with whitespace. Runs of `**` or `__`
/// (strong) are skipped — they're M050's responsibility.
fn has_space_in_emphasis(bytes: &[u8]) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c != b'*' && c != b'_' {
            i += 1;
            continue;
        }
        // Determine the marker run length at this position.
        let run = bytes[i..].iter().take_while(|&&b| b == c).count();
        if run != 1 {
            i += run;
            continue;
        }
        // Single marker. Look for matching single marker on the same
        // line that is also not part of a multi-marker run.
        let inner_start = i + 1;
        let mut j = inner_start;
        let mut found = None;
        while j < bytes.len() {
            if bytes[j] == c {
                let close_run = bytes[j..].iter().take_while(|&&b| b == c).count();
                if close_run == 1 {
                    found = Some(j);
                    break;
                }
                j += close_run;
                continue;
            }
            j += 1;
        }
        let Some(close) = found else {
            i += 1;
            continue;
        };
        if close == inner_start {
            // Empty pair `**` or `__`-style would have been a longer
            // run, so this is `*` immediately followed by `*` — odd
            // input. Skip.
            i = close + 1;
            continue;
        }
        let leading_space = bytes[inner_start] == b' ' || bytes[inner_start] == b'\t';
        let trailing_space = bytes[close - 1] == b' ' || bytes[close - 1] == b'\t';
        if leading_space || trailing_space {
            return true;
        }
        i = close + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::M037NoSpaceInEmphasis;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_tight_emphasis() {
        assert!(M037NoSpaceInEmphasis
            .lint(&f("This *is* fine.\n"))
            .is_empty());
    }
    #[test]
    fn flags_space_inside_emphasis_markers() {
        let v = M037NoSpaceInEmphasis.lint(&f("This * is * not fine.\n"));
        assert_eq!(v.len(), 1);
    }
    #[test]
    fn ignores_strong_markers() {
        assert!(M037NoSpaceInEmphasis
            .lint(&f(
                "As of **March 1, 2026** (the \"**Effective Date**\").\n"
            ))
            .is_empty());
    }
    #[test]
    fn ignores_emphasis_inside_code_spans() {
        assert!(M037NoSpaceInEmphasis
            .lint(&f("Look at `* foo *` inside backticks.\n"))
            .is_empty());
    }
}
