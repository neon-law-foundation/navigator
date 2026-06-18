//! Load the baked-in workshop manifest.
//!
//! There is one canonical workshop on the public surface today —
//! "Using the Navigator to Rapidly Solve Legal Outcomes" — but the
//! `WorkshopIndex` is intentionally kept general (Capricorn's call
//! in the engineer council: in two years there will be ten
//! workshops, not one).

use std::fs;
use std::io;
use std::path::Path;

use pulldown_cmark::{html, Options, Parser};

use super::{WorkshopMaterial, WorkshopSection};
use crate::content_loader::ContentLoadError;

struct ManifestEntry {
    slug: &'static str,
    title: &'static str,
    description: &'static str,
    filename: &'static str,
}

/// Subdirectory under the workshops content root where the
/// Navigator workshop's materials live.
const NAVIGATOR_FOLDER: &str = "navigator";

const NAVIGATOR_MANIFEST: &[ManifestEntry] = &[
    ManifestEntry {
        slug: "readme",
        title: "Using the Navigator to Rapidly Solve Legal Outcomes",
        description: "A single hands-on workshop for attorneys. Build a deed-of-sale notation \
                      with a notarization step using Gemini's Add AIDA connector — no command \
                      line, no software install. Walk out with a three-minute demo of a \
                      contract your lawyer-self would sign on Monday.",
        filename: "README.md",
    },
    ManifestEntry {
        slug: "deploy",
        title: "Deploy the Navigator",
        description: "Stand up your own Navigator instance on a custom Google Cloud project. Six \
                      grounded steps walk `navigator gcp setup` — APIs, VPC, Cloud SQL, three buckets, \
                      and a GKE Autopilot cluster — with a dry-run that shows every API call \
                      before a packet leaves your laptop.",
        filename: "DEPLOY.md",
    },
];

/// Load every manifest entry for the Navigator workshop. Missing
/// files are silently skipped so a partial install still boots; the
/// index page drops cards for materials it couldn't find.
pub fn load_navigator(content_root: &Path) -> Result<Vec<WorkshopMaterial>, ContentLoadError> {
    let folder = content_root.join(NAVIGATOR_FOLDER);
    let mut materials = Vec::new();
    for entry in NAVIGATOR_MANIFEST {
        let path = folder.join(entry.filename);
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(ContentLoadError::Io {
                    path: path.display().to_string(),
                    source: err,
                });
            }
        };
        materials.push(material_from_markdown(
            entry.slug,
            entry.title,
            entry.description,
            &raw,
        ));
    }
    Ok(materials)
}

/// Parse one stepped-content document into a [`WorkshopMaterial`]:
/// split it on `##` headings, render each section to HTML, and keep
/// the raw markdown for the copy-to-clipboard button.
///
/// Shared by the workshop loader (which feeds it manifest-declared
/// files from disk) and the `presentations` module (which feeds it a
/// single `include_str!`-baked talk). The title and description come
/// from the caller, not the markdown, so both surfaces control their
/// own chrome.
pub(crate) fn material_from_markdown(
    slug: &str,
    title: &str,
    description: &str,
    raw: &str,
) -> WorkshopMaterial {
    let (intro_md, section_specs) = split_sections(raw);
    let sections = section_specs
        .into_iter()
        .map(|(title, body_md)| WorkshopSection {
            title,
            body_html: render_markdown(&body_md),
        })
        .collect();
    WorkshopMaterial {
        slug: slug.to_string(),
        title: title.to_string(),
        description: description.to_string(),
        // The page chrome owns the sole `<h1>`; strip the leading
        // `#` title so the rendered body doesn't repeat it.
        body_html: render_markdown(&strip_leading_h1(raw)),
        intro_html: render_markdown(&intro_md),
        sections,
        raw_markdown: raw.to_string(),
    }
}

fn render_markdown(src: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_FOOTNOTES);
    let parser = Parser::new_ext(src, opts);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// Drop a single leading top-level (`# `) heading so the rendered body
/// does not duplicate the title the page chrome already renders as the
/// document's `<h1>`. Only the *first* such line, and only before any
/// content, is removed; `## ` and deeper headings are untouched.
fn strip_leading_h1(src: &str) -> String {
    let trimmed = src.trim_start();
    match trimmed.strip_prefix("# ") {
        Some(after_hash) => {
            // Drop the rest of the title line and any blank lines that
            // follow it, keeping the body verbatim.
            let body = after_hash.split_once('\n').map_or("", |(_, rest)| rest);
            body.trim_start().to_string()
        }
        // No leading H1 — return the source untouched.
        None => src.to_string(),
    }
}

/// True for an ATX `# ` heading but not `## ` or deeper.
fn is_h1(line: &str) -> bool {
    line.starts_with("# ")
}

