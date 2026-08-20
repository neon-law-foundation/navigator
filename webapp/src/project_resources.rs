//! The matter's collaboration resources — the six places work on a Project
//! actually happens, rendered as one panel on the matter page.
//!
//! Every matter travels with a private Slack channel, a private Notion page,
//! and a private Google Drive folder. Three more are optional: a Slack channel
//! shared with the client, a Notion page shared with the client, and the client
//! portal. Six links that a lawyer would otherwise keep in a bookmark folder.
//!
//! # The audience split is the whole design
//!
//! Each resource is either **firm-only** or **shared**, and the name says
//! which. That is not decoration: the private Notion page holds firm work
//! product and the private Slack channel holds lawyer-only chatter, so a client
//! who could see either would be reading the other side of their own matter.
//!
//! [`visible_resources`] is therefore the one place the split is applied, and
//! it filters by *audience*, never by "is the reader privileged". A firm-only
//! resource is dropped from the list a client is rendered from — it is not
//! rendered-then-hidden, and it is not disclosed as a withheld slot. A client
//! cannot tell a matter with no shared Notion page from one whose firm keeps
//! private notes, which is the same toggle-blindness the module ledger gets by
//! construction.
//!
//! # Who may configure them
//!
//! Reading is [`ViewerRole::is_firm_tier`] for the private half and everyone
//! for the shared half. *Writing* is [`ViewerRole::is_lawyer_tier`] — the three
//! tiers that may act on a matter. A clerk is a firm tier, so a clerk reads all
//! six and edits none, which is what makes the clerk surface read-only here for
//! the same reason it is read-only everywhere else.
//!
//! The panel renders no edit inputs of its own; configuring a resource is the
//! existing matter edit form (`/app/projects/{code}/edit`), so there is one
//! write path and one command boundary rather than a second inline one.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::resource_mark::{ResourceMark, ResourceMarkGlyph};
use crate::people::ViewerRole;

/// The host that serves a Drive folder, so the one derivation lives in one
/// place.
const DRIVE_FOLDER_PREFIX: &str = "https://drive.google.com/drive/folders/";

/// Who a resource may be shown to.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Audience {
    /// The firm only — every tier except `Client`.
    Firm,
    /// Everyone who can see the matter, the client included.
    Shared,
}

/// One of the six resources a matter carries.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResourceKind {
    /// The lawyer-only Slack channel.
    PrivateSlackChannel,
    /// The firm-only Notion page — internal write-up and working notes.
    PrivateNotionPage,
    /// The matter's folder in the firm's shared drive.
    PrivateDriveFolder,
    /// The Slack channel shared with the client, if the matter has one.
    SharedSlackChannel,
    /// The Notion page shared with the client, if the matter has one.
    SharedNotionPage,
    /// The matter's client portal — a Navigator route, not a third-party
    /// service.
    ClientPortal,
}

impl ResourceKind {
    /// Every resource, in render order: the firm's three, then the shared
    /// three. Declaration order *is* render order so the panel does not
    /// interleave audiences — a reader scanning it sees one block they may
    /// speak freely in and one block the client can read too.
    pub const ALL: &'static [ResourceKind] = &[
        ResourceKind::PrivateSlackChannel,
        ResourceKind::PrivateNotionPage,
        ResourceKind::PrivateDriveFolder,
        ResourceKind::SharedSlackChannel,
        ResourceKind::SharedNotionPage,
        ResourceKind::ClientPortal,
    ];

    /// Stable identifier, emitted as `data-resource` so a test names a row
    /// rather than matching its copy.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PrivateSlackChannel => "private-slack-channel",
            Self::PrivateNotionPage => "private-notion-page",
            Self::PrivateDriveFolder => "private-drive-folder",
            Self::SharedSlackChannel => "shared-slack-channel",
            Self::SharedNotionPage => "shared-notion-page",
            Self::ClientPortal => "client-portal",
        }
    }

    /// The row's link text.
    ///
    /// Each label says the audience out loud — "Private Slack channel", not
    /// "Slack" — because the label is the only thing standing between a lawyer
    /// and pasting client-visible material into the firm-only page, or the
    /// reverse. The vendor mark beside it names the service, so the label
    /// spends its words on the half the mark cannot show.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::PrivateSlackChannel => "Private Slack channel",
            Self::PrivateNotionPage => "Private Notion page",
            Self::PrivateDriveFolder => "Private Google Drive",
            Self::SharedSlackChannel => "Shared Slack channel",
            Self::SharedNotionPage => "Shared Notion page",
            Self::ClientPortal => "Client portal",
        }
    }

    /// Who may be shown this resource.
    #[must_use]
    pub const fn audience(self) -> Audience {
        match self {
            Self::PrivateSlackChannel | Self::PrivateNotionPage | Self::PrivateDriveFolder => {
                Audience::Firm
            }
            Self::SharedSlackChannel | Self::SharedNotionPage | Self::ClientPortal => {
                Audience::Shared
            }
        }
    }

    /// The mark that opens the row.
    #[must_use]
    pub const fn mark(self) -> ResourceMark {
        match self {
            Self::PrivateSlackChannel | Self::SharedSlackChannel => ResourceMark::Slack,
            Self::PrivateNotionPage | Self::SharedNotionPage => ResourceMark::Notion,
            Self::PrivateDriveFolder => ResourceMark::GoogleDrive,
            Self::ClientPortal => ResourceMark::Portal,
        }
    }

    /// Whether a reader of `role` may be shown this resource at all.
    #[must_use]
    pub fn visible_to(self, role: ViewerRole) -> bool {
        match self.audience() {
            Audience::Firm => role.is_firm_tier(),
            Audience::Shared => true,
        }
    }
}

