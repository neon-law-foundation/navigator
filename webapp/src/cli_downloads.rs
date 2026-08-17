//! The `navigator` CLI download section of the `/app/team` team home.
//!
//! The Firm publishes no external binary and runs no public download page, so
//! this section is the whole distribution channel. Everyone who operates
//! Navigator reaches it through the team home: Owner, Admin, Lawyer, and Clerk.
//! A `client` is the one authenticated tier denied — at the route (the Rego
//! policy), at the page (the team home's `require_firm_person` loader), and
//! again in this section's own server function — because a matter entitles
//! nobody to the Firm's operating tooling. The software is open source; this
//! gate is about who the page serves.
//!
//! **The section lists what is actually published, never what ought to be.** The
//! portal pre-layer resolves the current release's archives from object storage
//! and injects them; this module renders that list and nothing else. A
//! deployment whose release has not been published yet renders the empty state
//! rather than links that 404 — a download button that fails is worse than an
//! honest "not published yet", because the reader cannot tell whether they hit
//! a permissions problem.
//!
//! **The bytes never travel through this module.** Each row links to
//! `/app/team/download/{platform}`, which the portal resolves to a signed URL
//! or streams. That keeps the archive's storage key server-side and means the
//! wasm bundle carries no object-storage coordinates.
//!
//! The team home that composes this section is [`crate::team_home`].

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::people::ViewerRole;

/// One published archive, as the page offers it.
///
/// `href` is a Navigator route rather than a storage URL: the portal signs or
/// streams behind it, so nothing here leaks a bucket coordinate, and the link
/// stays stable across deployments whose storage differs.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct CliArchive {
    /// The platform slug in the download route — `windows`, `linux`, `macos`.
    pub platform: String,
    /// How the platform is named to a reader ("Windows").
    pub label: String,
    /// The archive's own filename, shown so a reader can confirm what landed
    /// in their downloads folder.
    pub filename: String,
    /// `/app/team/download/{platform}`.
    pub href: String,
    /// A human size ("18.4 MB"), or empty when storage did not report one.
    pub size: String,
}

/// The archives the portal pre-layer resolved, extracted back in
/// [`cli_downloads_view`].
#[derive(Clone, Default)]
pub struct InjectedDownloads {
    /// The release tag these archives belong to, e.g. `26.7.27`.
    pub version: String,
    pub archives: Vec<CliArchive>,
}

/// Everything the page renders.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct CliDownloadsView {
    pub firm_name: String,
    pub role: ViewerRole,
    pub logo: Option<crate::components::AppLogo>,
    /// The release tag on offer. Empty when nothing is published.
    pub version: String,
    pub archives: Vec<CliArchive>,
}

/// Resolve the injected archives, the caller's tier, and the app chrome.
///
/// Refuses a client before reading anything, so a direct hit on the generated
/// endpoint cannot enumerate what the firm ships.
#[server]
pub async fn cli_downloads_view() -> Result<CliDownloadsView, ServerFnError> {
    let role = crate::admin_listing::require_firm_person().await?;
    let injected =
        dioxus_fullstack_core::FullstackContext::extract::<axum::Extension<InjectedDownloads>, _>()
            .await
            .map_or_else(
                |_| InjectedDownloads::default(),
                |axum::Extension(downloads)| downloads,
            );

    Ok(CliDownloadsView {
        firm_name: crate::app_chrome::firm_name_from_context().await,
        role,
        logo: crate::app_chrome::app_logo_from_context().await,
        version: injected.version,
        archives: injected.archives,
    })
}

/// The platforms the page offers a card for: slug, label, and whether a release
/// publishes an archive for it.
///
/// Deliberately wider than what a release publishes. `PLATFORMS` in the portal
/// is the set with archives; this is the set a reader might be standing at, and
/// the two differ by macOS. A Mac user who finds no Mac card assumes the tool
/// does not run on their machine — it does, it is just built rather than
/// downloaded, so the card is where that instruction belongs.
///
/// The flag is what keeps the two absences apart, and they are not the same
/// thing. macOS has no archive on any release, ever, so its card offers the
/// build. Windows and Linux are published by every release, so their absence
/// means *this deployment* has not been published to — a temporary state an
/// operator fixes, not something the reader should go compile around.
const DISPLAY_PLATFORMS: &[(&str, &str, bool)] = &[
    ("linux", "Linux", true),
    ("macos", "macOS", false),
    ("windows", "Windows", true),
];

