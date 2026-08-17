//! `D002` — a section must be in the declared kind's catalog.
//!
//! Pick-and-choose is real, but it stays inside the kind. A section that
//! exists in the registry yet does not belong to the page being authored
//! is an error, because that is what makes per-kind checking mean
//! anything: an authority table on a deliverable package is not a
//! deliberate choice, it is a kind that was declared wrong.

use crate::dashboard::{catalog, Section};
use crate::{kind, line_byte_range, Rule, SourceFile, Violation};

/// `D002` — every section must be in the declared kind's catalog.
pub struct D002OutOfCatalogSection;

impl D002OutOfCatalogSection {
    pub const CODE: &'static str = "D002";
}

impl Rule for D002OutOfCatalogSection {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        crate::description_for_code(Self::CODE)
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let Some(declared) = kind::declared(&file.contents) else {
            return Vec::new();
        };
        if !declared.is_dashboard() {
            return Vec::new();
        }
        let Some(lenses) = crate::dashboard::declared_lenses(&file.contents) else {
            return Vec::new();
        };
        let allowed = catalog(declared);
        let allowed_names: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
        let mut out = Vec::new();
        for (lens, sections) in lenses {
            for name in sections {
                // An unrecognized name is D001's report, not this one.
                let Some(section) = Section::parse(&name) else {
                    continue;
                };
                if allowed.contains(&section) {
                    continue;
                }
                let line = crate::dashboard::section_line(&file.contents, &name);
                out.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line,
                    range: line_byte_range(&file.contents, line),
                    message: format!(
                        "Lens `{lens}` declares `{name}`, which is not in the `{}` catalog. \
                         This kind may carry: {}",
                        declared.as_str(),
                        allowed_names.join(", "),
                    ),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::D002OutOfCatalogSection;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("hub.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn a_section_in_the_catalog_passes() {
        let body = "---\nkind: deliverable_package\nlenses:\n  lawyer: [package_manifest, download_list]\n---\n";
        assert!(D002OutOfCatalogSection.lint(&file(body)).is_empty());
    }

    #[test]
    fn a_real_section_from_another_kind_is_flagged() {
        let body =
            "---\nkind: deliverable_package\nlenses:\n  lawyer: [package_manifest, discovery_pairs]\n---\n";
        let v = D002OutOfCatalogSection.lint(&file(body));
        assert_eq!(v.len(), 1, "got {v:?}");
        assert_eq!(v[0].code, "D002");
        assert!(
            v[0].message.contains("`discovery_pairs`"),
            "{}",
            v[0].message
        );
        assert!(
            v[0].message.contains("package_manifest"),
            "the message must name the catalog: {}",
            v[0].message,
        );
    }

    #[test]
    fn an_unknown_name_is_left_to_d001() {
        // Exactly one diagnostic per mistake: a name that is in no
        // vocabulary is D001's, and this rule stays silent on it.
        let body = "---\nkind: deliverable_package\nlenses:\n  lawyer: [vibe_chart]\n---\n";
        assert!(D002OutOfCatalogSection.lint(&file(body)).is_empty());
    }
}
