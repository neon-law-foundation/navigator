#![allow(clippy::doc_markdown)]
//! `/foundation/navigator` — the Neon Law Navigator hub.
//!
//! Foundation-branded landing page for the open-source project. Top to
//! bottom: a logo banner, the sovereign-software positioning ("Your
//! practice. Your data. Your cloud."), the cross-package strip (LSP / CLI /
//! MCP / Web), and — under the strip — the workspace `README.md`. The strip
//! acts as a tab row: nothing is selected on the hub, so the hub shows the
//! README; each per-package page selects its own tab and renders that
//! crate's README instead ([`crate::pages::package`]).
//!
//! The README is baked in at compile time with `include_str!`. Its
//! repo-relative links — written so they resolve for a git reader — are
//! retargeted by [`rewrite_link`] so they also resolve on the web (the
//! per-package pages reuse the same retargeting):
//!
//! - a top-level `docs/<name>.md` → the published `/docs/<name>` route;
//! - a `notation_templates/**/*.md` link → the raw `/api/templates/**`
//!   endpoint (`web::template_api`);
//! - every other repo-relative path (nested docs, `LICENSE-*`) → the
//!   GitHub source, so no link dead-ends;
//! - absolute URLs and same-page anchors pass through untouched.

use maud::{html, Markup};

use crate::brand::{foundation_github_url, FOUNDATION_BRAND};
use crate::markdown::render_with_link_rewrite;
use crate::{AuthState, Locale, PageLayout};

/// The workspace README, baked in at compile time. Resolved against the
/// `views` crate manifest dir, so it points at the workspace-root file.
const README: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../README.md"));

/// GitHub blob root for repo-relative links with no on-site route.
const REPO_BLOB_BASE: &str = "https://github.com/neon-law-foundation/navigator/blob/main/";

#[must_use]
pub fn render(auth: AuthState) -> Markup {
    render_in(auth, Locale::En)
}

/// Render the hub in `locale`. The hero and sovereign-software copy are
/// transcreated (Tier-A marketing prose — see [`docs/i18n.md`]); the package
/// strip and the README body below stay English (the README is an
/// English-only artifact by the English-first invariant). Both locales carry
/// the canonical path so the layout emits the `hreflang` pair and the navbar
/// language switcher.
#[must_use]
pub fn render_in(auth: AuthState, locale: Locale) -> Markup {
    let body = html! {
        (hero(locale))
        (sovereign_software(locale))
        (packages())
        // Under the strip: the README, unless a package tab is selected
        // (which happens on the per-package pages, not the hub).
        article.docs-article {
            (render_with_link_rewrite(README, rewrite_link))
        }
    };
    let description = match locale {
        Locale::En => {
            "Neon Law Navigator is sovereign legal software from the Neon Law \
             Foundation — open source under Apache-2.0 or MIT, built to self-host \
             so your data stays in your own cloud."
        }
        Locale::Es => {
            "Neon Law Navigator es software legal soberano de la Neon Law \
             Foundation: código abierto bajo Apache-2.0 o MIT, hecho para \
             autoalojarse para que tus datos se queden en tu propia nube."
        }
    };
    PageLayout::new("Neon Law Navigator")
        .with_description(description)
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .with_locale(locale)
        .with_canonical_path("/foundation/navigator")
        .render(&body)
}

/// The logo banner: the Foundation mark, the product wordmark, the
/// Sovereign Software tagline, and the GitHub call to action. The wordmark
/// and brand mark are proper nouns — identical in every locale.
fn hero(locale: Locale) -> Markup {
    let (tagline, subtitle, cta) = match locale {
        Locale::En => (
            "Sovereign legal software you can run yourself.",
            "An open-source operating system for a modern law practice — versioned \
             legal templates, durable workflows, attorney-reviewed automation, and \
             agent-accessible tooling.",
            "View on GitHub",
        ),
        Locale::Es => (
            "Software legal soberano que tú mismo operas.",
            "Un sistema operativo de código abierto para la práctica legal moderna: \
             plantillas legales versionadas, flujos de trabajo duraderos, automatización \
             revisada por abogados y herramientas accesibles para agentes.",
            "Ver en GitHub",
        ),
    };
    html! {
        section."text-center"."bg-body-tertiary"."rounded-3"."p-5"."mb-5" {
            img."mb-3"
                src=(FOUNDATION_BRAND.logo_href)
                alt=(format!("{} logo", FOUNDATION_BRAND.site_name))
                width="72"
                height="72";
            h1."display-4"."fw-bold"."mb-2" { "Neon Law Navigator" }
            p."lead"."mb-2" { (tagline) }
            p."mx-auto"."mb-4"."text-body-secondary" style="max-width: 44rem;" {
                (subtitle)
            }
            div."d-flex"."justify-content-center" {
                a."btn"."btn-primary"."btn-lg" href=(foundation_github_url()) {
                    i."bi bi-github me-2" aria-hidden="true" {}
                    (cta)
                }
            }
        }
    }
}