/// The CLI-download section of the team home. Split from the page so
/// [`crate::team_home`] can compose it beneath the destination cards, and so
/// tests render a fixed view without standing up the server function.
///
/// A `<section>` rather than a whole page: the team home owns the document title,
/// stylesheet, and navbar; this renders only the "Navigator CLI" block.
pub fn cli_download_section(view: &CliDownloadsView) -> Element {
    let view = view.clone();
    let has_archives = !view.archives.is_empty();
    let version = view.version.clone();

    // One card per platform a reader might be on, each resolved against what
    // this deployment actually published.
    let cards = DISPLAY_PLATFORMS.iter().map(|(slug, label, published)| {
        let archive = view.archives.iter().find(|a| a.platform == *slug).cloned();
        rsx! {
            PlatformCard {
                key: "{slug}",
                slug: (*slug).to_string(),
                label: (*label).to_string(),
                archive,
                releases_publish_it: *published,
                version: version.clone(),
            }
        }
    });

    rsx! {
        section { id: "cli-downloads", class: "team-home__section",
            header { class: "team-home__section-header",
                h2 { "Navigator CLI" }
                if has_archives {
                    p { class: "page-subtitle", "Release {view.version}" }
                }
            }
            p {
                "The command-line tool that validates notation, drives a live "
                "deployment, and runs the local environment. Install it on a "
                "machine you use for firm work."
            }
            if has_archives {
                div { class: "cli-download-cards", id: "cli-downloads-cards",
                    {cards}
                }
            } else {
                p { class: "empty-state", id: "cli-downloads-empty",
                    "No release has been published to this deployment yet."
                }
            }
            LicenceNote {}
        }
    }
}

/// One platform's card: the mark, the name, and whichever of the three states
/// this platform is actually in.
#[component]
fn PlatformCard(
    slug: String,
    label: String,
    archive: Option<CliArchive>,
    releases_publish_it: bool,
    version: String,
) -> Element {
    rsx! {
        section { class: "cli-download-card", id: "cli-card-{slug}",
            div { class: "cli-download-card__mark", PlatformMark { slug: slug.clone() } }
            h2 { class: "cli-download-card__name", "{label}" }
            match (archive, releases_publish_it) {
                // Published and present: the ordinary case, and a button.
                (Some(archive), _) => rsx! {
                    p { class: "cli-download-card__meta",
                        code { "{archive.filename}" }
                        span { class: "cli-download-card__size", "{archive.size}" }
                    }
                    a {
                        class: "cli-download-card__button",
                        id: "cli-download-{slug}",
                        href: "{archive.href}",
                        "Download for {label}"
                    }
                },
                // A release DOES publish this platform, so its absence is this
                // deployment not having been published to — an operator fixes
                // that. Telling the reader to go compile would send them around
                // a problem that is about to disappear.
                (None, true) => rsx! {
                    p { class: "cli-download-card__meta", id: "cli-pending-{slug}",
                        "The {label} archive has not been published to this "
                        "deployment yet. It ships with every release, so it will "
                        "appear here once the release finishes rolling."
                    }
                },
                // No release publishes this platform at all, so the build is the
                // real answer rather than a stopgap — and it is the same tagged
                // source the published archives are compiled from.
                (None, false) => rsx! {
                    p { class: "cli-download-card__meta",
                        "No archive is built for {label}. Install it from the "
                        "tagged source:"
                    }
                    pre { class: "cli-download-card__install",
                        code {
                            "git clone --depth 1 --branch {version} \\\n"
                            "    <navigator repo> \"$(mktemp -d /tmp/navigator.XXXXXX)/src\"\n"
                            "NAVIGATOR_RELEASE_TAG={version} \\\n"
                            "    cargo install --locked --path <that>/cli"
                        }
                    }
                },
            }
        }
    }
}

