//! The `/app/team` team home — the post-login landing for every firm tier.
//!
//! A firm person (Owner, Admin, Lawyer, or Clerk) lands here after signing in;
//! a `client` is answered 403 at the route, so this page never renders for one.
//! The home is a hub, not a dashboard: it greets the person and offers a
//! role-filtered set of destination cards into the surfaces their tier may
//! reach.
//!
//! The lens is the caller's tier, resolved once by [`crate::app_chrome`] and read
//! back here. The cards are gated exactly like the navbar's
//! [`crate::app_chrome::app_destinations`]: a card is never shown for a door the
//! viewer's tier is answered 403 at, so the home advertises only what it can
//! actually open.
//!
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app_chrome::{APP_ADMIN_HREF, APP_LAWYER_HREF, APP_PROJECTS_HREF};
use crate::people::ViewerRole;

/// The `<meta description>` for the team home.
const DESCRIPTION: &str = "Your Neon Law Navigator team home.";

/// The in-app documentation hub. Not in [`crate::app_chrome::app_destinations`]
/// (it is not a navbar door), so its path is named here for the home's card. Its
/// Rego rule admits every firm tier and denies a client, the same audience as
/// this page.
const APP_DOCS_HREF: &str = "/app/docs";

/// The public Navigator workshops, linked here as a team learning resource.
const WORKSHOPS_HREF: &str = "/workshops";

/// Everything the team home renders: the viewer's tier and the mounted brand's
/// mark for the app chrome.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct TeamHomeView {
    pub role: ViewerRole,
    #[serde(default)]
    pub logo: Option<crate::components::AppLogo>,
    #[serde(default)]
    pub firm_name: String,
}

/// Resolve the authenticated viewer and the request-scoped brand for the home.
#[server]
pub async fn team_home_view() -> Result<TeamHomeView, ServerFnError> {
    Ok(TeamHomeView {
        role: crate::admin_listing::require_firm_person().await?,
        logo: crate::app_chrome::app_logo_from_context().await,
        firm_name: crate::app_chrome::firm_name_from_context().await,
    })
}

/// The route entry for `/app/team`.
#[component]
pub fn TeamHome() -> Element {
    let resource = use_server_future(team_home_view)?;

    let view = match &*resource.read() {
        Some(Ok(view)) => view.clone(),
        Some(Err(_)) => {
            return rsx! {
                main { id: "team-home", p { "Failed to load the team home." } }
            }
        }
        None => {
            return rsx! {
                main { id: "team-home", p { "Loading…" } }
            }
        }
    };

    team_home_body(&view)
}

/// One destination on the team home: a titled, described link into a firm
/// surface.
#[derive(Clone, PartialEq, Eq)]
struct Destination {
    /// The `id` on the card, so a test can pin a tier to its cards.
    id: &'static str,
    title: &'static str,
    description: &'static str,
    href: &'static str,
}

/// The destinations a viewer of `role` sees on the home, in render order.
///
/// Every firm tier reaches Matters, the Docs, and the Workshops — the base cards
/// that need no tier gate here because their own Rego rules already admit the
/// whole firm-tier audience this page is scoped to. The Workbench (from Lawyer
/// up) and Admin (from Admin up) are gated exactly like the navbar's
/// [`crate::app_chrome::app_destinations`]. Pure, so the mapping is unit-tested
/// directly.
fn destinations_for(role: ViewerRole) -> Vec<Destination> {
    let mut cards = vec![Destination {
        id: "team-card-projects",
        title: "Matters",
        description: "Every matter you can see, in one list.",
        href: APP_PROJECTS_HREF,
    }];
    if role.is_lawyer_tier() {
        cards.push(Destination {
            id: "team-card-lawyer",
            title: "Workbench",
            description: "The firm workbench: your matters' status at a glance, the \
                          calendar, and the people, entities, and notations you manage.",
            href: APP_LAWYER_HREF,
        });
    }
    if role.is_admin_tier() {
        cards.push(Destination {
            id: "team-card-admin",
            title: "Admin",
            description: "Firm administration and the full matter directory.",
            href: APP_ADMIN_HREF,
        });
    }
    cards.push(Destination {
        id: "team-card-docs",
        title: "Docs",
        description: "The in-app documentation and glossary.",
        href: APP_DOCS_HREF,
    });
    cards.push(Destination {
        id: "team-card-workshops",
        title: "Workshops",
        description: "The Navigator training classes: the lawyer workbench, the admin \
                      deployment tier, and the contribution loop.",
        href: WORKSHOPS_HREF,
    });
    cards
}