/// The sovereign-software positioning — the headline idea, kept above the
/// package strip. Two short paragraphs, no cards. Transcreated per locale;
/// proper nouns (the cloud-native components) carry verbatim.
fn sovereign_software(locale: Locale) -> Markup {
    let heading = match locale {
        Locale::En => "Your practice. Your data. Your cloud.",
        Locale::Es => "Tu práctica. Tus datos. Tu nube.",
    };
    html! {
        section."mb-5" id="sovereign-software" {
            h2."mb-3" { (heading) }
            @match locale {
                Locale::En => {
                    p {
                        "Neon Law Navigator is sovereign software: predominantly open source \
                         under Apache-2.0 or MIT, built to run on infrastructure you control. \
                         Self-host it, and your client data stays where you put it."
                    }
                    p."mb-0" {
                        "Neon Law runs it on "
                        a href="https://cloud.google.com" { "Google Cloud" }
                        ". Because the stack is cloud-native open source — "
                        a href="https://kubernetes.io" { "Kubernetes" }
                        " for orchestration, Postgres for data, and licensable services like "
                        a href="https://restate.dev" { "Restate" }
                        " for durable execution — you can run the same system in your own cloud."
                    }
                }
                Locale::Es => {
                    p {
                        "Neon Law Navigator es software soberano: predominantemente de código \
                         abierto bajo Apache-2.0 o MIT, hecho para ejecutarse en la infraestructura \
                         que tú controlas. Alójalo tú mismo y los datos de tus clientes se quedan \
                         donde tú los pongas."
                    }
                    p."mb-0" {
                        "Neon Law lo ejecuta en "
                        a href="https://cloud.google.com" { "Google Cloud" }
                        ". Como la base es código abierto nativo de la nube — "
                        a href="https://kubernetes.io" { "Kubernetes" }
                        " para la orquestación, Postgres para los datos y servicios con licencia como "
                        a href="https://restate.dev" { "Restate" }
                        " para la ejecución duradera — puedes ejecutar el mismo sistema en tu propia nube."
                    }
                }
            }
        }
    }
}

/// The cross-package strip — the tab row above the README. Nothing is
/// selected on the hub, so the hub renders the README; selecting a tab
/// navigates to that package's page.
fn packages() -> Markup {
    html! {
        section."mb-4" {
            (crate::pages::package::package_strip(None))
        }
    }
}

/// Retarget one README link so it resolves on the website. See the module
/// docs for the mapping. `pub(crate)` so the per-package pages
/// ([`crate::pages::package`]) reuse the exact same retargeting.
pub(crate) fn rewrite_link(dest: &str) -> String {
    // Absolute URLs, mailto, and same-page anchors already resolve.
    if dest.starts_with("http://")
        || dest.starts_with("https://")
        || dest.starts_with("mailto:")
        || dest.starts_with('#')
    {
        return dest.to_string();
    }
    let (path, anchor) = match dest.split_once('#') {
        Some((p, a)) => (p, Some(a)),
        None => (dest, None),
    };
    // A top-level `docs/<name>.md` is a published doc; nested docs
    // (`docs/lsp/README.md`) are not, so they fall through to GitHub.
    if let Some(stem) = path
        .strip_prefix("docs/")
        .and_then(|rest| rest.strip_suffix(".md"))
    {
        if !stem.contains('/') {
            // URLs are kebab-case; the file stem keeps its underscores.
            return with_anchor(&format!("/docs/{}", crate::slug::to_url(stem)), anchor);
        }
    }
    // A `notation_templates/**/*.md` link maps to the raw template API.
    if let Some(stem) = path
        .strip_prefix("notation_templates/")
        .and_then(|rest| rest.strip_suffix(".md"))
    {
        return with_anchor(
            &format!("/api/templates/{}", crate::slug::to_url(stem)),
            anchor,
        );
    }
    // Everything else repo-relative resolves against the GitHub source.
    with_anchor(&format!("{REPO_BLOB_BASE}{path}"), anchor)
}