/// The platform marks, drawn inline so they inherit `currentColor` and follow
/// the theme.
///
/// These are simplified in-house glyphs — a penguin, an apple, four panes — not
/// the vendors' official brand assets, which are trademarks the Firm has no
/// licence to redistribute. They identify a platform, which is all the page
/// needs them to do.
#[component]
fn PlatformMark(slug: String) -> Element {
    match slug.as_str() {
        "linux" => rsx! {
            svg {
                class: "cli-platform-mark",
                view_box: "0 0 24 24",
                width: "40",
                height: "40",
                fill: "currentColor",
                role: "img",
                "aria-label": "Linux",
                // Tux, reduced to a body, two flippers, and a beak.
                path { d: "M12 1.6c-2.5 0-4 1.9-4 4.4 0 1.5.2 2.3-.5 3.4-.9 1.4-2.2 3-2.9 5-.5 1.4-.4 2.6.3 3.2.5.4 1.2.4 1.8.1.2.9.7 1.7 1.5 2.3 1.1.8 2.5 1.2 3.8 1.2s2.7-.4 3.8-1.2c.8-.6 1.3-1.4 1.5-2.3.6.3 1.3.3 1.8-.1.7-.6.8-1.8.3-3.2-.7-2-2-3.6-2.9-5-.7-1.1-.5-1.9-.5-3.4 0-2.5-1.5-4.4-4-4.4z" }
                circle { cx: "10.2", cy: "6.4", r: "1.1", fill: "var(--nav-color-surface)" }
                circle { cx: "13.8", cy: "6.4", r: "1.1", fill: "var(--nav-color-surface)" }
                path {
                    d: "M12 8.2c-.9 0-1.7.5-1.7 1s.8 1.1 1.7 1.1 1.7-.5 1.7-1.1-.8-1-1.7-1z",
                    fill: "var(--nav-color-warning, #e8a33d)",
                }
            }
        },
        "macos" => rsx! {
            svg {
                class: "cli-platform-mark",
                view_box: "0 0 24 24",
                width: "40",
                height: "40",
                fill: "currentColor",
                role: "img",
                "aria-label": "macOS",
                path { d: "M16.3 12.6c0-2 1.6-3 1.7-3.1-.9-1.4-2.4-1.5-2.9-1.6-1.2-.1-2.4.7-3 .7s-1.6-.7-2.6-.7c-1.3 0-2.6.8-3.2 2-1.4 2.4-.4 6 1 8 .7 1 1.5 2.1 2.5 2 1-.04 1.4-.6 2.6-.6s1.5.6 2.6.6c1.1 0 1.8-1 2.4-2 .8-1.1 1.1-2.2 1.1-2.3-.02 0-2.2-.8-2.2-3z" }
                path { d: "M14.4 6.3c.5-.7.9-1.6.8-2.5-.8 0-1.8.5-2.4 1.2-.5.6-.9 1.5-.8 2.4.9.1 1.8-.4 2.4-1.1z" }
            }
        },
        _ => rsx! {
            svg {
                class: "cli-platform-mark",
                view_box: "0 0 24 24",
                width: "40",
                height: "40",
                fill: "currentColor",
                role: "img",
                "aria-label": "Windows",
                path { d: "M3 5.6l7.4-1v7.1H3zM11.6 4.4L21 3v8.7h-9.4zM3 12.9h7.4V20L3 18.7zM11.6 12.9H21V21l-9.4-1.3z" }
            }
        },
    }
}

