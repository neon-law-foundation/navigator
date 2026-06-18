//! `F105` — frontmatter must declare a `confidential:` field set
//! to `true` or `false`. `README.md` files are exempt.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct F105ConfidentialRequired;

impl F105ConfidentialRequired {
    pub const CODE: &'static str = "F105";
}

impl Rule for F105ConfidentialRequired {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        if file
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("README.md"))
        {
            return Vec::new();
        }

        let report = |message: &str| -> Vec<Violation> {
            vec![Violation {
                code: Self::CODE,
                path: file.path.clone(),
                line: 1,
                range: line_byte_range(&file.contents, 1),
                message: message.to_string(),
            }]
        };

        let Some(fm) = frontmatter::extract(&file.contents) else {
            return report("Missing frontmatter (file must declare `confidential:`)");
        };
        match frontmatter::field(fm, "confidential") {
            None => report("Frontmatter is missing required `confidential:` field"),
            Some(value) => match value.as_str() {
                "true" | "false" => Vec::new(),
                _ => report("Frontmatter `confidential:` must be `true` or `false`"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::F105ConfidentialRequired;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(name: &str, body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from(name),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_when_confidential_is_true() {
        let f = file("trust.md", "---\ntitle: T\nconfidential: true\n---\n");
        assert!(F105ConfidentialRequired.lint(&f).is_empty());
    }

    #[test]
    fn passes_when_confidential_is_false() {
        let f = file("trust.md", "---\ntitle: T\nconfidential: false\n---\n");
        assert!(F105ConfidentialRequired.lint(&f).is_empty());
    }

    #[test]
    fn flags_missing_field() {
        let f = file("trust.md", "---\ntitle: T\n---\n");
        let v = F105ConfidentialRequired.lint(&f);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "F105");
        assert!(v[0].message.contains("missing"));
    }

    #[test]
    fn flags_non_boolean_value() {
        let f = file("trust.md", "---\nconfidential: maybe\n---\n");
        let v = F105ConfidentialRequired.lint(&f);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("must be `true` or `false`"));
    }

    #[test]
    fn flags_missing_frontmatter_entirely() {
        let v = F105ConfidentialRequired.lint(&file("trust.md", "No frontmatter."));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("Missing frontmatter"));
    }

    #[test]
    fn readme_files_are_exempt() {
        assert!(F105ConfidentialRequired
            .lint(&file("README.md", "No frontmatter at all"))
            .is_empty());
        // Case-insensitive — `readme.md` also exempt.
        assert!(F105ConfidentialRequired
            .lint(&file("readme.md", ""))
            .is_empty());
    }
}
