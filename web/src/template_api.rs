//! `GET /api/templates/:category/:name` — raw template markdown, served
//! inline so a reader on neonlaw.com sees the same bytes a git reader
//! sees. This backs the repository README's template links (e.g.
//! `notation_templates/nest/nevada.md`) without the `notation_templates/` tree leaving the
//! workspace root: it is still `include_str!`-d by `store::seed` and
//! scanned by `cli validate`. Here `web` embeds the whole tree a second
//! time, read-only, purely to serve it over HTTP.
//!
//! Only templates whose frontmatter explicitly declares
//! `confidential: false` are served. The bulk of the tree is
//! `confidential: true` — client-data-bearing onboarding and engagement
//! bodies — and those return 404. The check **fails closed**: a template
//! with no `confidential` key is treated as confidential, mirroring the
//! curated gallery's allow-list stance (`template_gallery`).

use include_dir::{include_dir, Dir};

/// The repository `notation_templates/` tree, embedded at build time. The path is
/// resolved against `web`'s manifest dir, so it tracks the dir in place
/// at the workspace root.
static TEMPLATES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../notation_templates");

/// Raw markdown for a non-confidential template, or `None` when the path
/// is unknown, the template is confidential, or the segments could be a
/// traversal attempt.
#[must_use]
pub fn find_raw(category: &str, name: &str) -> Option<&'static str> {
    // A real category/name is a single path segment each. Reject empties
    // and anything carrying a separator or a dot (`.`/`..`) so the lookup
    // can never escape the embedded tree.
    if [category, name]
        .iter()
        .any(|s| s.is_empty() || s.contains(['/', '\\', '.']))
    {
        return None;
    }
    // URLs are kebab-case (the route redirects an underscore request to
    // its hyphenated home first); the embedded tree keeps the on-disk
    // underscore names. Match by comparing the canonical kebab form of
    // both sides — `_`→`-` is not invertible, so we resolve forward from
    // the real names rather than guess a filename from the slug.
    let (want_category, want_name) = (views::slug::to_url(category), views::slug::to_url(name));
    let dir = TEMPLATES.dirs().find(|d| {
        d.path()
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| views::slug::to_url(n) == want_category)
    })?;
    let file = dir.files().find(|f| {
        f.path()
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| views::slug::to_url(s) == want_name)
    })?;
    let raw = file.contents_utf8()?;
    is_public(raw).then_some(raw)
}

/// Just the `confidential` flag of a template's frontmatter.
#[derive(serde::Deserialize)]
struct ConfidentialFlag {
    confidential: Option<bool>,
}

/// True only when the template's frontmatter carries an explicit
/// `confidential: false`. Absent or `true` → not public (fail closed).
fn is_public(raw: &str) -> bool {
    let Some(frontmatter) = frontmatter_block(raw) else {
        return false;
    };
    matches!(
        serde_yaml::from_str::<ConfidentialFlag>(frontmatter),
        Ok(ConfidentialFlag {
            confidential: Some(false)
        })
    )
}

/// The YAML between the opening `---` and the next `---`, or `None` when
/// the document has no frontmatter fence.
fn frontmatter_block(raw: &str) -> Option<&str> {
    let after = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))?;
    let end = after.find("\n---")?;
    Some(&after[..end])
}

#[cfg(test)]
mod tests {
    use super::{find_raw, is_public};

    #[test]
    fn serves_a_non_confidential_template_verbatim() {
        // nest/nevada is `confidential: false`, and the README links to
        // it — the raw bytes must come back so the link resolves.
        let raw = find_raw("nest", "nevada").expect("nest/nevada is public");
        assert!(raw.starts_with("---\n"), "served the raw markdown file");
        assert!(
            raw.contains("Nevada"),
            "served the actual Nevada entity-formation template"
        );
    }

    #[test]
    fn resolves_the_kebab_url_form_to_underscore_filenames() {
        // The route serves kebab-case URLs; the embedded tree keeps the
        // on-disk underscore names. A kebab `name` segment must resolve to
        // its underscore file…
        let by_kebab = find_raw("nonprofit", "form990-annual-report")
            .expect("kebab name resolves to form990_annual_report.md");
        assert!(by_kebab.contains("Form 990"));
        // …and so must a kebab `category` segment (`annual_report` →
        // `annual-report`).
        assert!(
            find_raw("annual-report", "nevada").is_some(),
            "kebab category resolves to the annual_report/ folder"
        );
        // The legacy underscore spellings still resolve (the route only
        // redirects the browser; direct callers and tests keep working).
        assert!(find_raw("nonprofit", "form990_annual_report").is_some());
    }

    #[test]
    fn refuses_a_confidential_template() {
        // The retainer is `confidential: true` and must never be served
        // over the public API even though the path is valid.
        assert!(
            find_raw("onboarding", "retainer").is_none(),
            "confidential templates must 404"
        );
    }

    #[test]
    fn unknown_path_is_none() {
        assert!(find_raw("nope", "missing").is_none());
    }

    #[test]
    fn rejects_path_traversal_segments() {
        assert!(find_raw("..", "nevada").is_none());
        assert!(find_raw("nest", "../onboarding/retainer").is_none());
        assert!(find_raw("nest/..", "nevada").is_none());
        assert!(find_raw("", "nevada").is_none());
    }

    #[test]
    fn is_public_fails_closed_without_the_key() {
        assert!(!is_public("---\ntitle: X\n---\nbody"));
        assert!(!is_public("no frontmatter at all"));
        assert!(is_public("---\nconfidential: false\n---\nbody"));
        assert!(!is_public("---\nconfidential: true\n---\nbody"));
    }
}