fn with_anchor(base: &str, anchor: Option<&str>) -> String {
    match anchor {
        Some(a) => format!("{base}#{a}"),
        None => base.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{render, render_in, rewrite_link, README};
    use crate::brand::FOUNDATION_BRAND;
    use crate::{AuthState, Locale};

    #[test]
    fn english_hub_offers_a_spanish_switcher() {
        let html = render(AuthState::Anonymous).into_string();
        // The page now declares a canonical path, so the layout pairs the
        // twins and renders the one-tap switcher to the Spanish hub.
        assert!(html.contains("hreflang=\"es\" href=\"/es/foundation/navigator\""));
        assert!(
            html.contains("language-switcher") && html.contains(">Español</a>"),
            "English hub should offer a Spanish switcher: {html}"
        );
    }

    #[test]
    fn spanish_hub_transcreates_the_hero_and_pitch_but_keeps_the_readme_english() {
        let html = render_in(AuthState::Anonymous, Locale::Es).into_string();
        // Spanish shell.
        assert!(html.contains("<html lang=\"es\""), "got: {html}");
        // Transcreated hero + sovereign copy.
        assert!(html.contains("Software legal soberano que tú mismo operas."));
        assert!(html.contains(">Tu práctica. Tus datos. Tu nube.</h2>"));
        assert!(html.contains("predominantemente de código abierto bajo Apache-2.0 o MIT"));
        assert!(html.contains("Ver en GitHub"));
        // Proper nouns carry verbatim.
        assert!(html.contains("Kubernetes") && html.contains("Restate"));
        // The README body below stays English (English-only artifact).
        assert!(html.contains("cargo run -p cli -- start-dev-server"));
        // The switcher points back to the English hub.
        assert!(html.contains("hreflang=\"en\" href=\"/foundation/navigator\""));
    }

    #[test]
    fn navigator_renders_under_the_foundation_brand() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!(
            "<title>{} | Neon Law Navigator</title>",
            FOUNDATION_BRAND.site_name
        )));
        // The hero leads with the product wordmark as the page H1.
        assert!(
            html.contains(">Neon Law Navigator</h1>"),
            "expected the product wordmark as the page heading: {html}"
        );
    }

    #[test]
    fn hero_is_a_logo_banner_with_one_github_cta() {
        let html = render(AuthState::Anonymous).into_string();
        // The Foundation mark rides the banner as an <img>.
        assert!(
            html.contains(&format!("src=\"{}\"", FOUNDATION_BRAND.logo_href)),
            "hero should show the Foundation logo: {html}"
        );
        assert!(html.contains("Sovereign legal software you can run yourself."));
        // One call to action: GitHub. No How-it-works button.
        assert!(html.contains("View on GitHub"));
        assert!(!html.contains("href=\"#how-it-works\""));
    }

    #[test]
    fn sovereign_text_makes_the_self_host_case_without_cards() {
        let html = render(AuthState::Anonymous).into_string();
        // The customer-forward heading; "sovereign" stays the idea in the
        // body, not the visible label.
        assert!(
            html.contains(">Your practice. Your data. Your cloud.</h2>"),
            "got: {html}"
        );
        // The load-bearing claims live in the two paragraphs.
        assert!(html.contains("predominantly open source under Apache-2.0 or MIT"));
        assert!(html.contains("your client data stays where you put it"));
        assert!(html.contains("run the same system in your own cloud"));
        assert!(html.contains("Kubernetes"));
        assert!(html.contains("Restate"));
        // The three-card row is gone.
        assert!(
            !html.contains(">Self-hosted</h3>") && !html.contains(">Your cloud</h3>"),
            "the pillar cards should be removed: {html}"
        );
    }

    #[test]
    fn strip_then_readme_with_no_package_preselected() {
        let html = render(AuthState::Anonymous).into_string();
        // The tab strip links every package…
        assert!(html.contains("aria-label=\"Neon Law Navigator packages\""));
        for href in [
            "/foundation/navigator/lsp",
            "/foundation/navigator/cli",
            "/foundation/navigator/mcp",
            "/foundation/navigator/web",
        ] {
            assert!(
                html.contains(&format!("href=\"{href}\"")),
                "missing {href}: {html}"
            );
        }
        // …but nothing is preselected on the hub (no active card).
        assert!(
            !html.contains("border-primary") && !html.contains("aria-current=\"page\""),
            "the hub should not preselect a package: {html}"
        );
        // The README renders under the strip: its getting-started command and
        // its Trademarks anchor (the notations page cross-links it).
        assert!(html.contains("cargo run -p cli -- start-dev-server"));
        assert!(html.contains("id=\"trademarks\""));
        // The strip sits above the README body.
        let strip = html
            .find("aria-label=\"Neon Law Navigator packages\"")
            .expect("package strip");
        let readme = html.find("start-dev-server").expect("readme body");
        assert!(strip < readme, "strip should sit above the README: {html}");
    }

    #[test]
    fn readme_links_are_retargeted_for_the_web() {
        let html = render(AuthState::Anonymous).into_string();
        // A top-level doc link maps to its published route; a template link
        // maps to the raw template API.
        assert!(html.contains("href=\"/docs/glossary#project\""));
        assert!(html.contains(
            "href=\"/api/templates/united-states/nevada/state/business-associations/entity-formation\""
        ));
    }

    #[test]
    fn navigator_is_baked_from_the_workspace_readme() {
        assert!(README.starts_with("# Neon Law Navigator"));
        assert!(README.contains("## License"));
        // The README carries no standalone Sovereign Software section — the
        // hub surfaces that pitch above the strip, so the README would only
        // duplicate it.
        assert!(!README.contains("## Sovereign Software"));
    }

    #[test]
    fn navigator_description_meta_is_emitted() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.contains("name=\"description\""));
        assert!(html.contains("sovereign legal software"));
    }

    #[test]
    fn top_level_doc_links_map_to_site_routes() {
        assert_eq!(
            rewrite_link("docs/glossary.md#project"),
            "/docs/glossary#project"
        );
        assert_eq!(
            rewrite_link("docs/notation.md#templates"),
            "/docs/notation#templates"
        );
        assert_eq!(rewrite_link("docs/glossary.md"), "/docs/glossary");
        // An underscore doc filename is rewritten to its kebab-case URL.
        assert_eq!(
            rewrite_link("docs/retainer_intake.md"),
            "/docs/retainer-intake"
        );
    }

    #[test]
    fn template_links_map_to_the_raw_api() {
        assert_eq!(
            rewrite_link("notation_templates/united_states/nevada/state/business_associations/entity_formation.md"),
            "/api/templates/united-states/nevada/state/business-associations/entity-formation"
        );
        // Underscores in either segment become hyphens in the URL.
        assert_eq!(
            rewrite_link(
                "notation_templates/united_states/federal/irs/taxation/form990_annual_report.md"
            ),
            "/api/templates/united-states/federal/irs/taxation/form990-annual-report"
        );
        assert_eq!(
            rewrite_link("notation_templates/united_states/nevada/state/business_associations/annual_report.md"),
            "/api/templates/united-states/nevada/state/business-associations/annual-report"
        );
    }

    #[test]
    fn nested_and_other_relative_links_point_at_github() {
        // Nested docs aren't published at `/docs/:slug`.
        assert_eq!(
            rewrite_link("docs/lsp/README.md"),
            "https://github.com/neon-law-foundation/navigator/blob/main/docs/lsp/README.md"
        );
        // License files live in the repo root, served from GitHub.
        assert_eq!(
            rewrite_link("LICENSE-APACHE"),
            "https://github.com/neon-law-foundation/navigator/blob/main/LICENSE-APACHE"
        );
    }

    #[test]
    fn absolute_and_anchor_links_pass_through() {
        assert_eq!(
            rewrite_link("https://www.neonlaw.com/foundation/mission"),
            "https://www.neonlaw.com/foundation/mission"
        );
        assert_eq!(rewrite_link("#trademarks"), "#trademarks");
        assert_eq!(rewrite_link("https://zed.dev"), "https://zed.dev");
    }
}
