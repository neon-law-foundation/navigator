//! `M025` — a document must have at most one top-level (H1) heading.
//! Mirrors markdownlint MD025 (single-title / single-h1).
//!
//! The first H1 is the document title; a second one usually means a
//! section was over-promoted to `#` instead of `##`. Only ATX headings
//! (`# Title`) are considered — the workspace writes headings in ATX
//! style (enforced by [`crate::M003HeadingStyle`]).

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct M025SingleH1;

impl M025SingleH1 {
    pub const CODE: &'static str = "M025";
}

impl Rule for M025SingleH1 {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        crate::description_for_code(Self::CODE)
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut seen_h1 = false;
        let mut violations = Vec::new();
        for (line_no, line) in frontmatter::body_lines(&file.contents) {
            if !is_atx_h1(line) {
                continue;
            }
            if seen_h1 {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: line_no,
                    range: line_byte_range(&file.contents, line_no),
                    message: "Multiple top-level (H1) headings; a document should have one \
                              title — demote later `#` headings to `##`"
                        .to_string(),
                });
            }
            seen_h1 = true;
        }
        violations
    }
}

/// True when `line` is an ATX level-1 heading: a single leading `#`
/// followed by a space (or end of line), not `##`.
fn is_atx_h1(line: &str) -> bool {
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if hashes != 1 {
        return false;
    }
    matches!(trimmed.as_bytes().get(1), Some(b' ') | None)
}

#[cfg(test)]
mod tests {
    use super::M025SingleH1;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_with_a_single_h1() {
        let body = "# Title\n\n## Section\n\n## Another\n";
        assert!(M025SingleH1.lint(&file(body)).is_empty());
    }

    #[test]
    fn flags_a_second_h1() {
        let body = "# Title\n\nbody\n\n# Second Title\n";
        let v = M025SingleH1.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].code, "M025");
        assert_eq!(v[0].line, 5);
    }

    #[test]
    fn flags_every_extra_h1() {
        let body = "# One\n# Two\n# Three\n";
        let v = M025SingleH1.lint(&file(body));
        assert_eq!(v.iter().map(|x| x.line).collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn does_not_count_deeper_headings() {
        let body = "# Title\n\n## H2\n### H3\n#### H4\n";
        assert!(M025SingleH1.lint(&file(body)).is_empty());
    }

    #[test]
    fn ignores_hashes_inside_code_fences() {
        let body = "# Title\n\n```\n# not a heading\n# also not\n```\n";
        assert!(M025SingleH1.lint(&file(body)).is_empty());
    }

    #[test]
    fn ignores_frontmatter_and_bare_hash_lines() {
        // `#nav` (no space) is not a heading; frontmatter is skipped.
        let body = "---\ntitle: X\n---\n\n# Real Title\n\n#nav-anchor\n";
        assert!(M025SingleH1.lint(&file(body)).is_empty());
    }
}
