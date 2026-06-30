//! Load the baked-in workshop manifest.
//!
//! Nebula groups public sharing materials — workshops, presentations,
//! and show-and-tells — while this loader keeps the authored markdown
//! manifest stable.

use std::fs;
use std::io;
use std::path::Path;

use pulldown_cmark::{html, Options, Parser};

use super::{WorkshopMaterial, WorkshopSection};
use crate::content_loader::ContentLoadError;

struct ManifestEntry {
    category: &'static str,
    slug: &'static str,
    title: &'static str,
    description: &'static str,
    /// Who the material is for, shown as the audience tag on the
    /// `/foundation/nebula` overview so a reader self-selects fast.
    audience: &'static str,
    /// The you-voiced takeaway shown as the overview card body —
    /// what the reader walks out with, never a guaranteed outcome.
    benefit: &'static str,
    filename: &'static str,
}

/// Subdirectory under the workshops content root where the
/// Neon Law Navigator workshop's materials live.
const NAVIGATOR_FOLDER: &str = "navigator";

const NAVIGATOR_MANIFEST: &[ManifestEntry] = &[
    ManifestEntry {
        category: "workshops",
        slug: "use-the-navigator",
        title: "Using Neon Law Navigator",
        description: "A single hands-on workshop for attorneys. Build a deed-of-sale notation \
                      with a notarization step using Gemini's Add AIDA connector — no command \
                      line, no software install. Walk out with a three-minute demo of a \
                      contract your lawyer-self would sign on Monday.",
        audience: "For lawyers",
        benefit: "You walk out with a deed-of-sale notation you built yourself and a \
                  three-minute demo you can run at your firm — the checks you already know to \
                  run, like choice of law, privilege, and confidentiality, applied before you \
                  sign. It runs inside the Gemini workspace you already use: no install, no \
                  command line.",
        filename: "README.md",
    },
    ManifestEntry {
        category: "workshops",
        slug: "deploy-the-navigator",
        title: "Deploying Neon Law Navigator",
        description: "Stand up your own Neon Law Navigator instance on a custom Google Cloud project. Six \
                      grounded steps walk `navigator gcp setup` — APIs, VPC, Cloud SQL, three buckets, \
                      and a GKE Autopilot cluster — with a dry-run that shows every API call \
                      before a packet leaves your laptop.",
        audience: "For operators",
        benefit: "You walk out running the same stack a working law firm runs, on your own \
                  Google Cloud project, for your own community. One command does most of the \
                  work, and a dry-run shows you every step before a packet leaves your laptop. \
                  It provisions billable cloud resources, so you set a budget alert first — we \
                  give you the command.",
        filename: "DEPLOY.md",
    },
    ManifestEntry {
        category: "workshops",
        slug: "contribute-to-the-navigator",
        title: "Contributing to Neon Law Navigator",
        description: "Neon Law Navigator is open source under Apache-2.0/MIT, run by the \
                      Neon Law Foundation. Five ways to make the corpus better for the next \
                      lawyer — open an issue, share what you learned, join a show-and-tell or \
                      a presentation, or simply use it. No code required for most of them.",
        audience: "For the community",
        benefit: "You walk out knowing five concrete ways to give back — from a GitHub issue \
                  to a template you share to showing up at a show-and-tell — and which one fits \
                  the time you have. The simplest contribution is to use Neon Law Navigator: \
                  every matter you run surfaces the next improvement.",
        filename: "CONTRIBUTE.md",
    },
    // A conference talk folded into Nebula when the standalone
    // Presentations surface was removed. Every code slide is an exact
    // copy of the workspace file it cites; the
    // `rust_in_peace_snippets_are_exact_copies_of_cited_sources` test fails
    // the build if one drifts.
    ManifestEntry {
        category: "presentations",
        slug: "rust-in-peace",
        title: "Rust in Peace",
        description:
            "A Neon Law Foundation talk for Rust NYC on how we use Rust to improve access to \
             justice: deterministic workflows from law — statute to Cucumber feature to template \
             to notation — dissected one modular, attorney-gated step at a time, with every code \
             slide an exact copy of the shipped repository kept honest by a grounding test.",
        audience: "For the hackers",
        benefit: "You walk out able to argue, from the real code, why a reviewed and repeatable \
                  workflow beats prompting an LLM. Every slide is an exact copy of the shipped \
                  repository — a build test fails if one drifts — so you react to the real \
                  thing, not a diagram.",
        filename: "RUST_IN_PEACE.md",
    },
];

/// Load every manifest entry for the Neon Law Navigator workshop. Missing
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
            entry.category,
            entry.slug,
            entry.title,
            entry.description,
            entry.audience,
            entry.benefit,
            &raw,
        ));
    }
    Ok(materials)
}

