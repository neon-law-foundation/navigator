//! `M026` — headings must not end with trailing punctuation.
//! Mirrors markdownlint MD026.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

const TRAILING_PUNCT: &[char] = &['.', ',', ';', ':', '!', '?'];

pub struct M026NoTrailingPunctuation;

impl M026NoTrailingPunctuation {
    pub const CODE: &'static str = "M026";
}

impl Rule for M026NoTrailingPunctuation {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        frontmatter::body_lines(&file.contents)
            .into_iter()
            .filter_map(|(line_no, line)| {
                let trimmed = line.trim_start();
                let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
                if hashes == 0 || hashes > 6 {
                    return None;
                }
                if trimmed.as_bytes().get(hashes) != Some(&b' ') {
                    return None;
                }
                let text = trimmed[hashes + 1..].trim_end_matches('#').trim_end();
                let last_char = text.chars().last()?;
                if TRAILING_PUNCT.contains(&last_char) {
                    Some(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: line_no,
                        range: line_byte_range(&file.contents, line_no),
                        message: format!("Heading ends with trailing punctuation `{last_char}`"),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::M026NoTrailingPunctuation;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_when_heading_has_no_trailing_punctuation() {
        assert!(M026NoTrailingPunctuation
            .lint(&f("# Clean Title\n## Another\n"))
            .is_empty());
    }
    #[test]
    fn flags_period_question_exclamation_colon_semicolon_comma() {
        for end in ['.', '?', '!', ':', ';', ','] {
            let body = format!("# Heading{end}\n");
            let v = M026NoTrailingPunctuation.lint(&f(&body));
            assert_eq!(v.len(), 1, "punct `{end}` should flag");
        }
    }
}
