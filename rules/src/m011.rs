//! `M011` — reversed link syntax `(text)[url]`. Mirrors MD011.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M011NoReversedLinks;

impl M011NoReversedLinks {
    pub const CODE: &'static str = "M011";
}

impl Rule for M011NoReversedLinks {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        file.contents
            .lines()
            .enumerate()
            .filter_map(|(idx, line)| {
                if line.contains(")[") {
                    Some(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message:
                            "Possible reversed link syntax `(text)[url]` — should be `[text](url)`"
                                .to_string(),
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
    use super::M011NoReversedLinks;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_proper_link_syntax() {
        assert!(M011NoReversedLinks
            .lint(&f("See [home](https://x).\n"))
            .is_empty());
    }
    #[test]
    fn flags_reversed_link_syntax() {
        let v = M011NoReversedLinks.lint(&f("See (home)[https://x].\n"));
        assert_eq!(v.len(), 1);
    }
}