/// Parse one stepped-content document into a [`WorkshopMaterial`]:
/// split it on `##` headings, render each section to HTML, and keep
/// the raw markdown for the copy-to-clipboard button.
///
/// Fed by the workshop loader with manifest-declared files from disk.
/// The title, description, audience, and benefit come from the caller,
/// not the markdown, so the surface controls its own chrome.
pub(crate) fn material_from_markdown(
    category: &str,
    slug: &str,
    title: &str,
    description: &str,
    audience: &str,
    benefit: &str,
    raw: &str,
) -> WorkshopMaterial {
    let (intro_md, section_specs) = split_sections(raw);
    let sections = section_specs
        .into_iter()
        .map(|(title, body_md)| {
            // Each `##` section is one slide: split its body on the first
            // top-level `---` thematic break into the slide face (above)
            // and the presenter notes (below).
            let (face_md, notes_md) = split_face_notes(&body_md);
            WorkshopSection {
                title,
                body_html: render_markdown(&face_md),
                notes_html: render_markdown(&notes_md),
            }
        })
        .collect();
    WorkshopMaterial {
        category: category.to_string(),
        slug: slug.to_string(),
        title: title.to_string(),
        description: description.to_string(),
        audience: audience.to_string(),
        benefit: benefit.to_string(),
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

/// Split one `##` section's markdown into `(slide_face, presenter_notes)`
/// on the first top-level `---` thematic break. The face is the slide
/// shown on top; the notes are the prose shown beneath it. A `---` inside
/// a fenced code block is sample text, never a divider. With no divider
/// the whole section is the face and the notes come back empty.
fn split_face_notes(section_md: &str) -> (String, String) {
    let lines: Vec<&str> = section_md.lines().collect();
    let mut in_fence = false;
    for (i, line) in lines.iter().enumerate() {
        if is_fence(line) {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence && is_thematic_break(line) {
            let face = lines[..i].join("\n");
            let notes = lines[i + 1..].join("\n");
            return (face.trim_end().to_string(), notes.trim().to_string());
        }
    }
    (section_md.trim_end().to_string(), String::new())
}

/// True for a `---` thematic break — a line that, trimmed, is three or
/// more dashes and nothing else. This is the slide/notes divider; other
/// break styles (`***`, `___`) are left as ordinary `<hr>` in the face.
fn is_thematic_break(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && t.bytes().all(|b| b == b'-')
}

#[cfg(test)]
mod tests {
    use super::{load_navigator, split_face_notes, split_sections, strip_leading_h1};
    use std::fs;
    use tempfile::TempDir;

    /// The slides + presenter-notes format is the contract for every
    /// workshop, now and in the future: each `##` slide must carry a
    /// `---` divider with presenter notes beneath it. This walks the real
    /// baked content (not a fixture) and fails the build if any slide is
    /// missing its face or its notes — so a new workshop can't ship in the
    /// old prose-only shape.
    #[test]
    fn every_workshop_section_has_presenter_notes() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/content/workshops");
        let materials = load_navigator(std::path::Path::new(root)).unwrap();
        assert!(
            !materials.is_empty(),
            "real workshop content failed to load from {root}"
        );
        for m in &materials {
            assert!(
                !m.sections.is_empty(),
                "workshop `{}` has no `##` slides",
                m.slug
            );
            for (i, s) in m.sections.iter().enumerate() {
                assert!(
                    !s.body_html.trim().is_empty(),
                    "workshop `{}` slide {} (`{}`) has an empty slide face",
                    m.slug,
                    i + 1,
                    s.title
                );
                assert!(
                    !s.notes_html.trim().is_empty(),
                    "workshop `{}` slide {} (`{}`) is missing presenter notes — every slide \
                     needs a `---` divider followed by notes",
                    m.slug,
                    i + 1,
                    s.title
                );
            }
        }
    }

    #[test]
    fn split_face_notes_divides_on_the_first_top_level_break() {
        let (face, notes) = split_face_notes("## Build\n\nSlide face.\n\n---\n\nPresenter notes.");
        assert!(face.contains("Slide face"));
        assert!(!face.contains("Presenter notes"));
        assert!(face.contains("## Build"));
        assert_eq!(notes, "Presenter notes.");
    }

    #[test]
    fn split_face_notes_returns_empty_notes_without_a_divider() {
        let (face, notes) = split_face_notes("## Build\n\nJust a face, no notes.");
        assert!(face.contains("Just a face"));
        assert!(notes.is_empty());
    }

    #[test]
    fn split_face_notes_ignores_a_break_inside_a_code_fence() {
        // A `---` line inside a fenced block is YAML/sample text, not the
        // slide/notes divider.
        let (face, notes) = split_face_notes(
            "## Build\n\n```yaml\nkey: value\n---\nmore: yaml\n```\n\n---\n\nNotes.",
        );
        assert!(face.contains("more: yaml"), "fenced --- stays in the face");
        assert_eq!(notes, "Notes.");
    }

    #[test]
    fn loaded_section_carries_face_and_notes_html() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("navigator");
        fs::create_dir_all(&target).unwrap();
        fs::write(
            target.join("README.md"),
            "# T\n\nLede.\n\n## Step one\n\nThe slide.\n\n---\n\nThe notes.\n",
        )
        .unwrap();
        let m = &load_navigator(dir.path()).unwrap()[0];
        assert!(m.sections[0].body_html.contains("The slide"));
        assert!(!m.sections[0].body_html.contains("The notes"));
        assert!(m.sections[0].notes_html.contains("The notes"));
    }

    #[test]
    fn load_navigator_returns_empty_when_directory_missing() {
        let materials = load_navigator(std::path::Path::new("/no/such/dir/12345")).unwrap();
        assert!(materials.is_empty());
    }

    /// The "Rust in Peace" talk became a workshop when the standalone
    /// Presentations surface was removed. Its convention survives the move:
    /// every code slide is introduced by ``From `path/to/file`:`` followed
    /// by a fenced block, and must be an **exact copy** of that workspace
    /// file. This walks the baked talk, reads each cited file from the
    /// workspace (not a second baked copy, which would always pass), and
    /// fails the build when a snippet drifts. The floor assertion keeps the
    /// convention itself from silently vanishing.
    #[test]
    fn rust_in_peace_snippets_are_exact_copies_of_cited_sources() {
        const TALK: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/content/workshops/navigator/RUST_IN_PEACE.md"
        ));
        let workspace_root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
        let lines: Vec<&str> = TALK.lines().collect();
        let mut grounded = 0;
        let mut i = 0;
        while i < lines.len() {
            if let Some(path) = lines[i]
                .strip_prefix("From `")
                .and_then(|rest| rest.strip_suffix("`:"))
            {
                let mut open = i + 1;
                while open < lines.len() && !lines[open].starts_with("```") {
                    open += 1;
                }
                assert!(
                    open < lines.len(),
                    "attribution for {path} has no code fence after it"
                );
                let mut close = open + 1;
                while close < lines.len() && lines[close] != "```" {
                    close += 1;
                }
                assert!(close < lines.len(), "code fence for {path} is never closed");
                let snippet = lines[open + 1..close].join("\n");
                let source = fs::read_to_string(format!("{workspace_root}/{path}"))
                    .unwrap_or_else(|e| panic!("cited source {path} is unreadable: {e}"));
                assert!(
                    source.contains(&snippet),
                    "slide snippet drifted from {path} — update the talk to match the source"
                );
                grounded += 1;
                i = close;
            }
            i += 1;
        }
        assert!(
            grounded >= 6,
            "expected at least 6 grounded snippets in the talk, found {grounded}"
        );
    }

    #[test]
    fn load_navigator_loads_the_rust_in_peace_talk_as_a_workshop() {
        // The talk now rides the workshop manifest; with its file present it
        // loads beside README/DEPLOY with steps split on its `##` beats.
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("navigator");
        fs::create_dir_all(&target).unwrap();
        fs::write(
            target.join("RUST_IN_PEACE.md"),
            "# Rust in Peace\n\nLede.\n\n## Agenda\n\nWhat we'll cover.\n",
        )
        .unwrap();
        let materials = load_navigator(dir.path()).unwrap();
        let talk = materials
            .iter()
            .find(|m| m.slug == "rust-in-peace")
            .expect("rust-in-peace loads as a workshop");
        assert_eq!(talk.title, "Rust in Peace");
        assert_eq!(talk.sections[0].title, "Agenda");
    }

    #[test]
    fn load_navigator_loads_the_using_workshop_from_readme() {
        // With only README.md on disk, the other manifest entries
        // (DEPLOY/CONTRIBUTE/RUST_IN_PEACE) are silently skipped, so the
        // load is exactly the "Using Neon Law Navigator" workshop.
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("navigator");
        fs::create_dir_all(&target).unwrap();
        fs::write(
            target.join("README.md"),
            "# Runbook\n\nWelcome to Neon Law Navigator.\n",
        )
        .unwrap();
        let materials = load_navigator(dir.path()).unwrap();
        assert_eq!(materials.len(), 1, "only README.md is on disk");
        assert_eq!(materials[0].category, "workshops");
        assert_eq!(materials[0].slug, "use-the-navigator");
        assert_eq!(materials[0].title, "Using Neon Law Navigator");
        // The audience tag and you-voiced benefit ride the manifest, not
        // the markdown — the overview card is fed from these.
        assert_eq!(materials[0].audience, "For lawyers");
        assert!(
            materials[0].benefit.starts_with("You walk out"),
            "benefit is second-person and leads with the takeaway, got: {}",
            materials[0].benefit
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
