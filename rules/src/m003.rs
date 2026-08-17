//! `M003` — heading style must be consistent across the file.
//! Mirrors MD003. Heading kinds: ATX (`# h`), ATX-closed (`# h #`),
//! Setext (h underlined with `===` or `---`).

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

#[derive(Clone, Copy, PartialEq, Eq)]
enum HeadingStyle {
    Atx,
    AtxClosed,
    Setext,
}

fn style_of(line: &str, next: Option<&str>) -> Option<HeadingStyle> {
    let t = line.trim_start();
    if t.starts_with('#') {
        let hashes = t.bytes().take_while(|&b| b == b'#').count();
        if hashes == 0 || hashes > 6 {
            return None;
        }
        if t.trim_end().ends_with('#') && t.trim_end().len() > hashes {
            return Some(HeadingStyle::AtxClosed);
        }
        return Some(HeadingStyle::Atx);
    }
    if let Some(next_line) = next {
        let nt = next_line.trim();
        if !t.is_empty()
            && !nt.is_empty()
            && (nt.chars().all(|c| c == '=') || nt.chars().all(|c| c == '-'))
        {
            return Some(HeadingStyle::Setext);
        }
    }
    None
}

pub struct M003HeadingStyle;

impl M003HeadingStyle {
    pub const CODE: &'static str = "M003";
}

impl Rule for M003HeadingStyle {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let lines: Vec<&str> = file.contents.lines().collect();
        let fm = frontmatter::line_range(&file.contents);
        let mut established: Option<HeadingStyle> = None;
        let mut violations = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            let line_no = idx + 1;
            if fm.as_ref().is_some_and(|r| r.contains(&line_no)) {
                continue;
            }
            let Some(style) = style_of(line, lines.get(idx + 1).copied()) else {
                continue;
            };
            match established {
                None => established = Some(style),
                Some(prev) if prev != style => {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: "Heading style differs from the first heading in the file"
                            .to_string(),
                    });
                }
                _ => {}
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M003HeadingStyle;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_consistent_atx() {
        assert!(M003HeadingStyle
            .lint(&f("# H1\n## H2\n### H3\n"))
            .is_empty());
    }
    #[test]
    fn flags_atx_then_setext_mix() {
        let v = M003HeadingStyle.lint(&f("# H1\n\nbody\n\nH2\n==\n"));
        assert_eq!(v.len(), 1);
    }
}