/// Split workshop markdown into `(intro, sections)`. The intro is
/// everything before the first `##` heading (with the leading `#`
/// title stripped); each section is the `(heading_text, markdown)` of
/// one `##` block, heading line included. Lines inside fenced code
/// blocks never start a section, so a `##` comment in a code sample
/// can't split the document.
fn split_sections(src: &str) -> (String, Vec<(String, String)>) {
    let mut intro: Vec<&str> = Vec::new();
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    let mut in_fence = false;
    let mut title_stripped = false;

    for line in src.lines() {
        if is_fence(line) {
            in_fence = !in_fence;
        }

        if !in_fence {
            if let Some(heading) = line.strip_prefix("## ") {
                if let Some((title, body)) = current.take() {
                    sections.push((title, body.join("\n")));
                }
                current = Some((heading.trim().to_string(), vec![line]));
                continue;
            }
        }

        if let Some((_, body)) = current.as_mut() {
            body.push(line);
        } else if !title_stripped && intro.is_empty() && line.trim().is_empty() {
            // Skip blank lines before the title.
        } else if !title_stripped && is_h1(line) {
            // Drop the leading title — the page chrome renders it.
            title_stripped = true;
        } else {
            intro.push(line);
        }
    }
    if let Some((title, body)) = current.take() {
        sections.push((title, body.join("\n")));
    }
    (intro.join("\n"), sections)
}

/// True for a ```` ``` ```` or `~~~` fence marker (any indentation).
fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

#[cfg(test)]
mod tests {
    use super::{load_navigator, split_sections, strip_leading_h1};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn load_navigator_returns_empty_when_directory_missing() {
        let materials = load_navigator(std::path::Path::new("/no/such/dir/12345")).unwrap();
        assert!(materials.is_empty());
    }

    #[test]
    fn load_navigator_returns_the_single_canonical_workshop() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("navigator");
        fs::create_dir_all(&target).unwrap();
        fs::write(
            target.join("README.md"),
            "# Runbook\n\nWelcome to Navigator.\n",
        )
        .unwrap();
        let materials = load_navigator(dir.path()).unwrap();
        assert_eq!(materials.len(), 1, "exactly one workshop on the surface");
        assert_eq!(materials[0].slug, "readme");
        assert_eq!(
            materials[0].title,
            "Using the Navigator to Rapidly Solve Legal Outcomes",
        );
    }

    #[test]
    fn rendered_body_drops_the_leading_title_h1() {
        // The page chrome renders the workshop title as the document's
        // sole <h1>; the markdown body must not repeat it (the bug:
        // two identical <h1>s on /…/readme).
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("navigator");
        fs::create_dir_all(&target).unwrap();
        fs::write(
            target.join("README.md"),
            "# Runbook\n\nWelcome.\n\n## First step\n\nDo the thing.\n",
        )
        .unwrap();
        let materials = load_navigator(dir.path()).unwrap();
        assert!(
            !materials[0].body_html.contains("<h1>"),
            "rendered body must carry no <h1>, got: {}",
            materials[0].body_html
        );
        // …but the raw markdown the copy button hands back keeps the
        // title so the downloaded file is self-describing.
        assert!(materials[0].raw_markdown.starts_with("# Runbook"));
    }

    #[test]
    fn workshop_splits_into_ordered_sections_with_intro() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("navigator");
        fs::create_dir_all(&target).unwrap();
        fs::write(
            target.join("README.md"),
            "# Title\n\nOrientation lede.\n\n## Step one\n\nAlpha.\n\n## Step two\n\nBeta.\n",
        )
        .unwrap();
        let m = &load_navigator(dir.path()).unwrap()[0];
        assert!(
            m.intro_html.contains("Orientation lede") && !m.intro_html.contains("<h2"),
            "intro is the pre-heading lede, got: {}",
            m.intro_html
        );
        assert_eq!(m.sections.len(), 2);
        assert_eq!(m.sections[0].title, "Step one");
        assert_eq!(m.sections[1].title, "Step two");
        assert!(m.sections[0].body_html.contains("<h2>Step one</h2>"));
        assert!(m.sections[0].body_html.contains("Alpha"));
        assert!(!m.sections[0].body_html.contains("Beta"));
    }

    #[test]
    fn strip_leading_h1_only_touches_the_first_top_level_heading() {
        assert_eq!(strip_leading_h1("# Title\n\nBody"), "Body");
        assert_eq!(strip_leading_h1("\n\n# Title\nBody"), "Body");
        // `##` is a section heading, not the title — leave it.
        assert_eq!(strip_leading_h1("## Section\nBody"), "## Section\nBody");
        // No leading H1 at all → unchanged.
        assert_eq!(strip_leading_h1("Just text"), "Just text");
    }

    #[test]
    fn split_sections_ignores_headings_inside_code_fences() {
        // A `##`-prefixed line inside a fenced code block is sample
        // text, not a step boundary.
        let (_intro, sections) =
            split_sections("# T\n\n## Real\n\n```\n## not a heading\n```\n\nEnd.\n");
        assert_eq!(sections.len(), 1, "only the real ## heading splits");
        assert_eq!(sections[0].0, "Real");
        assert!(sections[0].1.contains("## not a heading"));
    }

    #[test]
    fn load_navigator_skips_missing_files_without_error() {
        let dir = TempDir::new().unwrap();
        // Folder exists but README absent — the manifest entry is
        // silently dropped, no error returned.
        fs::create_dir_all(dir.path().join("navigator")).unwrap();
        let materials = load_navigator(dir.path()).unwrap();
        assert!(materials.is_empty());
    }
}
