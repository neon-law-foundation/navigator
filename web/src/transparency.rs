//! `/foundation/transparency` — the Foundation's public-disclosure page.
//!
//! A 501(c)(3) must make three things available for public inspection under
//! IRC §6104(d): its exemption application and supporting documents, the IRS
//! determination letter, and its three most recent annual returns (Form
//! 990-series). The Foundation publishes those here, and — going beyond what
//! the law requires — also publishes its governance documents (bylaws, the
//! conflict of interest policy) and its quarterly board minutes.
//!
//! The determination-letter PDF is served straight from `web/public/`; this
//! module loads the *narrative* documents from `web/content/foundation/`:
//!
//! - One markdown file per governance document at the top level
//!   (`bylaws.md`, `conflict_of_interest.md`) → [`DocCategory::Governance`].
//! - One markdown file per quarter under `minutes/`, named `YYYY-qN.md`
//!   (`2021-q1.md`) or `YYQN_minutes.md` (`26Q2_minutes.md`) →
//!   [`DocCategory::Minutes`], served under `/foundation/transparency/minutes/`.
//!
//! Front-matter (`title`, `description`) and the markdown body are parsed by
//! the shared [`marketing::loader`], so a document file is shaped exactly like
//! a marketing fragment — only the directory layout, not the file, carries the
//! category.

use std::path::Path;
use std::sync::Arc;

use walkdir::WalkDir;

use crate::content_loader::ContentLoadError;
use crate::marketing;

/// File basenames inside the foundation tree that are NOT documents.
const NON_DOC_FILES: &[&str] = &["README.md", ".gitkeep"];

/// Which section of the transparency page a document belongs to. Derived
/// from the directory the file lives in, not from front-matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocCategory {
    /// A governance document at the top of the tree (`bylaws.md`,
    /// `conflict_of_interest.md`).
    Governance,
    /// A quarterly board-minutes file under `minutes/`.
    Minutes,
}

/// One published transparency document, built from a markdown file's
/// front-matter plus body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparencyDoc {
    /// Routing key. Governance docs use the kebab-cased file stem
    /// (`conflict-of-interest`); minutes use the compact quarter key (`26q2`).
    pub slug: String,
    /// Canonical path for this document.
    pub path: String,
    /// Document title (front-matter `title`).
    pub title: String,
    /// One-line summary (front-matter `description`); used for the index
    /// blurb and the per-document `<meta description>`.
    pub description: String,
    /// Which section the document appears under.
    pub category: DocCategory,
    /// Sort key within a category. Governance: a small priority (bylaws
    /// before the conflict policy). Minutes: `year * 10 + quarter`, so the
    /// minutes accessor can order them newest-first.
    pub sort_key: u32,
    /// Rendered HTML body (NOT raw markdown).
    pub body_html: String,
}

/// `Arc`-wrapped lookup shared as router state. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct TransparencyIndex {
    docs: Arc<Vec<TransparencyDoc>>,
}

impl TransparencyIndex {
    #[must_use]
    pub fn new(docs: Vec<TransparencyDoc>) -> Self {
        Self {
            docs: Arc::new(docs),
        }
    }

    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Governance documents (bylaws, conflict policy), ordered by priority.
    #[must_use]
    pub fn governance(&self) -> Vec<&TransparencyDoc> {
        let mut v: Vec<&TransparencyDoc> = self
            .docs
            .iter()
            .filter(|d| d.category == DocCategory::Governance)
            .collect();
        v.sort_by(|a, b| {
            a.sort_key
                .cmp(&b.sort_key)
                .then_with(|| a.slug.cmp(&b.slug))
        });
        v
    }

    /// Quarterly board minutes, newest first.
    #[must_use]
    pub fn minutes(&self) -> Vec<&TransparencyDoc> {
        let mut v: Vec<&TransparencyDoc> = self
            .docs
            .iter()
            .filter(|d| d.category == DocCategory::Minutes)
            .collect();
        // Descending sort key → newest quarter first; ties break on slug for
        // a deterministic order in tests.
        v.sort_by(|a, b| {
            b.sort_key
                .cmp(&a.sort_key)
                .then_with(|| a.slug.cmp(&b.slug))
        });
        v
    }

