//! Bake the workspace docs into the binary and render them to HTML.
//!
//! The manifest below `include_str!`s every top-level `docs/*.md`,
//! resolved from `CARGO_MANIFEST_DIR` so the
//! paths are robust to where the binary runs. There is deliberately no
//! glob and no runtime file read: the prod image builds from `web/`, so
//! `docs/` is outside it. Adding a doc means adding a manifest line —
//! and [`tests::manifest_covers_every_top_level_doc`] fails the build if
//! a new `docs/*.md` is left out, so the "serve every doc" invariant
//! can't silently drift into a curated allowlist.
//!
//! Two transforms run at render time over the pulldown-cmark event
//! stream:
//!
//! 1. [`rewrite_link`] maps a same-directory `foo.md` / `foo.md#bar`
//!    reference to `/docs/foo` / `/docs/foo#bar`, leaving external URLs,
//!    bare `#anchors`, and `../`-relative links untouched (those still
//!    resolve as repo-relative links on GitHub).
//! 2. Every heading gets a GitHub-style slug `id`, so the in-page
//!    `#anchor` links the rewriter produces actually land.

use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};

use super::{Doc, DocsIndex};

/// `CARGO_MANIFEST_DIR` is `…/web`; the docs tree is one level up.
macro_rules! doc {
    ($slug:literal, $rel:literal) => {
        (
            $slug,
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../", $rel)),
        )
    };
}

/// `(slug, raw_markdown)` for every published doc. The slug is the file
/// stem so [`rewrite_link`] (`notation.md` → `/docs/notation`) lines up
/// with the route. Keep this list 1:1 with the top-level `docs/*.md`
/// files; the completeness test enforces it.
const MANIFEST: &[(&str, &str)] = &[
    doc!("access-model", "docs/access-model.md"),
    doc!("aida-a2a-interaction", "docs/aida-a2a-interaction.md"),
    doc!("bulk-contact-import", "docs/bulk-contact-import.md"),
    doc!("cronjobs", "docs/cronjobs.md"),
    doc!("docusign-esignature", "docs/docusign-esignature.md"),
    doc!("durable-workflows", "docs/durable-workflows.md"),
    doc!("editing-workflows", "docs/editing-workflows.md"),
    doc!("email-events-pipeline", "docs/email-events-pipeline.md"),
    doc!("env-driven-devx", "docs/env-driven-devx.md"),
    doc!("erd", "docs/erd.md"),
    doc!("gemini-enterprise-mcp", "docs/gemini-enterprise-mcp.md"),
    doc!("git-project-repos", "docs/git-project-repos.md"),
    doc!("gitops", "docs/gitops.md"),
    doc!("gke-prod", "docs/gke-prod.md"),
    doc!("glossary", "docs/glossary.md"),
    doc!("gov-forms", "docs/gov-forms.md"),
    doc!("iceberg-archive", "docs/iceberg-archive.md"),
    doc!("i18n", "docs/i18n.md"),
    doc!("multi-cloud", "docs/multi-cloud.md"),
    doc!("nautilus-design", "docs/nautilus-design.md"),
    doc!("nautilus-workflows", "docs/nautilus-workflows.md"),
    doc!("northstar-estate-flow", "docs/northstar-estate-flow.md"),
    doc!("notation", "docs/notation.md"),
    doc!("notation-authoring", "docs/notation-authoring.md"),
    doc!("observability", "docs/observability.md"),
    doc!("oidc", "docs/oidc.md"),
    doc!("oss-install", "docs/oss-install.md"),
    doc!("recurring-billing", "docs/recurring-billing.md"),
    doc!("retainer_intake", "docs/retainer_intake.md"),
    doc!("RUNBOOK", "docs/RUNBOOK.md"),
    doc!("secrets-doppler", "docs/secrets-doppler.md"),
    doc!("solana-attestation", "docs/solana-attestation.md"),
    doc!("test-database", "docs/test-database.md"),
    doc!(
        "third-party-integrations",
        "docs/third-party-integrations.md"
    ),
    doc!("workspace-layout", "docs/workspace-layout.md"),
    doc!("xero-billing", "docs/xero-billing.md"),
];