/// The stored values a matter's resources are built from, exactly as the
/// `project` row holds them.
///
/// Server-side input to [`visible_resources`]; it never crosses to the client
/// build, because a firm-only URL a client may not see must not be serialised
/// into a page that client is served.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectResourceLinks {
    pub private_slack_channel_url: Option<String>,
    pub private_notion_page_url: Option<String>,
    /// The Drive **folder id**, not a URL — the browser address is derived from
    /// it, because the id is the folder's actual coordinate and the address is
    /// a fixed prefix over it. Nothing is guessed here.
    pub drive_folder_id: Option<String>,
    pub shared_slack_channel_url: Option<String>,
    pub shared_notion_page_url: Option<String>,
}

/// One resource row, resolved to what the panel renders.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ProjectResource {
    pub kind: ResourceKind,
    pub url: String,
}

/// The rendered panel — wasm-safe, and holding only rows this reader may see.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default, Debug)]
pub struct ProjectResourcesView {
    pub resources: Vec<ProjectResource>,
    /// `true` when this reader may change the configuration, which is the three
    /// lawyer tiers. Drives the "Configure" affordance only; the edit route runs
    /// its own gate.
    pub can_configure: bool,
    /// The matter code, for the configure link.
    pub project_code: String,
}

/// Resolve the resources a reader of `role` may see on this matter.
///
/// Two filters, in this order:
///
/// 1. **Audience.** A firm-only resource is not built at all for a client, so
///    its URL never reaches the rendered page — not in markup, not in a
///    hydration payload.
/// 2. **Configured.** A resource with no stored value is absent, never an empty
///    slot. An unset shared Notion page and a matter that has none look
///    identical, so a client cannot infer that something was withheld.
///
/// A blank stored string counts as unset: the command boundary clears a column
/// by writing blank, and a row whose `href` was `""` would render as a link to
/// the current page.
#[must_use]
pub fn visible_resources(
    links: &ProjectResourceLinks,
    project_code: &str,
    role: ViewerRole,
) -> Vec<ProjectResource> {
    ResourceKind::ALL
        .iter()
        .copied()
        .filter(|kind| kind.visible_to(role))
        .filter_map(|kind| {
            let url = match kind {
                ResourceKind::PrivateSlackChannel => {
                    configured(links.private_slack_channel_url.as_ref())
                }
                ResourceKind::PrivateNotionPage => {
                    configured(links.private_notion_page_url.as_ref())
                }
                ResourceKind::PrivateDriveFolder => configured(links.drive_folder_id.as_ref())
                    .map(|id| format!("{DRIVE_FOLDER_PREFIX}{id}")),
                ResourceKind::SharedSlackChannel => {
                    configured(links.shared_slack_channel_url.as_ref())
                }
                ResourceKind::SharedNotionPage => configured(links.shared_notion_page_url.as_ref()),
                // The portal is a Navigator route on this matter, so it is
                // configured by the matter existing rather than by a column.
                ResourceKind::ClientPortal => Some(format!("/app/projects/{project_code}/portal/")),
            }?;
            Some(ProjectResource { kind, url })
        })
        .collect()
}

