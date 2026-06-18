//! Directory walking, file filtering, and rule orchestration.
//!
//! The engine reads markdown files under a directory, applies every
//! configured rule to each one, and returns the aggregated violations.
//! Non-markdown files, `README.md`, `CLAUDE.md`, hidden directories
//! (anything starting with `.`), and `target/` are skipped by default.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::{Rule, SourceFile, Violation};

/// The result of linting a directory: how many files were inspected
/// and every violation produced.
#[derive(Debug, Default)]
pub struct LintReport {
    pub files_scanned: usize,
    pub violations: Vec<Violation>,
}

impl LintReport {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Decides whether a directory or file should be visited.
pub trait FileFilter: Send + Sync {
    fn include_dir(&self, path: &Path) -> bool;
    fn include_file(&self, path: &Path) -> bool;
}

/// The default filter: skip hidden directories (`.git`, `.build`,
/// `.claude`, …), `target/`, and a small allowlist of names that are
/// almost never Navigator notation (`README.md`, `CLAUDE.md`,
/// `CODE_OF_CONDUCT.md`, `LICENSE.md`, `ERD.md`) plus directory
/// subtrees that hold non-notation content (`AgentDocumentation`,
/// `workshops`, `Blog`).
pub struct DefaultFileFilter {
    pub excluded_names: Vec<String>,
    pub excluded_directories: Vec<String>,
}

impl DefaultFileFilter {
    /// File basenames excluded by default.
    pub const DEFAULT_EXCLUDED_FILENAMES: &'static [&'static str] = &[
        "README.md",
        "CLAUDE.md",
        "CODE_OF_CONDUCT.md",
        "LICENSE.md",
        "ERD.md",
    ];

    /// Directory names whose entire subtree is skipped by default.
    pub const DEFAULT_EXCLUDED_DIRECTORIES: &'static [&'static str] =
        &["AgentDocumentation", "workshops", "Blog"];

    /// A filter equivalent to passing `--no-default-excludes`: lint
    /// every `*.md` file under visible directories.
    #[must_use]
    pub fn without_default_excludes() -> Self {
        Self {
            excluded_names: Vec::new(),
            excluded_directories: Vec::new(),
        }
    }
}

impl Default for DefaultFileFilter {
    fn default() -> Self {
        Self {
            excluded_names: Self::DEFAULT_EXCLUDED_FILENAMES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            excluded_directories: Self::DEFAULT_EXCLUDED_DIRECTORIES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }
}

impl FileFilter for DefaultFileFilter {
    fn include_dir(&self, path: &Path) -> bool {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return true;
        };
        if name.starts_with('.') {
            return false;
        }
        if name == "target" {
            return false;
        }
        !self.excluded_directories.iter().any(|n| n == name)
    }

