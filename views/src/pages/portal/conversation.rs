//! The matter's privileged conversation log, rendered at
//! `GET /portal/projects/:id/conversation`.
//!
//! One thread for the whole matter: document comments, inbound and outbound
//! email, and portal messages interleaved in time — the back-and-forth the
//! firm has with the client, no matter which door it came through. Every
//! entry is attorney-client privileged; the firm sees the whole thread, a
//! client sees everything except firm-internal notes (the handler picks the
//! query, the template never has to decide).
//!
//! The composer posts a portal message; staff can flag a message as an
//! internal note. The form `hx-post`s and swaps the refreshed thread fragment
//! back in, falling back to a normal POST when HTMX is absent.

use maud::{html, Markup};
use uuid::Uuid;

use crate::PageLayout;

/// One entry in the conversation, already resolved to display strings by the
/// handler (author name, channel/direction labels).
pub struct MessageRow<'a> {
    /// Channel literal (`document_comment`, `email_inbound`, …) — used only
    /// to pick a human label.
    pub channel: &'a str,
    /// Direction literal (`inbound`, `outbound`, `internal`).
    pub direction: &'a str,
    /// Display name of who wrote it (person name, counterparty address, or a
    /// firm/system label).
    pub author: &'a str,
    /// Optional subject line.
    pub subject: Option<&'a str>,
    /// Message body.
    pub body: &'a str,
    /// When it happened (display string).
    pub occurred_at: &'a str,
}

impl MessageRow<'_> {
    /// A short, human label for the channel.
    fn channel_label(&self) -> &'static str {
        match self.channel {
            "document_comment" => "Comment",
            "email_inbound" | "email_outbound" => "Email",
            "portal_message" => "Message",
            "sms_inbound" | "sms_outbound" => "Text",
            _ => "Note",
        }
    }

    /// Bootstrap contextual class keyed on direction — internal notes stand
    /// apart so staff never mistake one for a client-visible message.
    fn direction_class(&self) -> &'static str {
        match self.direction {
            "inbound" => "border-start border-4 border-primary",
            "outbound" => "border-start border-4 border-success",
            _ => "border-start border-4 border-warning bg-body-tertiary",
        }
    }
}

pub struct Thread<'a> {
    pub project_id: Uuid,
    pub project_name: &'a str,
    pub messages: &'a [MessageRow<'a>],
    /// `true` when the viewer is staff/admin — shows the "internal note"
    /// toggle on the composer.
    pub is_staff: bool,
    pub csrf_token: &'a str,
}

/// Just the message list — the fragment HTMX swaps in after a post.
#[must_use]
pub fn render_fragment(t: &Thread<'_>) -> Markup {
    html! {
        div id="conversation-thread" {
            @if t.messages.is_empty() {
                p."text-body-secondary" { "No messages yet." }
            } @else {
                div."d-flex flex-column gap-3" {
                    @for m in t.messages {
                        div.(format!("card {}", m.direction_class())) {
                            div."card-body py-2" {
                                div."d-flex justify-content-between align-items-center mb-1" {
                                    span {
                                        strong { (m.author) }
                                        span."badge text-bg-light text-uppercase ms-2" { (m.channel_label()) }
                                        @if m.direction == "internal" {
                                            span."badge text-bg-warning text-uppercase ms-1" { "Internal" }
                                        }
                                    }
                                    span."text-body-secondary small" { (m.occurred_at) }
                                }
                                @if let Some(subject) = m.subject {
                                    div."fw-semibold small" { (subject) }
                                }
                                div."mt-1" style="white-space: pre-wrap" { (m.body) }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[must_use]
pub fn render(t: &Thread<'_>) -> Markup {
    let post_url = format!("/portal/projects/{}/conversation/messages", t.project_id);
    let body = html! {
        section."portal portal-conversation" {
            nav."mb-3" {
                a href=(format!("/portal/projects/{}", t.project_id)) { "← Back to matter" }
            }
            h1."mb-1" { "Conversation" }
            p."text-body-secondary mb-4" { (t.project_name) }

            (render_fragment(t))

            section."mt-4" {
                h2."h5 mb-3" { "Add a message" }
                form method="post" action=(post_url)
                    hx-post=(post_url) hx-target="#conversation-thread" hx-swap="outerHTML" {
                    input type="hidden" name="_csrf" value=(t.csrf_token);
                    div."mb-2" {
                        textarea."form-control" name="body" rows="3"
                            placeholder="Write a message…" required {}
                    }
                    @if t.is_staff {
                        div."form-check mb-2" {
                            input."form-check-input" type="checkbox" name="internal" value="1" id="internal-note";
                            label."form-check-label" for="internal-note" {
                                "Internal note (not visible to the client)"
                            }
                        }
                    }
                    button."btn btn-primary" type="submit" { "Send" }
                }
            }
        }
    };
    PageLayout::new("Conversation")
        .with_description("Your privileged conversation with the firm on this matter.")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render, render_fragment, MessageRow, Thread};
    use uuid::Uuid;

    fn thread<'a>(messages: &'a [MessageRow<'a>], is_staff: bool) -> Thread<'a> {
        Thread {
            project_id: Uuid::now_v7(),
            project_name: "Libra estate plan",
            messages,
            is_staff,
            csrf_token: "tok",
        }
    }

    #[test]
    fn renders_interleaved_messages_with_channel_labels() {
        let messages = [
            MessageRow {
                channel: "email_inbound",
                direction: "inbound",
                author: "Libra",
                subject: Some("Question"),
                body: "Here is my info.",
                occurred_at: "2026-06-08T09:00:00Z",
            },
            MessageRow {
                channel: "document_comment",
                direction: "outbound",
                author: "Nick",
                subject: None,
                body: "Good point — fixed.",
                occurred_at: "2026-06-08T10:00:00Z",
            },
        ];
        let html = render(&thread(&messages, true)).into_string();
        assert!(html.contains("Here is my info."));
        assert!(html.contains("Good point — fixed."));
        assert!(html.contains("Email"));
        assert!(html.contains("Comment"));
        assert!(html.contains("conversation-thread"));
    }

    #[test]
    fn staff_sees_internal_toggle_client_does_not() {
        let staff_html = render(&thread(&[], true)).into_string();
        assert!(staff_html.contains("Internal note"));
        let client_html = render(&thread(&[], false)).into_string();
        assert!(!client_html.contains("Internal note"));
    }

    #[test]
    fn internal_messages_are_badged() {
        let messages = [MessageRow {
            channel: "portal_message",
            direction: "internal",
            author: "Nick",
            subject: None,
            body: "strategy",
            occurred_at: "2026-06-08T10:00:00Z",
        }];
        let frag = render_fragment(&thread(&messages, true)).into_string();
        assert!(frag.contains("Internal"));
    }
}