    /// Look up one document by its slug.
    #[must_use]
    pub fn get(&self, slug: &str) -> Option<&TransparencyDoc> {
        self.docs.iter().find(|d| d.slug == slug)
    }

    /// `true` when no documents are loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

/// Priority for a governance slug — bylaws lead, the conflict policy follows,
/// anything else sorts after both (then alphabetically).
fn governance_priority(slug: &str) -> u32 {
    match slug {
        "bylaws" => 0,
        "conflict-of-interest" => 1,
        _ => 2,
    }
}

fn parse_legacy_minutes_stem(stem: &str) -> Option<(String, u32)> {
    let (year_part, quarter_part) = stem.split_once('-')?;
    let year: u32 = year_part.parse().ok()?;
    let quarter: u32 = quarter_part.strip_prefix('q')?.parse().ok()?;
    if !(1..=4).contains(&quarter) {
        return None;
    }
    Some((
        format!("{:02}q{quarter}", year.checked_sub(2000)?),
        year * 10 + quarter,
    ))
}

fn parse_compact_minutes_stem(stem: &str) -> Option<(String, u32)> {
    let (key, suffix) = stem.split_once('_')?;
    if suffix != "minutes" {
        return None;
    }
    let mut chars = key.chars();
    let y1 = chars.next()?.to_digit(10)?;
    let y2 = chars.next()?.to_digit(10)?;
    if !chars.next()?.eq_ignore_ascii_case(&'q') {
        return None;
    }
    let quarter = chars.next()?.to_digit(10)?;
    if chars.next().is_some() || !(1..=4).contains(&quarter) {
        return None;
    }
    let year = 2000 + y1 * 10 + y2;
    Some((format!("{:02}q{quarter}", year - 2000), year * 10 + quarter))
}

/// Parse a minutes file stem into `(route slug, sort key)`. Supports the
/// existing `YYYY-qN` files and the compact `YYQN_minutes` filename used for
/// newly approved minutes.
fn parse_minutes_stem(stem: &str) -> Option<(String, u32)> {
    parse_compact_minutes_stem(stem).or_else(|| parse_legacy_minutes_stem(stem))
}

/// Walk `dir` for transparency documents. Returns an empty index (not an
/// error) when `dir` doesn't exist, so a fork with no foundation content yet
/// boots cleanly.
pub fn load_dir(dir: &Path) -> Result<TransparencyIndex, ContentLoadError> {
    let mut docs = Vec::new();
    if !dir.exists() {
        return Ok(TransparencyIndex::empty());
    }
    for entry in WalkDir::new(dir).follow_links(false) {
        let entry = entry.map_err(|e| ContentLoadError::Io {
            path: dir.display().to_string(),
            source: std::io::Error::other(e),
        })?;
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if NON_DOC_FILES.contains(&name) {
            continue;
        }
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let in_minutes = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("minutes");

        let (slug, canonical_path, category, sort_key) = if in_minutes {
            let Some((slug, sort_key)) = parse_minutes_stem(stem) else {
                tracing::warn!(
                    file = name,
                    "skipping minutes file: name is not YYYY-qN.md or YYQN_minutes.md"
                );
                continue;
            };
            (
                slug.clone(),
                format!("/foundation/transparency/minutes/{slug}"),
                DocCategory::Minutes,
                sort_key,
            )
        } else {
            let slug = views::slug::to_url(stem);
            let priority = governance_priority(&slug);
            (
                slug.clone(),
                format!("/foundation/transparency/{slug}"),
                DocCategory::Governance,
                priority,
            )
        };

        let raw = std::fs::read_to_string(path).map_err(|e| ContentLoadError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        let doc =
            marketing::loader::parse(&raw, &slug).ok_or(ContentLoadError::MissingFrontmatter {
                path: path.display().to_string(),
            })?;
        docs.push(TransparencyDoc {
            slug,
            path: canonical_path,
            title: doc.title,
            description: doc.description,
            category,
            sort_key,
            body_html: doc.body_html,
        });
    }
    Ok(TransparencyIndex::new(docs))
}

#[cfg(test)]
mod tests {
    use super::{load_dir, parse_minutes_stem, DocCategory, TransparencyIndex};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn empty_index_is_empty() {
        let ix = TransparencyIndex::empty();
        assert!(ix.is_empty());
        assert!(ix.governance().is_empty());
        assert!(ix.minutes().is_empty());
        assert!(ix.get("anything").is_none());
    }

    #[test]
    fn parses_minutes_stem_into_sort_key() {
        assert_eq!(
            parse_minutes_stem("2021-q1"),
            Some(("21q1".to_string(), 20211))
        );
        assert_eq!(
            parse_minutes_stem("2026-q2"),
            Some(("26q2".to_string(), 20262))
        );
        assert_eq!(
            parse_minutes_stem("26Q2_minutes"),
            Some(("26q2".to_string(), 20262))
        );
        assert!(parse_minutes_stem("2021-q5").is_none());
        assert!(parse_minutes_stem("2021-q0").is_none());
        assert!(parse_minutes_stem("26Q5_minutes").is_none());
        assert!(parse_minutes_stem("notaquarter").is_none());
        assert!(parse_minutes_stem("2021-x1").is_none());
    }

    #[test]
    fn bundled_foundation_directory_loads_cleanly() {
        // Guards the real `web/content/foundation/` tree and documents the
        // authoring contract by example: top-level files are governance docs,
        // files under `minutes/` are quarterly board minutes served under
        // `/foundation/transparency/minutes/`.
        let ix = load_dir(std::path::Path::new(crate::DEFAULT_FOUNDATION_DIR)).unwrap();
        let bylaws = ix.get("bylaws").expect("bylaws governance doc loads");
        assert_eq!(bylaws.category, DocCategory::Governance);
        assert_eq!(bylaws.title, "Bylaws");
        assert!(!bylaws.body_html.is_empty());

        assert!(ix.get("conflict-of-interest").is_some());

        let q1_2021 = ix
            .get("21q1")
            .expect("first quarter of minutes loads at 21q1");
        assert_eq!(q1_2021.category, DocCategory::Minutes);

        // Twenty-two quarters, Q1 2021 through Q2 2026, newest first.
        let minutes = ix.minutes();
        assert_eq!(minutes.len(), 22);
        assert_eq!(minutes.first().unwrap().slug, "26q2");
        assert_eq!(
            minutes.first().unwrap().path,
            "/foundation/transparency/minutes/26q2"
        );
        assert_eq!(minutes.last().unwrap().slug, "21q1");

        // Governance order: bylaws before the conflict policy.
        let gov = ix.governance();
        assert_eq!(gov.first().unwrap().slug, "bylaws");
        assert_eq!(gov[1].slug, "conflict-of-interest");
    }

    #[test]
    fn load_dir_returns_empty_index_when_directory_missing() {
        let ix = load_dir(std::path::Path::new("/no/such/foundation/dir/xyz")).unwrap();
        assert!(ix.is_empty());
    }

    #[test]
    fn load_dir_categorizes_by_directory_and_orders_minutes_newest_first() {
        let tmp = TempDir::new().unwrap();
        let doc = |title: &str, body: &str| format!("---\ntitle: {title}\n---\n{body}\n");
        fs::write(tmp.path().join("bylaws.md"), doc("Bylaws", "gov body")).unwrap();
        fs::create_dir(tmp.path().join("minutes")).unwrap();
        fs::write(
            tmp.path().join("minutes/2021-q1.md"),
            doc("Q1 2021", "older"),
        )
        .unwrap();
        fs::write(
            tmp.path().join("minutes/26Q2_minutes.md"),
            doc("Q2 2026", "newer"),
        )
        .unwrap();
        // A README and a malformed minutes name are both skipped.
        fs::write(tmp.path().join("README.md"), "# not a doc\n").unwrap();
        fs::write(tmp.path().join("minutes/notes.md"), doc("Bad", "x")).unwrap();

        let ix = load_dir(tmp.path()).unwrap();
        let gov: Vec<&str> = ix.governance().iter().map(|d| d.slug.as_str()).collect();
        assert_eq!(gov, vec!["bylaws"]);
        let minutes: Vec<&str> = ix.minutes().iter().map(|d| d.slug.as_str()).collect();
        assert_eq!(minutes, vec!["26q2", "21q1"]);
        assert!(ix.get("notes").is_none());
    }
}