    fn include_file(&self, path: &Path) -> bool {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        let is_md = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if !is_md {
            return false;
        }
        if self.excluded_names.iter().any(|n| n == name) {
            return false;
        }
        // Reject if any ancestor directory matches an excluded name.
        for ancestor in path.components() {
            if let std::path::Component::Normal(seg) = ancestor {
                if let Some(s) = seg.to_str() {
                    if self.excluded_directories.iter().any(|n| n == s) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Orchestrates a set of [`Rule`]s over a directory of markdown files.
pub struct RuleEngine {
    rules: Vec<Box<dyn Rule>>,
    filter: Box<dyn FileFilter>,
}

impl RuleEngine {
    #[must_use]
    pub fn new(rules: Vec<Box<dyn Rule>>) -> Self {
        Self {
            rules,
            filter: Box::new(DefaultFileFilter::default()),
        }
    }

    #[must_use]
    pub fn with_filter(mut self, filter: Box<dyn FileFilter>) -> Self {
        self.filter = filter;
        self
    }

    /// Walk `dir`, lint every included markdown file, and return the
    /// aggregated report. Returns an `io::Error` if the directory
    /// can't be read or a file fails to load.
    pub fn lint_directory(&self, dir: &Path) -> io::Result<LintReport> {
        let mut report = LintReport::default();
        for entry in WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() && e.depth() > 0 {
                    self.filter.include_dir(e.path())
                } else {
                    true
                }
            })
        {
            let entry = entry.map_err(io::Error::other)?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if !self.filter.include_file(path) {
                continue;
            }
            let contents = fs::read_to_string(path)?;
            let file = SourceFile {
                path: PathBuf::from(path),
                contents,
            };
            for rule in &self.rules {
                report.violations.extend(rule.lint(&file));
            }
            report.files_scanned += 1;
        }
        Ok(report)
    }
}

/// The canonical Navigator rule set, in the stable presentation
/// order. `F104` is included with no recognized codes by default —
/// callers that want strict flow-code validation should construct a
/// `RuleEngine` with their own list that supplies
/// `F104FlowQuestionCodes::new(codes)`.
#[must_use]
pub fn navigator_default_rules() -> Vec<Box<dyn Rule>> {
    use crate::{
        F101FrontmatterTitle, F102RespondentType, F103SnakeCaseFilename, F104FlowQuestionCodes,
        F105ConfidentialRequired, F106StaffReviewRequired, F107SignaturePlaceholders,
        M001HeadingIncrement, M003HeadingStyle, M004ULStyle, M005ListIndent, M007ULIndent,
        M009NoTrailingSpaces, M010NoHardTabs, M011NoReversedLinks, M012NoMultipleBlanks,
        M018NoMissingSpaceATX, M019NoMultipleSpaceATX, M020NoMissingSpaceClosedATX,
        M021NoMultipleSpaceClosedATX, M022BlanksAroundHeadings, M023HeadingStartLeft,
        M024NoDuplicateHeading, M026NoTrailingPunctuation, M027NoMultipleSpaceBlockquote,
        M028NoBlanksBlockquote, M029OLPrefix, M030ListMarkerSpace, M031BlanksAroundFences,
        M032BlanksAroundLists, M034NoBareUrls, M035HRStyle, M037NoSpaceInEmphasis,
        M038NoSpaceInCode, M039NoSpaceInLinks, M040FencedCodeLanguage, M042NoEmptyLinks,
        M045NoAltText, M046CodeBlockStyle, M047SingleTrailingNewline, M048CodeFenceStyle,
        M049EmphasisStyle, M050StrongStyle, M051LinkFragments, M052ReferenceLinksImages,
        M053LinkImageReferenceDefinitions, M054LinkImageStyle, M055TablePipeStyle,
        M056TableColumnCount, M058BlanksAroundTables, M059DescriptiveLinkText,
        M060TableColumnStyle, S101LineLength,
    };
    vec![
        Box::new(S101LineLength::default()),
        Box::new(F101FrontmatterTitle),
        Box::new(F102RespondentType),
        Box::new(F103SnakeCaseFilename),
        Box::new(F104FlowQuestionCodes::new(Vec::<String>::new())),
        Box::new(F105ConfidentialRequired),
        Box::new(F106StaffReviewRequired),
        Box::new(F107SignaturePlaceholders),
        Box::new(M001HeadingIncrement),
        Box::new(M003HeadingStyle),
        Box::new(M004ULStyle),
        Box::new(M005ListIndent),
        Box::new(M007ULIndent),
        Box::new(M009NoTrailingSpaces),
        Box::new(M010NoHardTabs),
        Box::new(M011NoReversedLinks),
        Box::new(M012NoMultipleBlanks),
        Box::new(M018NoMissingSpaceATX),
        Box::new(M019NoMultipleSpaceATX),
        Box::new(M020NoMissingSpaceClosedATX),
        Box::new(M021NoMultipleSpaceClosedATX),
        Box::new(M022BlanksAroundHeadings),
        Box::new(M023HeadingStartLeft),
        Box::new(M024NoDuplicateHeading),
        Box::new(M026NoTrailingPunctuation),
        Box::new(M027NoMultipleSpaceBlockquote),
        Box::new(M028NoBlanksBlockquote),
        Box::new(M029OLPrefix),
        Box::new(M030ListMarkerSpace),
        Box::new(M031BlanksAroundFences),
        Box::new(M032BlanksAroundLists),
        Box::new(M034NoBareUrls),
        Box::new(M035HRStyle),
        Box::new(M037NoSpaceInEmphasis),
        Box::new(M038NoSpaceInCode),
        Box::new(M039NoSpaceInLinks),
        Box::new(M040FencedCodeLanguage),
        Box::new(M042NoEmptyLinks),
        Box::new(M045NoAltText),
        Box::new(M046CodeBlockStyle),
        Box::new(M047SingleTrailingNewline),
        Box::new(M048CodeFenceStyle),
        Box::new(M049EmphasisStyle),
        Box::new(M050StrongStyle),
        Box::new(M051LinkFragments),
        Box::new(M052ReferenceLinksImages),
        Box::new(M053LinkImageReferenceDefinitions),
        Box::new(M054LinkImageStyle),
        Box::new(M055TablePipeStyle),
        Box::new(M056TableColumnCount),
        Box::new(M058BlanksAroundTables),
        Box::new(M059DescriptiveLinkText),
        Box::new(M060TableColumnStyle),
    ]
}

/// The Markdown-only subset of [`navigator_default_rules`] — every
/// rule except the F-family, plus `S102` (line-packing). Suitable for
/// linting arbitrary prose markdown (READMEs, blog posts, marketing
/// copy) that doesn't carry the Navigator notation frontmatter and
/// that benefits from being packed tight to the 120-character budget.
///
/// `S102` is markdown-only rather than universal because template
/// fixtures intentionally keep some lines short for readability
/// alongside their structured YAML; only free-form prose should be
/// reflowed to the limit.
#[must_use]
pub fn navigator_markdown_only_rules() -> Vec<Box<dyn Rule>> {
    let mut rules: Vec<Box<dyn Rule>> = navigator_default_rules()
        .into_iter()
        .filter(|r| !r.code().starts_with('F'))
        .collect();
    // Place S102 right after S101 so the two line-length rules sit
    // next to each other.
    let insert_at = rules
        .iter()
        .position(|r| r.code() == "S101")
        .map_or(0, |i| i + 1);
    rules.insert(insert_at, Box::new(crate::S102LinePacking::default()));
    rules
}

#[cfg(test)]
mod tests {
    use super::{navigator_default_rules, DefaultFileFilter, FileFilter, RuleEngine};
    use crate::{F101FrontmatterTitle, F102RespondentType, Rule, S101LineLength};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    /// Minimal three-rule set used by the engine integration tests
    /// that assert specific violation counts. The full default rule
    /// set is exercised by the parity test below.
    fn minimal_engine_rules() -> Vec<Box<dyn Rule>> {
        vec![
            Box::new(S101LineLength::default()),
            Box::new(F101FrontmatterTitle),
            Box::new(F102RespondentType),
        ]
    }

    #[test]
    fn default_filter_includes_markdown_excludes_readme_and_claude() {
        let f = DefaultFileFilter::default();
        assert!(f.include_file(Path::new("foo/bar.md")));
        assert!(!f.include_file(Path::new("foo/README.md")));
        assert!(!f.include_file(Path::new("foo/CLAUDE.md")));
        assert!(!f.include_file(Path::new("foo/notes.txt")));
    }

    #[test]
    fn default_filter_skips_hidden_dirs_and_target() {
        let f = DefaultFileFilter::default();
        assert!(!f.include_dir(Path::new("foo/.git")));
        assert!(!f.include_dir(Path::new("foo/.claude")));
        assert!(!f.include_dir(Path::new("foo/target")));
        assert!(f.include_dir(Path::new("foo/src")));
    }

    #[test]
    fn engine_returns_empty_report_for_directory_with_no_markdown() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "notes.txt", "not markdown");
        let report = RuleEngine::new(minimal_engine_rules())
            .lint_directory(dir.path())
            .unwrap();
        assert_eq!(report.files_scanned, 0);
        assert!(report.is_clean());
    }

    #[test]
    fn engine_lints_valid_file_with_no_violations() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "trust.md",
            "---\ntitle: Trust\nrespondent_type: entity\n---\n\nBody.",
        );
        let report = RuleEngine::new(minimal_engine_rules())
            .lint_directory(dir.path())
            .unwrap();
        assert_eq!(report.files_scanned, 1);
        assert!(report.is_clean(), "{:?}", report.violations);
    }

    #[test]
    fn engine_aggregates_violations_across_rules_and_files() {
        let dir = TempDir::new().unwrap();
        // File 1: line too long AND missing respondent_type.
        write(
            dir.path(),
            "a.md",
            &format!("---\ntitle: A\n---\n\n{}", "x".repeat(121)),
        );
        // File 2: missing title.
        write(
            dir.path(),
            "sub/b.md",
            "---\nrespondent_type: person\n---\n",
        );
        // File 3: valid — should produce no violations.
        write(
            dir.path(),
            "c.md",
            "---\ntitle: C\nrespondent_type: person_and_entity\n---\n",
        );
        let report = RuleEngine::new(minimal_engine_rules())
            .lint_directory(dir.path())
            .unwrap();
        assert_eq!(report.files_scanned, 3);
        let codes: Vec<&str> = report.violations.iter().map(|v| v.code).collect();
        assert!(codes.contains(&"S101"));
        assert!(codes.contains(&"F101"));
        assert!(codes.contains(&"F102"));
        // No false positives from c.md.
        assert_eq!(report.violations.len(), 3);
    }

    /// Stable presentation order of the default rule set. Embedded
    /// literally so this test fails loudly if a future change
    /// silently reorders or drops a rule.
    const EXPECTED_DEFAULT_RULE_CODES: &[&str] = &[
        "S101", "F101", "F102", "F103", "F104", "F105", "F106", "F107", "M001", "M003", "M004",
        "M005", "M007", "M009", "M010", "M011", "M012", "M018", "M019", "M020", "M021", "M022",
        "M023", "M024", "M026", "M027", "M028", "M029", "M030", "M031", "M032", "M034", "M035",
        "M037", "M038", "M039", "M040", "M042", "M045", "M046", "M047", "M048", "M049", "M050",
        "M051", "M052", "M053", "M054", "M055", "M056", "M058", "M059", "M060",
    ];

    #[test]
    fn navigator_default_rule_codes_are_stable() {
        let actual_codes: Vec<&'static str> =
            navigator_default_rules().iter().map(|r| r.code()).collect();
        assert_eq!(
            actual_codes, EXPECTED_DEFAULT_RULE_CODES,
            "default rule set order drifted; update EXPECTED_DEFAULT_RULE_CODES intentionally if this was on purpose"
        );
    }

    #[test]
    fn navigator_markdown_only_rules_drop_f_family_and_add_s102() {
        use super::navigator_markdown_only_rules;
        let codes: Vec<&'static str> = navigator_markdown_only_rules()
            .iter()
            .map(|r| r.code())
            .collect();
        assert!(codes.iter().all(|c| !c.starts_with('F')));
        // S102 sits right after S101.
        let mut expected: Vec<&str> = EXPECTED_DEFAULT_RULE_CODES
            .iter()
            .copied()
            .filter(|c| !c.starts_with('F'))
            .collect();
        let pos = expected.iter().position(|c| *c == "S101").unwrap() + 1;
        expected.insert(pos, "S102");
        assert_eq!(codes, expected);
    }

    #[test]
    fn engine_skips_readme_claude_and_hidden_dirs() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "README.md", &"x".repeat(200));
        write(dir.path(), "CLAUDE.md", &"x".repeat(200));
        write(dir.path(), ".hidden/inside.md", &"x".repeat(200));
        write(dir.path(), "target/build.md", &"x".repeat(200));
        write(
            dir.path(),
            "good.md",
            "---\ntitle: Good\nrespondent_type: entity\n---\n",
        );
        let report = RuleEngine::new(minimal_engine_rules())
            .lint_directory(dir.path())
            .unwrap();
        assert_eq!(report.files_scanned, 1);
        assert!(report.is_clean());
    }
}
