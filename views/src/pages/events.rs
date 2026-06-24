//! `/events` — public event index and event detail pages.

use maud::{html, Markup, PreEscaped};

use crate::{AuthState, PageLayout};

pub struct EventSummary<'a> {
    pub slug: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub time: &'a str,
    pub place: &'a str,
}

pub struct EventContent<'a> {
    pub slug: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub time: &'a str,
    pub place: &'a str,
    pub external_event_provider: &'a str,
    pub external_event_url: &'a str,
    pub ics_url: &'a str,
    pub body_html: &'a str,
    pub video_url: Option<&'a str>,
    pub recap_url: Option<&'a str>,
}

#[must_use]
pub fn render_index(events: &[EventSummary<'_>], auth: AuthState) -> Markup {
    let body = html! {
        article {
            h1 { "Events" }
            p {
                "Open gatherings for legal professionals to trade practical workflows, "
                "show-and-tells, and ideas that improve access."
            }
            @if events.is_empty() {
                section.empty-state {
                    p { "No events are scheduled yet. Check back soon." }
                }
            } @else {
                @for event in events {
                    article.blog-post-summary {
                        h2 { a href=(format!("/events/{}", event.slug)) { (event.title) } }
                        p.blog-date { small { (event.time) " · " (event.place) } }
                        p { (event.description) }
                        p { a href=(format!("/events/{}", event.slug)) { "View event →" } }
                    }
                }
            }
        }
    };
    PageLayout::new("Events")
        .with_description("Open legal technology events from Neon Law.")
        .with_auth(auth)
        .render(&body)
}

#[must_use]
pub fn render_event(event: &EventContent<'_>, auth: AuthState) -> Markup {
    let body = html! {
        article.blog-post style="max-width: 65ch; margin-inline: auto;" {
            p { a href="/events" { "← All events" } }
            h1 { (event.title) }
            p.blog-date { small { (event.time) " · " (event.place) } }
            p {
                a.btn.btn-primary href=(event.external_event_url) {
                    "RSVP on " (provider_label(event.external_event_provider))
                }
                " "
                a.btn.btn-outline-secondary href=(event.ics_url) { "Add to calendar" }
            }
            (PreEscaped(event.body_html))
            @if let Some(video_url) = event.video_url {
                h2 { "Video" }
                p { a href=(video_url) { "Watch the show-and-tell" } }
            }
            @if let Some(recap_url) = event.recap_url {
                h2 { "Recap" }
                p { a href=(recap_url) { "Read the event recap" } }
            }
        }
    };
    PageLayout::new(event.title)
        .with_description(if event.description.is_empty() {
            "An event from Neon Law."
        } else {
            event.description
        })
        .with_auth(auth)
        .render(&body)
}

fn provider_label(provider: &str) -> &str {
    if provider.eq_ignore_ascii_case("luma") {
        "Luma"
    } else {
        "event page"
    }
}

#[cfg(test)]
mod tests {
    use super::{render_event, render_index, EventContent, EventSummary};
    use crate::brand::FIRM_BRAND;

    #[test]
    fn index_links_each_event_by_slug() {
        let events = vec![EventSummary {
            slug: "seattle-agentic-workflows-for-lawyers",
            title: "Agentic Workflows for Lawyers",
            description: "A practical AI workflow gathering.",
            time: "July 2, 2026, 11:00 AM-3:00 PM Pacific",
            place: "Private lounge, downtown Seattle",
        }];
        let html = render_index(&events, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("href=\"/events/seattle-agentic-workflows-for-lawyers\""));
        assert!(html.contains("Agentic Workflows for Lawyers"));
        assert!(!html.contains("No events are scheduled"));
    }

    #[test]
    fn event_renders_rsvp_calendar_and_video_links() {
        let event = EventContent {
            slug: "seattle-agentic-workflows-for-lawyers",
            title: "Agentic Workflows for Lawyers",
            description: "A practical AI workflow gathering.",
            time: "July 2, 2026, 11:00 AM-3:00 PM Pacific",
            place: "Private lounge, downtown Seattle",
            external_event_provider: "luma",
            external_event_url: "https://luma.com/k26256ut",
            ics_url: "/events/seattle-agentic-workflows-for-lawyers/calendar.ics",
            body_html: "<p>Body.</p>",
            video_url: Some("https://example.com/video"),
            recap_url: None,
        };
        let html = render_event(&event, crate::AuthState::Anonymous).into_string();
        assert!(html.contains(&format!(
            "<title>{} | Agentic Workflows for Lawyers</title>",
            FIRM_BRAND.site_name
        )));
        assert!(html.contains("RSVP on Luma"));
        assert!(
            html.contains("href=\"/events/seattle-agentic-workflows-for-lawyers/calendar.ics\"")
        );
        assert!(html.contains("Watch the show-and-tell"));
        assert!(html.contains("Body."));
    }
}
