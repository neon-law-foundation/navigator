//! Admin /retainers pages — the "start a walk" form, the per-step
//! questionnaire walker ([`question_step`]), and the post-walk
//! landing.

use maud::{html, Markup};
use uuid::Uuid;

use crate::components::form::{Choice, Field, FormCard};
use crate::PageLayout;

/// "Start a stepwise retainer walk" form — collects the minimum
/// to create a Notation. The walker collects everything else
/// (`client_name`, `project_name`, `product_description`) one
/// question at a time.
///
/// `templates` is the list of selectable onboarding templates as
/// `(code, label)` pairs — the handler restricts it to the
/// `onboarding__*` family, so opening a matter always starts it with a
/// retainer-type notation.
#[derive(Default)]
pub struct StartWalk<'a> {
    pub client_email: &'a str,
    pub retainer_template_code: &'a str,
    pub templates: &'a [(String, String)],
    pub csrf_token: &'a str,
    pub error: Option<&'a str>,
}

#[must_use]
pub fn start_walk(form: &StartWalk<'_>) -> Markup {
    // A matter opens on an onboarding template; the dropdown is the
    // canonical picker so staff choose "Neon Law Nest" rather than typing
    // `onboarding__retainer_nest`. Default the selection to the retainer.
    let selected = if form.retainer_template_code.is_empty() {
        "onboarding__retainer"
    } else {
        form.retainer_template_code
    };
    let options: Vec<Choice<'_>> = form
        .templates
        .iter()
        .map(|(code, label)| Choice::new(code, label))
        .collect();
    let fields = vec![
        Field::email("Client email", "client_email", form.client_email)
            .required()
            .placeholder("libra@example.com"),
        Field::select(
            "Onboarding template",
            "retainer_template_code",
            options,
            Some(selected),
        )
        .required()
        .help("Every matter opens on an onboarding template — the client's retainer for this product."),
    ];
    let body = html! {
        section.admin {
            div.container {
                (FormCard::new("New retainer", "/portal/admin/retainers/new", "Start walk")
                    .intro(html! {
                        "Creates a Notation and walks the questionnaire one "
                        "question at a time. Once the questionnaire reaches "
                        code { "END" } ", the retainer-intake workflow takes "
                        "over (intake → render → signature)."
                    })
                    .fields(fields)
                    .csrf(form.csrf_token)
                    .error(form.error)
                    .cancel("/portal/admin")
                    .render())
            }
        }
    };
    PageLayout::new("New retainer — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

pub struct IntakeResult<'a> {
    pub notation_id: Uuid,
    pub workflow_state: &'a str,
    /// `Some(envelope_id)` once the document is sent for signature;
    /// `None` when the notation is parked at `staff_review` awaiting
    /// attorney approval because it carries custom content (a custom
    /// clause or a client-entered answer).
    pub signature_request_id: Option<&'a str>,
    pub rendered: Markup,
    pub csrf_token: &'a str,
}

#[must_use]
pub fn result(view: &IntakeResult<'_>) -> Markup {
    // Three phases of the durable send, keyed off the workflow state and
    // whether an envelope has gone out:
    //   - `staff_review`, no envelope        → approve (renders + parks)
    //   - `document_open__*`, no envelope     → document rendering; send
    //   - envelope present                    → sent for signature
    let awaiting_review =
        view.signature_request_id.is_none() && view.workflow_state == "staff_review";
    let ready_to_send =
        view.signature_request_id.is_none() && view.workflow_state.starts_with("document_open__");
    let heading = if awaiting_review {
        "Awaiting attorney review"
    } else if ready_to_send {
        "Document rendering — ready to send"
    } else {
        "Retainer intake started"
    };
    let approve_send = format!("/portal/admin/notations/{}/approve-send", view.notation_id);
    let send = format!("/portal/admin/notations/{}/send", view.notation_id);
    let body = html! {
        section.admin {
            div.container {
                h1 { (heading) }
                @if awaiting_review {
                    p."text-body-secondary" {
                        "This matter carries custom content — a custom clause or an answer "
                        "the client entered themselves — so it parks here for an attorney. "
                        "The exact document below is what gets signed: review it, then "
                        "approve and send."
                    }
                }
                @if ready_to_send {
                    p."text-body-secondary" {
                        "Approved. The document is rendering for signature. Once it is "
                        "ready, send it — the binding envelope goes out only on this "
                        "deliberate step."
                    }
                }
                dl.admin-summary {
                    dt { "Notation id" }
                    dd { (view.notation_id) }
                    dt { "Workflow state" }
                    dd { code { (view.workflow_state) } }
                    dt { "Signature request" }
                    dd { code { (view.signature_request_id.unwrap_or("—")) } }
                }
                @if awaiting_review {
                    form.mb-3 method="post" action=(approve_send) aria-label="Approve and send for signature" {
                        input type="hidden" name="_csrf" value=(view.csrf_token);
                        button."btn btn-primary" type="submit" {
                            "Approve and send for signature"
                        }
                    }
                }
                @if ready_to_send {
                    form.mb-3 method="post" action=(send) aria-label="Send for signature" {
                        input type="hidden" name="_csrf" value=(view.csrf_token);
                        button."btn btn-primary" type="submit" {
                            "Send for signature"
                        }
                    }
                }
                h2 { "Rendered document" }
                (view.rendered)
                p { a href="/portal/admin/retainers/intake" { "Start another intake" } }
            }
        }
    };
    PageLayout::new("Retainer intake — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// One-question-at-a-time walker step. Renders the current
/// question's prompt and a single-field form whose POST target is
/// `/portal/admin/notations/:id/step` (the questionnaire walker handler).
pub struct QuestionStep<'a> {
    pub notation_id: Uuid,
    /// Human label for the flow being walked, derived from the bound
    /// template (e.g. "Retainer intake", "Closing letter"). The walker
    /// is generic over any notation, so the chrome must name the
    /// actual template — not assume the retainer.
    pub flow_label: &'a str,
    /// Question.code the form is asking — used as the row's
    /// payload key when the POST writes the Answer.
    pub question_code: &'a str,
    /// Human-readable prompt rendered as the form label.
    pub question_prompt: &'a str,
    /// `string`, `text`, `int`, `bool`, … — selects the input
    /// element shape.
    pub answer_type: &'a str,
    /// Prior answer to pre-fill (e.g. the user navigated back).
    pub prior_answer: Option<&'a str>,
    /// `(current, total)` — staff-visible progress indicator.
    pub progress: (usize, usize),
    pub csrf_token: &'a str,
    pub error: Option<&'a str>,
}

#[must_use]
pub fn question_step(view: &QuestionStep<'_>) -> Markup {
    let (current, total) = view.progress;
    let prior = view.prior_answer.unwrap_or("");
    let action = format!("/portal/admin/notations/{}/step", view.notation_id);
    let title = format!("{} — step {current} of {total}", view.flow_label);
    let page_title = format!("{} — Admin", view.flow_label);
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
        "datetime" => (
            vec![Field::input(view.question_prompt, "value", prior, "datetime-local").required()],
            None,
        ),
        "custom_usd" => (
            vec![Field::input(view.question_prompt, "value", prior, "number")
                .prefix("$")
                .step("0.01")
                .placeholder("0.00")
                .help("Enter dollars and cents, e.g. 1250.00.")
                .required()],
            None,
        ),
        "bool" | "yes_no" => (
            vec![Field::checkbox(
                view.question_prompt,
                "value",
                "true",
                prior == "true",
            )],
            None,
        ),
        // string / unknown: fall back to a single-line text input.
        _ => (
            vec![Field::text(view.question_prompt, "value", prior).required()],
            None,
        ),
    };
    let mut card = FormCard::new(&title, &action, "Continue")
        .intro(html! {
            "Question " code { (view.question_code) } "."
            @if view.answer_type == "people_list" {
                p."mt-2"."mb-0" { (view.question_prompt) }
            }
        })
        .fields(fields)
        .csrf(view.csrf_token)
        .error(view.error)
        .cancel_labeled("/portal/admin", "Save and exit");
    if let Some(extra) = extra {
        card = card.extra_fields(extra);
    }
    let send_intake = format!("/portal/admin/notations/{}/send-intake", view.notation_id);
    let body = html! {
        section.admin {
            div.container {
                (card.render())
                // Hand off to the client: they answer the client-facing
                // questions themselves, pre-filled with anything entered
                // here, and both authorships interleave on this notation.
                form.mt-3 method="post" action=(send_intake) aria-label="Send the client their intake link" {
                    input type="hidden" name="_csrf" value=(view.csrf_token);
                    button.btn."btn-outline-secondary".btn-sm type="submit" {
                        "Send the client their intake link"
                    }
                }
                // Add per-matter custom prose before sending — any clause
                // routes the document back through attorney review.
                p.mt-2 {
                    a href=(format!("/portal/admin/notations/{}/clauses", view.notation_id)) {
                        "Add custom clauses to this matter →"
                    }
                }
            }
        }
    };
    PageLayout::new(&page_title)
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{question_step, result, start_walk, IntakeResult, QuestionStep, StartWalk};
    use maud::PreEscaped;
    use uuid::Uuid;

    const ID1: Uuid = Uuid::from_u128(1);
    const ID42: Uuid = Uuid::from_u128(42);

    #[test]
    fn start_walk_renders_minimal_create_fields_and_action() {
        let html = start_walk(&StartWalk::default()).into_string();
        assert!(html.contains("action=\"/portal/admin/retainers/new\""));
        assert!(html.contains("name=\"client_email\""));
        assert!(html.contains("name=\"retainer_template_code\""));
        // The walker collects these — they must NOT be on the
        // create form anymore.
        assert!(!html.contains("name=\"client_name\""));
        assert!(!html.contains("name=\"project_name\""));
        assert!(!html.contains("name=\"product_description\""));
        assert!(html.contains(">Start walk</button>"));
    }

    #[test]
    fn start_walk_shows_error_when_provided() {
        let html = start_walk(&StartWalk {
            error: Some("template `foo` not found"),
            ..StartWalk::default()
        })
        .into_string();
        assert!(html.contains("template `foo` not found"));
    }

    #[test]
    fn start_walk_includes_csrf_when_token_present() {
        let html = start_walk(&StartWalk {
            csrf_token: "TOKEN_VALUE",
            ..StartWalk::default()
        })
        .into_string();
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"TOKEN_VALUE\""));
    }

    #[test]
    fn question_step_renders_prompt_progress_and_post_target() {
        let html = question_step(&QuestionStep {
            notation_id: ID42,
            flow_label: "Closing letter",
            question_code: "client_email",
            question_prompt: "What is the client's email address?",
            answer_type: "string",
            prior_answer: None,
            progress: (2, 4),
            csrf_token: "",
            error: None,
        })
        .into_string();
        assert!(html.contains(&format!("action=\"/portal/admin/notations/{ID42}/step\"")));
        assert!(html.contains("client_email"));
        assert!(html.contains("step 2 of 4"));
        // The chrome is template-driven, not hard-coded to the retainer.
        assert!(html.contains("Closing letter — step 2 of 4"));
        // maud escapes the apostrophe; just check the surrounding
        // words so we don't pin the exact entity form.
        assert!(html.contains("What is the client"));
        assert!(html.contains("email address?"));
        assert!(html.contains(">Continue</button>"));
        assert!(html.contains("type=\"text\""));
    }

    #[test]
    fn question_step_pre_fills_prior_answer_for_back_button_display() {
        let html = question_step(&QuestionStep {
            notation_id: ID1,
            flow_label: "Retainer intake",
            question_code: "client_name",
            question_prompt: "Client name",
            answer_type: "string",
            prior_answer: Some("Libra"),
            progress: (1, 4),
            csrf_token: "",
            error: None,
        })
        .into_string();
        assert!(html.contains("value=\"Libra\""));
    }

    #[test]
    fn question_step_uses_textarea_for_text_answer_type() {
        let html = question_step(&QuestionStep {
            notation_id: ID1,
            flow_label: "Retainer intake",
            question_code: "product_description",
            question_prompt: "Describe the services",
            answer_type: "text",
            prior_answer: None,
            progress: (4, 4),
            csrf_token: "",
            error: None,
        })
        .into_string();
        assert!(html.contains("<textarea"));
    }

    #[test]
    fn question_step_includes_csrf_when_token_present() {
        let html = question_step(&QuestionStep {
            notation_id: ID1,
            flow_label: "Retainer intake",
            question_code: "client_name",
            question_prompt: "Client name",
            answer_type: "string",
            prior_answer: None,
            progress: (1, 4),
            csrf_token: "TOKEN_VALUE",
            error: None,
        })
        .into_string();
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"TOKEN_VALUE\""));
    }

    #[test]
    fn result_renders_notation_id_workflow_state_and_signature_id() {
        let html = result(&IntakeResult {
            notation_id: ID42,
            workflow_state: "sent_for_signature__pending",
            signature_request_id: Some("stub-42-1"),
            rendered: PreEscaped("<article class=\"notation\"><p>body</p></article>".to_string()),
            csrf_token: "",
        })
        .into_string();
        assert!(html.contains(&ID42.to_string()));
        assert!(html.contains("sent_for_signature__pending"));
        assert!(html.contains("stub-42-1"));
        assert!(html.contains("<article class=\"notation\"><p>body</p></article>"));
    }

    #[test]
    fn result_parked_shows_approve_and_send_when_no_signature_yet() {
        let html = result(&IntakeResult {
            notation_id: ID42,
            workflow_state: "staff_review",
            signature_request_id: None,
            rendered: PreEscaped("<article class=\"notation\"><p>body</p></article>".to_string()),
            csrf_token: "TOK",
        })
        .into_string();
        assert!(html.contains("Awaiting attorney review"));
        assert!(html.contains(&format!(
            "action=\"/portal/admin/notations/{ID42}/approve-send\""
        )));
        assert!(html.contains(">Approve and send for signature</button>"));
    }
}
