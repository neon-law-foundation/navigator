//! The curated, public template gallery served at `/templates`.
//!
//! A visitor browses a small, **client-safe** subset of the workspace
//! `templates/` tree and downloads the raw `.md` — the same bytes a git
//! reader sees, so the markdown notation format speaks for itself. This
//! reuses the [`crate::docs`] "bake the file in, serve it verbatim"
//! shape; it does not invent a second file streamer.
//!
//! The allow-list is **explicit and owned**. Only the entries in
//! [`MANIFEST`] are reachable: a template not on the list 404s rather
//! than being guessed into existence, so internal or `confidential:
//! true` templates can never leak through a path. The list leads with
//! the federal, jurisdiction-neutral Form 990 and labels the
//! Nevada-specific filings loudly — we never imply coverage outside the
//! firm's bar admissions.
//!
//! [`tests::every_listed_template_is_non_confidential`] is the
//! load-bearing invariant: a `confidential: true` template added to the
//! manifest by mistake fails the build, not production.

use std::sync::LazyLock;

/// Jurisdiction a template is written for. Rendered as a loud badge so a
/// visitor never mistakes a Nevada filing for their own state's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jurisdiction {
    /// Federal / United States — jurisdiction-neutral (e.g. an IRS form).
    Federal,
    /// State of Nevada.
    Nevada,
}

impl Jurisdiction {
    /// Loud, human label for the badge.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Jurisdiction::Federal => "Federal · United States",
            Jurisdiction::Nevada => "Nevada",
        }
    }

    /// Bootstrap badge class. Federal reads neutral; a state-specific
    /// filing reads as a caution so nobody assumes nationwide reach.
    #[must_use]
    pub fn badge_class(self) -> &'static str {
        match self {
            Jurisdiction::Federal => "bg-primary",
            Jurisdiction::Nevada => "bg-warning text-dark",
        }
    }
}

/// A hand-curated allow-list row. `title` + `confidential` are NOT here
/// — they are parsed from the file's own frontmatter at load so the page
/// can never drift from the template source.
struct ManifestEntry {
    category: &'static str,
    name: &'static str,
    blurb: &'static str,
    jurisdiction: Jurisdiction,
    raw: &'static str,
}

/// `include_str!` a template by `category/name`, resolved from the
/// `web` crate manifest dir (the `templates/` tree is one level up).
macro_rules! template {
    ($category:literal, $name:literal, $jurisdiction:expr, $blurb:literal) => {
        ManifestEntry {
            category: $category,
            name: $name,
            blurb: $blurb,
            jurisdiction: $jurisdiction,
            raw: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../templates/",
                $category,
                "/",
                $name,
                ".md"
            )),
        }
    };
}

/// The curated, client-safe allow-list. Leads with the federal Form 990
/// (jurisdiction-neutral — the safest artifact to hand any nonprofit),
/// then the two Nevada-specific nonprofit filings, each loudly labeled.
const MANIFEST: &[ManifestEntry] = &[
    template!(
        "nonprofit",
        "form990_annual_report",
        Jurisdiction::Federal,
        "The annual information return every tax-exempt organization files \
         with the IRS (IRC §6033) — the year's revenue, governance, and \
         program work. Federal: the same form wherever your nonprofit is \
         incorporated."
    ),
    template!(
        "nonprofit",
        "nevada_501c3_formation",
        Jurisdiction::Nevada,
        "Articles of incorporation that form a Nevada nonprofit and set it up \
         to seek 501(c)(3) status — mission, founding board, and registered \
         agent. Written for Nevada filings."
    ),
    template!(
        "nonprofit",
        "nevada_charitable_solicitation_registration",
        Jurisdiction::Nevada,
        "The registration a charity files with the Nevada Secretary of State \
         before soliciting donations in the state. Written for Nevada; other \
         states run their own registries."
    ),
];

/// One gallery entry as served: the curated manifest fields plus the
/// `title` and `confidential` flag parsed from the template's
/// frontmatter at load.
pub struct GalleryTemplate {
    /// Category folder (`nonprofit`).
    pub category: &'static str,
    /// File stem (`form990_annual_report`).
    pub name: &'static str,
    /// Human title, parsed from the template's frontmatter `title`.
    pub title: String,
    /// Plain-language "what it's for".
    pub blurb: &'static str,
    /// The jurisdiction the template targets.
    pub jurisdiction: Jurisdiction,
    /// The full raw template file — served verbatim on download.
    pub raw: &'static str,
    /// Parsed `confidential` flag; the invariant test asserts it false.
    confidential: bool,
}