/// Build the index of baked docs. Parsed once at boot.
#[must_use]
pub fn bundled() -> DocsIndex {
    let mut docs: Vec<Doc> = MANIFEST
        .iter()
        .map(|(slug, raw)| Doc {
            // The manifest keys are the on-disk file stems (so the
            // completeness test can diff them against `docs/*.md`); the
            // route slug is their kebab-case URL form.
            slug: views::slug::to_url(slug),
            title: title_from_markdown(raw, slug),
            body_html: render_markdown(raw),
        })
        .collect();
    docs.sort_by(|a, b| a.slug.cmp(&b.slug));
    DocsIndex::new(docs)
}

/// The page title is the doc's first `# ` heading. Anything else before
/// an H1 means the file leads with content, not a title — fall back to
/// the slug rather than guessing.
fn title_from_markdown(raw: &str, fallback: &str) -> String {
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("# ") {
            return heading.trim().to_string();
        }
        if !trimmed.is_empty() {
            break;
        }
    }
    fallback.to_string()
}

/// Map a markdown link destination to a site route. Only a
/// same-directory markdown reference is rewritten:
///
/// - `notation.md`        → `/docs/notation`
/// - `glossary.md#blob`   → `/docs/glossary#blob`
///
/// Everything else is returned verbatim so it keeps working as a
/// repo-relative link on GitHub: external URLs (`https://…`,
/// `mailto:…`), bare in-page anchors (`#council`), and any link with a
/// path component (`../store/foo.rs`,
/// `../web/content/marketing/mission.md`, `../web/content/marketing/home.md`).
#[must_use]
pub fn rewrite_link(dest: &str) -> String {
    let (path, anchor) = match dest.split_once('#') {
        Some((p, a)) => (p, Some(a)),
        None => (dest, None),
    };
    // Bare `#anchor` (same-page) or a link carrying any path component
    // is left alone — only a sibling `name.md` in `docs/` maps to a
    // `/docs/name` route.
    if path.is_empty() || path.contains('/') {
        return dest.to_string();
    }
    let Some(stem) = path.strip_suffix(".md") else {
        return dest.to_string();
    };
    if stem.is_empty() {
        return dest.to_string();
    }
    // URLs are kebab-case; the file stem keeps its underscores. The
    // `#anchor` is a heading slug (which may legitimately hold
    // underscores), so it is passed through untouched.
    let stem = views::slug::to_url(stem);
    match anchor {
        Some(a) => format!("/docs/{stem}#{a}"),
        None => format!("/docs/{stem}"),
    }
}

/// GitHub-style heading slug: lowercase, drop punctuation, spaces → `-`,
/// keep existing hyphens and underscores. Matches the anchors our docs
/// already link to (`Engagement / Retainer` → `engagement--retainer`).
#[must_use]
fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c == ' ' {
            out.push('-');
        } else if c == '-' || c == '_' {
            out.push(c);
        }
    }
    out
}

/// Render markdown to HTML, rewriting `.md` links to `/docs/*` routes
/// and stamping a slug `id` on every heading so in-page anchors resolve.
#[must_use]
fn render_markdown(src: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_FOOTNOTES);

    let events: Vec<Event> = Parser::new_ext(src, opts).collect();
    let mut out_events: Vec<Event> = Vec::with_capacity(events.len());

    for i in 0..events.len() {
        match &events[i] {
            // Stamp a slug id on headings that don't already declare one.
            Event::Start(Tag::Heading {
                level,
                id: None,
                classes,
                attrs,
            }) => {
                let text = heading_text(&events[i + 1..]);
                out_events.push(Event::Start(Tag::Heading {
                    level: *level,
                    id: Some(slugify(&text).into()),
                    classes: classes.clone(),
                    attrs: attrs.clone(),
                }));
            }
            // Repoint markdown-relative links/images at site routes.
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => out_events.push(Event::Start(Tag::Link {
                link_type: *link_type,
                dest_url: rewrite_link(dest_url).into(),
                title: title.clone(),
                id: id.clone(),
            })),
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => out_events.push(Event::Start(Tag::Image {
                link_type: *link_type,
                dest_url: rewrite_link(dest_url).into(),
                title: title.clone(),
                id: id.clone(),
            })),
            other => out_events.push(other.clone()),
        }
    }

    let mut out = String::new();
    html::push_html(&mut out, out_events.into_iter());
    out
}

