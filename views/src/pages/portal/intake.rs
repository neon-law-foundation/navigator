//! Client self-serve intake — the magic-link surface where a client
//! answers (or confirms) the client-facing questions on a notation.
//!
//! The demand-side sibling of the admin walker
//! ([`crate::pages::admin::retainers::question_step`]): one question per
//! step, pre-filled with anything staff already entered on the client's
//! behalf and editable, with plain "here's what you're confirming"
//! framing. The page is mobile-first and saves per step, so a drop-off
//! resumes where the client left off.

use maud::{html, Markup};
use uuid::Uuid;

use crate::components::form::{Field, FormCard};
use crate::PageLayout;

/// One client-facing intake step.
pub struct IntakeStep<'a> {
    pub project_id: Uuid,
    pub notation_id: Uuid,
    /// The bound template's title (e.g. "Retainer Agreement") — names
    /// what the client is filling in.
    pub flow_label: &'a str,
    /// Question.code the form is asking — the POST's question key.
    pub question_code: &'a str,
    /// Human-readable prompt rendered as the form label.
    pub question_prompt: &'a str,
    /// `string`, `text`, `int`, `bool`, … — selects the input shape.
    pub answer_type: &'a str,
    /// Any current answer to pre-fill — including one staff entered on
    /// the client's behalf, which the client confirms or corrects.
    pub prior_value: Option<&'a str>,
    /// `(current, total)` — client-facing progress.
    pub progress: (usize, usize),
    pub csrf_token: &'a str,
    pub error: Option<&'a str>,
}

#[must_use]
pub fn intake_step(view: &IntakeStep<'_>) -> Markup {
    let (current, total) = view.progress;
    let prior = view.prior_value.unwrap_or("");
    let action = format!(
        "/portal/projects/{}/intake/{}",
        view.project_id, view.notation_id
    );
    let title = format!("{} — step {current} of {total}", view.flow_label);
    let page_title = format!("Your {} — Neon Law Navigator", view.flow_label);
    // `people_list` is a composite widget (several inputs assembled by
    // the POST handler into one JSON answer); everything else is one
    // `value` control.
    let (fields, extra) = match view.answer_type {
        "people_list" => (
            Vec::new(),
            Some(crate::components::people_list_inputs(prior, 3)),
        ),
        "text" => (
            vec![Field::textarea(view.question_prompt, "value", prior, 4).required()],
            None,
        ),
        "int" => (
            vec![Field::number(view.question_prompt, "value", prior).required()],
            None,
        ),
        "bool" => (
            vec![Field::checkbox(
                view.question_prompt,
                "value",
                "true",
                prior == "true",
            )],
            None,
        ),
        _ => (
            vec![Field::text(view.question_prompt, "value", prior).required()],
            None,
        ),
    };
    let cancel = format!("/portal/projects/{}", view.project_id);
    let mut card = FormCard::new(&title, &action, "Save and continue")
        .intro(html! {
            @if view.answer_type == "people_list" {
                p."mb-2" { (view.question_prompt) }
            }
            "Your legal team started this for you. Confirm what's here or "
            "fix anything that's wrong, then continue — your answers save as "
            "you go, so you can finish later if you need to."
        })
        .fields(fields)
        .csrf(view.csrf_token)
        .error(view.error)
        .cancel_labeled(&cancel, "Finish later");
    if let Some(extra) = extra {
        card = card.extra_fields(extra);
    }
    let body = html! {
        section."portal" {
            div.container {
                (card.render())
            }
        }
    };
    PageLayout::new(&page_title)
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// The "you're done with your part" landing once the client has answered
/// every client-facing question.
pub struct IntakeComplete<'a> {
    pub project_id: Uuid,
    pub flow_label: &'a str,
    /// Count of client-facing questions the client answered.
    pub total: usize,
}

#[must_use]
pub fn intake_complete(view: &IntakeComplete<'_>) -> Markup {
    let page_title = format!("Your {} — Neon Law Navigator", view.flow_label);
    let back = format!("/portal/projects/{}", view.project_id);
    let body = html! {
        section."portal" {
            div.container {
                div."card p-4" {
                    h1."mb-2" { "Thank you — your part is done" }
                    p."text-body-secondary mb-3" {
                        "You've answered everything we needed from you for your "
                        (view.flow_label) ". Your legal team will finish the rest, "
                        "review it, and send you the final document to sign. Nothing "
                        "goes out until an attorney has reviewed it."
                    }
                    a.btn."btn-primary" href=(back) { "Back to your matter" }
                }
            }
        }
    };
    PageLayout::new(&page_title)
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}