/// A stored value that is actually set — `None` for absent *and* for blank.
fn configured(value: Option<&String>) -> Option<String> {
    value
        .map(String::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// The resources panel.
///
/// Renders nothing at all when the reader has no visible resources, rather than
/// an empty "Resources" heading — a client on a matter with no shared anything
/// should not be told there is a panel they are seeing none of.
#[component]
pub fn ProjectResourcesPanel(view: ProjectResourcesView) -> Element {
    if view.resources.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "lawyer-detail__section project-resources",
            h2 { "Resources" }
            ul { class: "project-resources__list",
                for resource in view.resources.iter() {
                    li {
                        key: "{resource.kind.name()}",
                        class: "project-resources__row",
                        "data-resource": resource.kind.name(),
                        a {
                            class: "project-resources__link",
                            href: "{resource.url}",
                            // The portal is an in-app route, so it opens in
                            // place; a third-party service opens beside the
                            // matter rather than navigating away from it.
                            target: if resource.kind == ResourceKind::ClientPortal { "" } else { "_blank" },
                            rel: if resource.kind == ResourceKind::ClientPortal { "" } else { "noopener noreferrer" },
                            ResourceMarkGlyph {
                                mark: resource.kind.mark(),
                                class: "project-resources__mark".to_string(),
                            }
                            span { class: "project-resources__label", "{resource.kind.label()}" }
                        }
                    }
                }
            }
            if view.can_configure {
                p { class: "project-resources__configure",
                    a {
                        class: "nav-link",
                        href: "/app/projects/{view.project_code}/edit",
                        "Configure resources"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        visible_resources, Audience, ProjectResourceLinks, ProjectResourcesPanel,
        ProjectResourcesView, ResourceKind,
    };
    use crate::people::ViewerRole;
    use dioxus::prelude::*;

    /// Every resource configured, so a filter that fails open is visible as a
    /// row that should not be there.
    fn every_link() -> ProjectResourceLinks {
        ProjectResourceLinks {
            private_slack_channel_url: Some(
                "https://neonlaw.slack.com/archives/C0PRIVATE".to_string(),
            ),
            private_notion_page_url: Some(
                "https://www.notion.so/neonlaw/Private-abc123".to_string(),
            ),
            drive_folder_id: Some("1QaBcD_2-efG".to_string()),
            shared_slack_channel_url: Some(
                "https://neonlaw.slack.com/archives/C0SHARED".to_string(),
            ),
            shared_notion_page_url: Some("https://www.notion.so/neonlaw/Shared-def456".to_string()),
        }
    }

    fn names(role: ViewerRole) -> Vec<&'static str> {
        visible_resources(&every_link(), "sample-litigation", role)
            .into_iter()
            .map(|r| r.kind.name())
            .collect()
    }

    fn render(view: ProjectResourcesView) -> String {
        dioxus_ssr::render_element(rsx! { ProjectResourcesPanel { view } })
    }

    /// The firm tiers see all six. Clerk is included deliberately: a supervised
    /// non-lawyer works for the firm and reads the matter's working surfaces.
    #[test]
    fn every_firm_tier_sees_all_six() {
        for role in [
            ViewerRole::Owner,
            ViewerRole::Admin,
            ViewerRole::Lawyer,
            ViewerRole::Clerk,
        ] {
            assert_eq!(
                names(role),
                [
                    "private-slack-channel",
                    "private-notion-page",
                    "private-drive-folder",
                    "shared-slack-channel",
                    "shared-notion-page",
                    "client-portal",
                ],
                "{:?} should see every resource",
                role.authority_rank()
            );
        }
    }

    /// **The confidentiality boundary.** A client sees the three shared
    /// resources and none of the firm's — the private Notion page is firm work
    /// product and the private Slack channel is lawyer-only chatter.
    #[test]
    fn a_client_sees_only_the_shared_resources() {
        assert_eq!(
            names(ViewerRole::Client),
            [
                "shared-slack-channel",
                "shared-notion-page",
                "client-portal"
            ]
        );
    }

    /// The stronger form of the same rule: no firm-only *URL* reaches a
    /// client's page, so the filter cannot be defeated by a render that leaks
    /// an href it decided not to label.
    #[test]
    fn no_firm_only_url_is_built_for_a_client() {
        let rendered = render(ProjectResourcesView {
            resources: visible_resources(&every_link(), "sample-litigation", ViewerRole::Client),
            can_configure: false,
            project_code: "sample-litigation".to_string(),
        });
        for firm_only in [
            "C0PRIVATE",
            "Private-abc123",
            "1QaBcD_2-efG",
            "drive.google.com",
        ] {
            assert!(
                !rendered.contains(firm_only),
                "a client's page leaked `{firm_only}`: {rendered}"
            );
        }
        assert!(rendered.contains("C0SHARED"), "{rendered}");
    }

    /// An unset resource is absent, not an empty row — that is what keeps a
    /// client from inferring a withheld page.
    #[test]
    fn an_unset_resource_does_not_render_a_slot() {
        let resources = visible_resources(
            &ProjectResourceLinks::default(),
            "sample-litigation",
            ViewerRole::Lawyer,
        );
        // Only the portal, which the matter's existence configures.
        assert_eq!(
            resources.iter().map(|r| r.kind.name()).collect::<Vec<_>>(),
            ["client-portal"]
        );
    }

    /// A blank column is unset. Clearing a resource writes blank, and a row
    /// with an empty `href` would link to the page it is on.
    #[test]
    fn a_blank_value_counts_as_unset() {
        let links = ProjectResourceLinks {
            private_notion_page_url: Some("   ".to_string()),
            ..ProjectResourceLinks::default()
        };
        let resources = visible_resources(&links, "sample-litigation", ViewerRole::Lawyer);
        assert!(
            !resources
                .iter()
                .any(|r| r.kind == ResourceKind::PrivateNotionPage),
            "a blank page is not a resource"
        );
    }

    /// The Drive address is derived from the stored folder id, which is the
    /// folder's real coordinate.
    #[test]
    fn the_drive_row_is_derived_from_the_folder_id() {
        let resource = visible_resources(&every_link(), "sample-litigation", ViewerRole::Lawyer)
            .into_iter()
            .find(|r| r.kind == ResourceKind::PrivateDriveFolder)
            .expect("drive row");
        assert_eq!(
            resource.url,
            "https://drive.google.com/drive/folders/1QaBcD_2-efG"
        );
    }

    /// The portal points at this matter's mount, keyed on the code.
    #[test]
    fn the_portal_row_points_at_this_matters_mount() {
        let resource = visible_resources(&every_link(), "kizuna", ViewerRole::Client)
            .into_iter()
            .find(|r| r.kind == ResourceKind::ClientPortal)
            .expect("portal row");
        assert_eq!(resource.url, "/app/projects/kizuna/portal/");
    }

    /// Reading and writing are different questions: a clerk reads every
    /// resource and configures none.
    #[test]
    fn only_the_lawyer_tiers_are_offered_the_configure_link() {
        for (role, offered) in [
            (ViewerRole::Owner, true),
            (ViewerRole::Admin, true),
            (ViewerRole::Lawyer, true),
            (ViewerRole::Clerk, false),
            (ViewerRole::Client, false),
        ] {
            let html = render(ProjectResourcesView {
                resources: visible_resources(&every_link(), "sample-litigation", role),
                can_configure: role.is_lawyer_tier(),
                project_code: "sample-litigation".to_string(),
            });
            assert_eq!(
                html.contains("Configure resources"),
                offered,
                "configure affordance wrong for rank {}",
                role.authority_rank()
            );
        }
    }

    /// No visible resources means no panel, rather than a heading over nothing.
    #[test]
    fn an_empty_panel_renders_nothing() {
        let html = render(ProjectResourcesView::default());
        assert!(!html.contains("Resources"), "{html}");
    }

    /// The audience split is declared once and every kind is classified, so a
    /// seventh resource cannot be added without choosing a side.
    #[test]
    fn every_resource_declares_an_audience() {
        let firm = ResourceKind::ALL
            .iter()
            .filter(|k| k.audience() == Audience::Firm)
            .count();
        assert_eq!(firm, 3, "three firm-only resources");
        assert_eq!(ResourceKind::ALL.len(), 6);
        // The name each kind carries is unique, or `data-resource` could not
        // identify a row.
        let mut names: Vec<_> = ResourceKind::ALL.iter().map(|k| k.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "resource names must be unique");
    }

    /// A third-party link opens beside the matter; the in-app portal does not.
    #[test]
    fn only_third_party_links_open_in_a_new_tab() {
        let html = render(ProjectResourcesView {
            resources: visible_resources(&every_link(), "sample-litigation", ViewerRole::Lawyer),
            can_configure: true,
            project_code: "sample-litigation".to_string(),
        });
        assert!(html.contains(r#"rel="noopener noreferrer""#), "{html}");
        // Every vendor mark is present, which is how a reader tells the rows
        // apart at a glance.
        for mark in ["slack", "notion", "google-drive", "portal"] {
            assert!(
                html.contains(&format!(r#"data-resource-mark="{mark}""#)),
                "missing the {mark} mark: {html}"
            );
        }
    }
}