/// Concatenate the text of a heading from the events that follow its
/// `Start(Heading)` up to the matching `End`. `Code` spans count as
/// text so `## `code`` headings still slug sensibly.
fn heading_text(rest: &[Event]) -> String {
    let mut text = String::new();
    for ev in rest {
        match ev {
            Event::End(TagEnd::Heading(_)) => break,
            Event::Text(t) | Event::Code(t) => text.push_str(t),
            _ => {}
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{bundled, rewrite_link, slugify, title_from_markdown, MANIFEST};
    use std::collections::HashSet;

    #[test]
    fn rewrite_link_maps_sibling_md_to_route() {
        assert_eq!(rewrite_link("notation.md#x"), "/docs/notation#x");
        assert_eq!(rewrite_link("glossary.md"), "/docs/glossary");
        assert_eq!(rewrite_link("access-model.md"), "/docs/access-model");
        // An underscore filename is rewritten to its kebab-case URL,
        // while a heading anchor (which may carry underscores) is left as
        // authored.
        assert_eq!(rewrite_link("retainer_intake.md"), "/docs/retainer-intake");
        assert_eq!(
            rewrite_link("retainer_intake.md#step_one"),
            "/docs/retainer-intake#step_one"
        );
    }

    #[test]
    fn rewrite_link_leaves_everything_else_untouched() {
        // Non-`.md` repo-relative source links.
        assert_eq!(rewrite_link("../store/foo.rs"), "../store/foo.rs");
        // External URLs.
        assert_eq!(rewrite_link("https://example.com"), "https://example.com");
        assert_eq!(
            rewrite_link("mailto:support@neonlaw.com"),
            "mailto:support@neonlaw.com"
        );
        // Bare in-page anchor.
        assert_eq!(rewrite_link("#council"), "#council");
        // `.md` links that escape the docs dir stay repo-relative.
        assert_eq!(
            rewrite_link("../web/content/marketing/mission.md"),
            "../web/content/marketing/mission.md"
        );
        assert_eq!(rewrite_link("../README.md"), "../README.md");
        assert_eq!(
            rewrite_link("../web/content/marketing/home.md"),
            "../web/content/marketing/home.md"
        );
    }

    #[test]
    fn slugify_matches_github_anchor_rules() {
        assert_eq!(slugify("Council"), "council");
        assert_eq!(slugify("Workflow Runtime"), "workflow-runtime");
        // Punctuation drops, the surrounding spaces each become a hyphen
        // — the double hyphen our notation doc links to.
        assert_eq!(slugify("Engagement / Retainer"), "engagement--retainer");
    }

    #[test]
    fn title_comes_from_leading_h1() {
        assert_eq!(title_from_markdown("# Glossary\n\nbody", "x"), "Glossary");
        assert_eq!(
            title_from_markdown("\n\n# Notation vocabulary\n", "x"),
            "Notation vocabulary"
        );
        // Content before any H1 → fall back to the slug.
        assert_eq!(
            title_from_markdown("lead paragraph\n# Late", "fallback"),
            "fallback"
        );
    }

    #[test]
    fn manifest_covers_every_top_level_doc() {
        // The "serve every doc" invariant: no top-level `docs/*.md` may
        // be silently dropped from the manifest.
        let docs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs");
        let slugs: HashSet<&str> = MANIFEST.iter().map(|(s, _)| *s).collect();
        for entry in std::fs::read_dir(&docs_dir).expect("docs dir readable") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let stem = path.file_stem().unwrap().to_str().unwrap();
            assert!(
                slugs.contains(stem),
                "docs/{stem}.md is not in the docs MANIFEST — add a `doc!(\"{stem}\", \
                 \"docs/{stem}.md\")` line so it publishes at /docs/{stem}"
            );
        }
    }

    #[test]
    fn underscore_doc_publishes_at_its_kebab_slug() {
        // `docs/retainer_intake.md` is the only underscore doc; its route
        // slug is the kebab-case form, even though the manifest key (and
        // the file on disk) keep the underscore.
        let ix = bundled();
        assert!(
            ix.find("retainer-intake").is_some(),
            "retainer_intake.md should publish at /docs/retainer-intake"
        );
        assert!(
            ix.find("retainer_intake").is_none(),
            "the underscore slug is not a valid route — the handler redirects it"
        );
    }

    #[test]
    fn bundled_renders_glossary_and_notation_with_anchors() {
        let ix = bundled();
        let glossary = ix.find("glossary").expect("glossary published");
        assert_eq!(glossary.title, "Glossary");
        // Heading rendered as <h2> with a slug id so `#council` lands.
        assert!(
            glossary
                .body_html
                .contains("<h2 id=\"council\">Council</h2>"),
            "missing slugged Council heading"
        );
        // Cross-doc link rewritten to a site route.
        assert!(
            glossary.body_html.contains("href=\"/docs/notation\""),
            "glossary's notation.md link should point at /docs/notation"
        );
        // A `../`-relative link is left repo-relative.
        assert!(
            glossary
                .body_html
                .contains("href=\"../web/content/marketing/mission.md\""),
            "../web/content/marketing/mission.md should stay repo-relative"
        );

        let notation = ix.find("notation").expect("notation published");
        // notation links glossary.md#blob → /docs/glossary#blob.
        assert!(
            notation.body_html.contains("href=\"/docs/glossary#blob\""),
            "notation's glossary anchor link should be rewritten"
        );
    }

    #[test]
    fn nautilus_design_publishes_with_scope_boundary_and_citations() {
        // The Nautilus design doc is the compliance contract every later
        // workflow PR cites. It must publish at /docs/nautilus-design and
        // carry the four scope-boundary holdings grounded in official
        // statutory citations — not paraphrases that can silently drift.
        let ix = bundled();
        let doc = ix
            .find("nautilus-design")
            .expect("nautilus-design published at /docs/nautilus-design");
        let body = &doc.body_html;
        // The flat-fee / no-cut trust line is load-bearing.
        assert!(
            body.contains("$66") && body.contains("never"),
            "must state the flat $66/mo fee and that it never moves"
        );
        // The four core letters' statutory hooks, by exact section.
        for cite in [
            "1692c(a)(2)", // notice of representation
            "1692g",       // debt validation, 30-day window
            "1692c(c)",    // cease communication
            "1681i",       // FCRA reinvestigation (§611)
        ] {
            assert!(body.contains(cite), "missing FDCPA/FCRA citation {cite}");
        }
        // The compliance carve-outs that keep us out of the TSR advance-fee
        // ban and the bankruptcy debt-relief-agency label.
        assert!(
            body.contains("310.4(a)(5)"),
            "must cite the FTC TSR advance-fee ban it stays clear of"
        );
        assert!(
            body.contains("Milavetz") && body.contains("528"),
            "must cite the bankruptcy debt-relief-agency boundary"
        );
        // The UPL control: a licensed attorney signs every letter.
        assert!(
            body.contains("@approve"),
            "must name the @approve attorney-approval gate as the UPL control"
        );
        // Litigation is referred out, never answered as correspondence.
        assert!(
            body.contains("href=\"/services/litigation\""),
            "must refer litigation out to /services/litigation"
        );
    }

    #[test]
    fn nautilus_workflows_index_publishes_with_step_chain_and_guardrails() {
        // The workflows build index maps each Nautilus letter to its
        // shared-step-library chain and names the UPL gate every PR
        // reuses. It must publish at /docs/nautilus-workflows and stay
        // 1:1 with the five-workflow build sequence.
        let ix = bundled();
        let doc = ix
            .find("nautilus-workflows")
            .expect("nautilus-workflows published at /docs/nautilus-workflows");
        let body = &doc.body_html;
        // The shared step chain: render → attorney-approve → send.
        for token in ["document_open__", "staff_review", "email_send__"] {
            assert!(body.contains(token), "missing shared step prefix {token}");
        }
        // staff_review IS the @approve UPL gate; the guardrail enforces it.
        assert!(
            body.contains("@approve") && body.contains("staff_review_gates_filing"),
            "must tie @approve to the staff_review_gates_filing guardrail"
        );
        // The five letters of the shared template library.
        for letter in [
            "notice_of_representation",
            "debt_validation",
            "cease_communication",
            "fcra_dispute",
            "settlement_letter",
        ] {
            assert!(body.contains(letter), "missing shared template {letter}");
        }
        // One worker, never a per-workflow pod.
        assert!(
            body.contains("workflows-service") && body.contains("one worker"),
            "must state the one-worker rule"
        );
        // Links back to the compliance contract.
        assert!(
            body.contains("href=\"/docs/nautilus-design\""),
            "must link to the nautilus-design compliance contract"
        );
    }
}