/// The loaded page. Split from the component so tests render a fixed view
/// without standing up the server function.
pub fn team_home_body(view: &TeamHomeView) -> Element {
    let role = view.role;
    let firm_name = view.firm_name.clone();

    let cards = destinations_for(role).into_iter().map(|d| {
        rsx! {
            a {
                key: "{d.id}",
                id: "{d.id}",
                class: "team-home__card",
                href: "{d.href}",
                h2 { class: "team-home__card-title", "{d.title}" }
                p { class: "team-home__card-desc", "{d.description}" }
            }
        }
    });

    rsx! {
        document::Title { "{firm_name} | Team" }
        document::Meta { name: "description", content: DESCRIPTION }
        document::Stylesheet { href: crate::components::THEME_STYLESHEET_HREF }
        crate::components::AppNavbar {
            destinations: crate::app_chrome::app_destinations(role),
            logo: view.logo.clone(),
        }
        main { id: "team-home", class: "nav-theme",
            header { class: "page-header",
                h1 { "Team home" }
                p { class: "page-subtitle",
                    "Your team together with Neon Law Navigator."
                }
            }
            nav { class: "team-home__cards", "aria-label": "Team destinations",
                {cards}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TeamHomeView;
    use super::{destinations_for, team_home_body};
    use crate::people::ViewerRole;

    fn view_for(role: ViewerRole) -> TeamHomeView {
        TeamHomeView {
            firm_name: "Neon Law".to_string(),
            role,
            logo: None,
        }
    }

    fn render(role: ViewerRole) -> String {
        dioxus_ssr::render_element(team_home_body(&view_for(role)))
    }

    fn card_ids(role: ViewerRole) -> Vec<&'static str> {
        destinations_for(role).into_iter().map(|d| d.id).collect()
    }

    /// Every firm tier gets the Matters, Docs, and Workshops cards; a clerk gets
    /// exactly those three and never the firm-workbench or admin doors.
    #[test]
    fn a_clerk_sees_only_the_shared_firm_destinations() {
        assert_eq!(
            card_ids(ViewerRole::Clerk),
            [
                "team-card-projects",
                "team-card-docs",
                "team-card-workshops"
            ]
        );
    }

    /// A lawyer gains the workbench card, but not admin.
    #[test]
    fn a_lawyer_gains_the_workbench_not_admin() {
        assert_eq!(
            card_ids(ViewerRole::Lawyer),
            [
                "team-card-projects",
                "team-card-lawyer",
                "team-card-docs",
                "team-card-workshops"
            ]
        );
    }

    /// The admin tiers gain the admin card too.
    #[test]
    fn the_admin_tiers_gain_the_admin_card() {
        for role in [ViewerRole::Admin, ViewerRole::Owner] {
            assert_eq!(
                card_ids(role),
                [
                    "team-card-projects",
                    "team-card-lawyer",
                    "team-card-admin",
                    "team-card-docs",
                    "team-card-workshops"
                ],
                "rank {}",
                role.authority_rank()
            );
        }
    }

    /// The rendered page carries the greeting and the role-filtered cards, with
    /// no CLI distribution surface.
    #[test]
    fn the_home_composes_greeting_cards_without_downloads() {
        let lawyer = render(ViewerRole::Lawyer);
        assert!(lawyer.contains("Team home"), "the greeting: {lawyer}");
        assert!(
            lawyer.contains("Your team together with Neon Law Navigator."),
            "the requested subtitle: {lawyer}"
        );
        assert!(
            lawyer.contains("team-card-lawyer"),
            "workbench card: {lawyer}"
        );
        assert!(
            !lawyer.contains("team-card-admin"),
            "a lawyer must not see the admin card: {lawyer}"
        );
        // The reference cards every firm tier reaches: the docs and the
        // firm-internal workshops.
        assert!(
            lawyer.contains(r#"href="/app/docs""#),
            "the Docs card links to the in-app docs: {lawyer}"
        );
        assert!(
            lawyer.contains(r#"href="/workshops""#),
            "the Workshops card links to the training catalog: {lawyer}"
        );
        assert!(
            !lawyer.contains("Navigator CLI"),
            "the CLI download section must be absent: {lawyer}"
        );
        assert!(
            !lawyer.contains("/app/team/download/"),
            "the team home must not publish a CLI download link: {lawyer}"
        );
    }

    /// The navbar advertises the Team home, and the destinations gate exactly as
    /// the navbar does — a lawyer's page carries the workbench door.
    #[test]
    fn the_page_navbar_carries_the_firm_doors() {
        let lawyer = render(ViewerRole::Lawyer);
        assert!(
            lawyer.contains(r#"href="/app/team""#),
            "the navbar advertises the Team home: {lawyer}"
        );
        assert!(
            lawyer.contains(r#"href="/app/lawyer""#),
            "a lawyer's navbar carries the workbench: {lawyer}"
        );
    }
}