impl GalleryTemplate {
    /// Download filename, e.g. `form990_annual_report.md`.
    #[must_use]
    pub fn download_filename(&self) -> String {
        format!("{}.md", self.name)
    }

    /// The inner YAML of the leading `---` frontmatter block (no fences),
    /// shown verbatim so the visitor sees the notation contract itself.
    #[must_use]
    pub fn frontmatter(&self) -> &'static str {
        frontmatter_block(self.raw)
    }

    /// Whether this entry is flagged confidential. Always false for a
    /// served entry (the invariant test enforces it); exposed for tests.
    #[must_use]
    pub fn is_confidential(&self) -> bool {
        self.confidential
    }
}

/// The frontmatter shape we read off each template. Only the two fields
/// the gallery needs; the N-rules validate the rest.
#[derive(serde::Deserialize)]
struct Frontmatter {
    title: String,
    #[serde(default)]
    confidential: bool,
}

/// Slice the inner YAML of a leading `---` … `---` frontmatter block.
/// The returned slice keeps the input's lifetime, so a `'static` `raw`
/// (the baked manifest content) yields a `'static` block. Returns `""`
/// for a file without a frontmatter fence — which then fails to parse, a
/// loud build failure rather than a silent empty page.
fn frontmatter_block(raw: &str) -> &str {
    let after = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
        .unwrap_or("");
    match after.find("\n---") {
        Some(end) => &after[..end],
        None => after,
    }
}

fn parse_frontmatter(raw: &str) -> Frontmatter {
    serde_yaml::from_str(frontmatter_block(raw))
        .expect("gallery template must have valid `title` + `confidential` frontmatter")
}

/// The loaded gallery, parsed once. Empty manifests are impossible (it's
/// a `const` list), so this is never empty in practice.
static GALLERY: LazyLock<Vec<GalleryTemplate>> = LazyLock::new(|| {
    MANIFEST
        .iter()
        .map(|entry| {
            let fm = parse_frontmatter(entry.raw);
            GalleryTemplate {
                category: entry.category,
                name: entry.name,
                title: fm.title,
                blurb: entry.blurb,
                jurisdiction: entry.jurisdiction,
                raw: entry.raw,
                confidential: fm.confidential,
            }
        })
        .collect()
});

/// Every curated template, in manifest order (Form 990 leads).
#[must_use]
pub fn gallery() -> &'static [GalleryTemplate] {
    &GALLERY
}

/// Look up one allow-listed template. `None` — and therefore a 404 at
/// the route — for anything not on the curated list.
#[must_use]
pub fn find(category: &str, name: &str) -> Option<&'static GalleryTemplate> {
    GALLERY
        .iter()
        .find(|t| t.category == category && t.name == name)
}

#[cfg(test)]
mod tests {
    use super::{find, frontmatter_block, gallery, Jurisdiction};

    #[test]
    fn every_listed_template_is_non_confidential() {
        // The load-bearing guardrail: a `confidential: true` template can
        // never reach the public gallery. If this fails, a confidential
        // template was added to the manifest — remove it.
        for t in gallery() {
            assert!(
                !t.is_confidential(),
                "{}/{} is confidential: true and must not be publicly downloadable",
                t.category,
                t.name
            );
        }
    }

    #[test]
    fn gallery_leads_with_the_federal_form_990() {
        let first = &gallery()[0];
        assert_eq!(first.name, "form990_annual_report");
        assert_eq!(first.jurisdiction, Jurisdiction::Federal);
    }

    #[test]
    fn title_is_parsed_from_each_template_frontmatter() {
        let t = find("nonprofit", "form990_annual_report").expect("listed");
        assert!(t.title.contains("Form 990"), "got title {:?}", t.title);
    }

    #[test]
    fn confidential_templates_are_not_reachable() {
        // Retainer + Closing Letter are `confidential: true`; they must
        // never be on the list and so must 404 by being absent.
        assert!(find("onboarding", "Retainer").is_none());
        assert!(find("closing", "Letter").is_none());
        // A guessed/typo'd path is also absent.
        assert!(find("nonprofit", "DoesNotExist").is_none());
    }

    #[test]
    fn frontmatter_block_is_the_yaml_not_the_body() {
        let raw = "---\ntitle: X\ncode: y\n---\n\n# Heading\n\nbody\n";
        let block = frontmatter_block(raw);
        assert!(block.contains("code: y"));
        assert!(!block.contains("# Heading"));
    }

    #[test]
    fn served_frontmatter_excludes_the_template_body() {
        let t = find("nonprofit", "form990_annual_report").unwrap();
        let fm = t.frontmatter();
        assert!(fm.contains("code: form_990__annual_report"));
        assert!(!fm.contains("# IRS Form 990"));
    }
}