/// The licence the download travels under.
///
/// The archive carries `LICENSE` beside the executable and the binary prints
/// the same terms with `navigator --license`, so this note points at them rather
/// than restating them. Restating licence terms in page copy would create a
/// second, drifting version of an instrument the Firm relies on.
///
/// The one obligation the note does name is § 13, because it is the one a reader
/// will not infer from "open source": running a modified Navigator as a service
/// for other people carries a source obligation that shipping it does not.
/// Someone deciding on this page whether to deploy a fork needs that before they
/// start, not after.
#[component]
fn LicenceNote() -> Element {
    rsx! {
        section { class: "licence-note", id: "cli-downloads-licence",
            h2 { "Terms" }
            p {
                "Each archive carries the licence beside the executable, and the "
                "installed binary prints it with "
                code { "navigator --license" }
                ". Navigator is free software under the GNU Affero General "
                "Public License v3: read it, build it, fork it, and "
                "redistribute it. If you modify it and run it as a service "
                "others reach over a network, section 13 obliges you to offer "
                "those users your modified source. The NEON LAW marks are "
                "reserved and the licence does not carry them."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{cli_download_section, CliArchive, CliDownloadsView};
    use crate::people::ViewerRole;

    fn archive(platform: &str, label: &str) -> CliArchive {
        CliArchive {
            platform: platform.to_string(),
            label: label.to_string(),
            filename: format!("navigator-26.7.27-{platform}.tar.gz"),
            href: format!("/app/team/download/{platform}"),
            size: "18.4 MB".to_string(),
        }
    }

    fn view_with(archives: Vec<CliArchive>) -> CliDownloadsView {
        CliDownloadsView {
            firm_name: "Neon Law".to_string(),
            role: ViewerRole::Clerk,
            logo: None,
            version: "26.7.27".to_string(),
            archives,
        }
    }

    fn render(view: &CliDownloadsView) -> String {
        dioxus_ssr::render_element(cli_download_section(view))
    }

    #[test]
    fn every_published_archive_becomes_a_download_button() {
        let out = render(&view_with(vec![
            archive("windows", "Windows"),
            archive("linux", "Linux"),
        ]));

        for platform in ["windows", "linux"] {
            assert!(
                out.contains(&format!("/app/team/download/{platform}")),
                "the {platform} card must link to its download route: {out}"
            );
            assert!(
                out.contains(&format!("cli-download-{platform}")),
                "the {platform} card must carry its download button: {out}"
            );
        }
        assert!(
            out.contains("navigator-26.7.27-linux.tar.gz"),
            "each card names the file it delivers: {out}"
        );
        assert!(
            !out.contains("cli-downloads-empty"),
            "a page with archives must not render the empty state: {out}"
        );
    }

    /// Every platform a reader might be standing at gets a card, whether or not
    /// a release publishes an archive for it.
    #[test]
    fn all_three_platforms_get_a_card() {
        let out = render(&view_with(vec![archive("linux", "Linux")]));

        for slug in ["linux", "macos", "windows"] {
            assert!(
                out.contains(&format!("cli-card-{slug}")),
                "{slug} must get a card: {out}"
            );
        }
        for label in ["Linux", "macOS", "Windows"] {
            assert!(out.contains(label), "{label} must be named: {out}");
        }
    }

    /// macOS gets the source install, because no release builds a macOS
    /// archive and none is going to appear.
    #[test]
    fn macos_offers_the_source_install() {
        let out = render(&view_with(vec![
            archive("linux", "Linux"),
            archive("windows", "Windows"),
        ]));

        assert!(
            !out.contains("/app/team/download/macos"),
            "macOS has no archive, so it must offer no download link: {out}"
        );
        assert!(
            out.contains("cargo install --locked --path"),
            "the macOS card must carry the source install: {out}"
        );
        assert!(
            out.contains("No archive is built for macOS"),
            "the macOS card must say why there is no button: {out}"
        );
        // The install must name the release it builds, not a floating branch.
        assert!(
            out.contains("--branch 26.7.27"),
            "the source install must pin the release tag: {out}"
        );
    }

    /// A platform a release DOES publish, missing from this deployment, is a
    /// publish that has not happened — not a reason to go compile.
    ///
    /// This is the distinction that matters: Windows ships with every release,
    /// so telling a Windows user to build from source would send them around a
    /// problem that an operator is about to fix. Only macOS, which no release
    /// builds, gets the compile instruction.
    #[test]
    fn a_publishable_platform_missing_here_says_it_is_pending_not_unbuilt() {
        let out = render(&view_with(vec![archive("linux", "Linux")]));

        assert!(
            out.contains("cli-pending-windows"),
            "Windows must report a pending publish: {out}"
        );
        assert!(
            out.contains("has not been published to this deployment yet"),
            "the pending card must name the real cause: {out}"
        );
        assert!(
            !out.contains("No archive is built for Windows"),
            "Windows archives ARE built; only macOS is not: {out}"
        );
        // Exactly one source-install block on the page: macOS's.
        assert_eq!(
            out.matches("cargo install --locked --path").count(),
            1,
            "only macOS may offer the source install: {out}"
        );
    }

    /// The marks theme with the page rather than carrying fixed brand colours,
    /// and each is labelled for a screen reader.
    #[test]
    fn the_platform_marks_are_inline_and_themed() {
        let out = render(&view_with(vec![archive("linux", "Linux")]));

        assert!(
            out.matches("cli-platform-mark").count() >= 3,
            "each card needs its own inline mark: {out}"
        );
        assert!(
            out.contains("currentColor"),
            "the marks must inherit the theme colour: {out}"
        );
        for label in [
            "aria-label=\"Linux\"",
            "aria-label=\"macOS\"",
            "aria-label=\"Windows\"",
        ] {
            assert!(out.contains(label), "the mark must be labelled: {label}");
        }
    }

    /// A deployment with nothing published says so, rather than offering links
    /// that 404. A failing download button reads as a permissions problem and
    /// sends the reader to ask why they were denied.
    #[test]
    fn a_deployment_with_no_published_release_says_so() {
        let out = render(&view_with(Vec::new()));

        assert!(
            out.contains("cli-downloads-empty"),
            "no archives must render the empty state: {out}"
        );
        assert!(
            !out.contains("/app/team/download/"),
            "the empty state must offer no download link: {out}"
        );
        assert!(
            !out.contains("Release "),
            "an unpublished deployment must not claim a version: {out}"
        );
    }

    /// The page points at the shipped instruments instead of restating them, so
    /// there is only ever one version of the terms.
    #[test]
    fn the_page_points_at_the_licence_rather_than_restating_it() {
        let out = render(&view_with(vec![archive("linux", "Linux")]));

        assert!(
            out.contains("navigator --license"),
            "the terms note must name the command that prints them: {out}"
        );
    }

    /// The download page is where a stranger reads what they may do with the
    /// binary, so it has to describe the licence the binary actually carries.
    ///
    /// It once promised an end-user licence agreement and denied source,
    /// modification, and redistribution rights — the terms of a proprietary
    /// distribution. The AGPL grants all three, so that copy did not merely go
    /// stale: it told a reader they lacked a permission the licence beside the
    /// executable had already given them.
    ///
    /// § 13 is asserted alongside the grant because this page is where someone
    /// decides whether to deploy a fork, and a network-use obligation they learn
    /// about afterwards is one they have already breached.
    #[test]
    fn the_terms_note_describes_the_agpl_grant_rather_than_a_retired_eula() {
        let out = render(&view_with(vec![archive("linux", "Linux")]));

        assert!(
            out.contains("Affero"),
            "the terms note must name the licence the binary carries: {out}"
        );
        assert!(
            out.contains("section 13") && out.contains("network"),
            "the terms note must state the network-use obligation, which is the \
             one a reader will not infer from \"open source\": {out}"
        );
        for retired in ["MIT", "Apache-2.0", "dual-licensed"] {
            assert!(
                !out.contains(retired),
                "the terms note must not still offer the retired permissive \
                 grant `{retired}`: {out}"
            );
        }
        for retired in ["end-user licence", "EULA", "conveys no source"] {
            assert!(
                !out.contains(retired),
                "the terms note must not carry the retired proprietary copy \
                 ({retired:?}): {out}"
            );
        }
    }
}
