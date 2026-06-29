//! Layer-1 accessibility gate — no browser required.
//!
//! Renders every portal create/edit form straight from its view
//! function and asserts the structural invariants the [`FormCard`]
//! component is supposed to guarantee:
//!
//! 1. no positive `tabindex` (DOM order drives focus; the only
//!    allowed value is `-1` on the focusable error banner),
//! 2. every visible control has an `id` and a matching `<label for>`,
//! 3. every `<label for>` points at an id that exists,
//! 4. every `aria-describedby` points at an id that exists,
//! 5. every `<form>` carries an accessible name (`aria-label`).
//!
//! These run in milliseconds under `cargo test`. The deeper,
//! engine-backed pass is `web/tests/accessibility_e2e.rs`, which runs
//! axe-core in a real browser. Together: fast guardrail + real audit,
//! both Rust-only.
//!
//! [`FormCard`]: views::components::FormCard

use std::collections::HashSet;

use regex::Regex;
use uuid::Uuid;

use views::pages::admin::{entities, people, projects, retainers};

const ID1: Uuid = Uuid::from_u128(1);
const ID2: Uuid = Uuid::from_u128(2);

/// Assert that `html` (a full rendered page) meets the form a11y
/// invariants. `label` names the page in failure messages.
fn assert_forms_accessible(html: &str, label: &str) {
    let id_re = Regex::new(r#"\bid="([^"]+)""#).unwrap();
    let ids: HashSet<&str> = id_re
        .captures_iter(html)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();

    // 1. No positive tabindex. Negative (-1) is fine — it's how the
    //    error banner is made programmatically focusable.
    let tab_re = Regex::new(r#"tabindex="(-?\d+)""#).unwrap();
    for cap in tab_re.captures_iter(html) {
        let value: i32 = cap[1].parse().unwrap();
        assert!(
            value < 0,
            "{label}: found tabindex=\"{value}\" — positive tabindex reorders the page tab \
             sequence ahead of the nav (WCAG 2.4.3 / axe failure); let DOM order drive focus",
        );
    }

    // Gather every `<label for>` target.
    let label_for_re = Regex::new(r#"<label[^>]*\bfor="([^"]+)""#).unwrap();
    let labelled: HashSet<&str> = label_for_re
        .captures_iter(html)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();

    // 3. Every label points at an id that exists.
    for target in &labelled {
        assert!(
            ids.contains(target),
            "{label}: <label for=\"{target}\"> has no element with that id",
        );
    }

    // 2. Every visible control has an id and a matching label.
    let control_re = Regex::new(r"<(input|select|textarea)\b([^>]*)>").unwrap();
    for cap in control_re.captures_iter(html) {
        let tag = &cap[1];
        let attrs = &cap[2];
        // Hidden inputs (CSRF) carry no user-facing label by design.
        if attrs.contains(r#"type="hidden""#) {
            continue;
        }
        let id = id_re.captures(attrs).map_or_else(
            || panic!("{label}: <{tag}> control without an id: {attrs}"),
            |c| c.get(1).unwrap().as_str(),
        );
        assert!(
            labelled.contains(id),
            "{label}: control id=\"{id}\" has no <label for=\"{id}\">",
        );
    }

    // 4. Every aria-describedby points at an id that exists.
    let desc_re = Regex::new(r#"aria-describedby="([^"]+)""#).unwrap();
    for cap in desc_re.captures_iter(html) {
        let target = cap.get(1).unwrap().as_str();
        assert!(
            ids.contains(target),
            "{label}: aria-describedby=\"{target}\" has no element with that id",
        );
    }

    // 5. Every form has an accessible name.
    let form_re = Regex::new(r"<form\b([^>]*)>").unwrap();
    let mut saw_form = false;
    for cap in form_re.captures_iter(html) {
        saw_form = true;
        assert!(
            cap[1].contains("aria-label="),
            "{label}: <form> has no aria-label (accessible name): {}",
            &cap[1],
        );
    }
    assert!(
        saw_form,
        "{label}: expected at least one <form> on the page"
    );
}

#[test]
fn people_forms_are_accessible() {
    let create = people::new_form(&people::PersonForm {
        csrf_token: "TOK",
        ..people::PersonForm::default()
    });
    assert_forms_accessible(&create.into_string(), "people::new_form");

    // Locked bootstrap admin row: disabled <select> + helper text.
    let edit = people::edit_form(
        ID1,
        &people::PersonForm {
            name: "Nick",
            email: "nick@neonlaw.com",
            role: "admin",
            role_locked: true,
            csrf_token: "TOK",
            ..people::PersonForm::default()
        },
        None,
    );
    assert_forms_accessible(&edit.into_string(), "people::edit_form (locked role)");

    // Error state: the banner must be present + focusable, and the
    // page must still satisfy every invariant.
    let errored = people::new_form(&people::PersonForm {
        email: "bad",
        error: Some("Email is invalid"),
        ..people::PersonForm::default()
    });
    let html = errored.into_string();
    assert_forms_accessible(&html, "people::new_form (error)");
    assert!(
        html.contains("role=\"alert\"") && html.contains("tabindex=\"-1\""),
        "error banner should be an announced, focusable alert",
    );
}

#[test]
fn entity_forms_are_accessible() {
    let types = [entities::TypeChoice {
        id: ID1,
        name: "LLC",
    }];
    let jurs = [entities::JurisdictionChoice {
        id: ID1,
        name: "Nevada",
        code: "NV",
    }];
    let create = entities::new_form(&entities::EntityForm::default(), &types, &jurs);
    assert_forms_accessible(&create.into_string(), "entities::new_form");

    let edit = entities::edit_form(
        ID2,
        &entities::EntityForm {
            name: "Acme",
            entity_type_id: Some(ID1),
            jurisdiction_id: Some(ID1),
            error: Some("Pick a jurisdiction"),
        },
        &types,
        &jurs,
    );
    assert_forms_accessible(&edit.into_string(), "entities::edit_form (error)");
}

#[test]
fn project_forms_are_accessible() {
    let entity_choices = [projects::EntityChoice {
        id: ID1,
        name: "Acme",
    }];
    let create = projects::new_form(&projects::Form::default(), &entity_choices);
    assert_forms_accessible(&create.into_string(), "projects::new_form");

    // With onboarding templates present, the optional retainer block
    // renders (checkbox + template picker + signer fields); it must be
    // accessible too.
    let retainer_templates = [(
        "onboarding__retainer".to_string(),
        "Retainer Agreement — onboarding__retainer".to_string(),
    )];
    let create_retainer = projects::new_form(
        &projects::Form {
            retainer_templates: &retainer_templates,
            ..Default::default()
        },
        &entity_choices,
    );
    assert_forms_accessible(
        &create_retainer.into_string(),
        "projects::new_form (retainer block)",
    );

    let edit = projects::edit_form(
        ID2,
        &projects::Form {
            name: "Audit",
            status: "open",
            entity_id: Some(ID1),
            error: None,
            ..Default::default()
        },
        &entity_choices,
    );
    assert_forms_accessible(&edit.into_string(), "projects::edit_form");

    // Detail page: the multipart upload form, embedded under the page
    // h1 (no autofocus, h2 headings).
    let detail = projects::detail(&projects::Detail {
        id: ID2,
        name: "Sison Trust",
        status: "open",
        entity_name: Some("Acme"),
        staff_dri: Some("Nick Shook"),
        client_dri: Some("Libra Client"),
        documents: &[],
        estate: None,
        csrf_token: Some("TOK"),
    });
    let html = detail.into_string();
    assert_forms_accessible(&html, "projects::detail (upload)");
    // An embedded form on a content page must not autofocus anything.
    assert!(
        !html.contains("autofocus"),
        "project detail forms are embedded and must not autofocus",
    );
}

#[test]
fn retainer_forms_are_accessible() {
    let templates = [(
        "onboarding__retainer".to_string(),
        "Retainer Agreement — onboarding__retainer".to_string(),
    )];
    let start = retainers::start_walk(&retainers::StartWalk {
        client_email: "",
        retainer_template_code: "",
        templates: &templates,
        csrf_token: "TOK",
        error: None,
    });
    assert_forms_accessible(&start.into_string(), "retainers::start_walk");

    // Every answer-type variant the walker can render.
    for answer_type in ["string", "text", "int", "bool"] {
        let step = retainers::question_step(&retainers::QuestionStep {
            notation_id: ID1,
            flow_label: "Retainer intake",
            question_code: "client_email",
            question_prompt: "What is the client's email address?",
            answer_type,
            prior_answer: None,
            progress: (2, 4),
            csrf_token: "TOK",
            error: None,
        });
        assert_forms_accessible(
            &step.into_string(),
            &format!("retainers::question_step ({answer_type})"),
        );
    }
}
