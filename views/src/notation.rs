//! Notation renderer — fills in a template body with a context map.
//!
//! A template body is plain text with three placeholder grammars, all
//! evaluated by [`fill`] (the shared evaluator this render path and the
//! form-fill path both meet):
//!
//! - **Bare** — `{{code}}` / `{{type__role}}` substitutes the context
//!   value for that key.
//! - **Iterator** — `{{#for x in people__members}} … {{x.name}} … {{/for}}`
//!   walks an aggregate answer (a JSON array stored under the state key) and
//!   renders the inner block once per row, resolving `{{x.part}}` against
//!   that row.
//! - **Dotted `row.part`** — inside a loop, `{{x.part}}` reads a field off
//!   the current row (the same `row`/`part` access `forms::resolve` does).
//!
//! Unfilled placeholders are *not* an error; rendering a partly-filled
//! notation is a valid intermediate state. Tests use the "no `{{` left in
//! the output" assertion to detect missing keys.

use std::collections::BTreeMap;

use maud::{html, Markup};

const FOR_OPEN: &str = "{{#for ";
const FOR_CLOSE: &str = "{{/for}}";

/// Evaluate `body` against `context` — expand `{{#for …}}` iterators over
/// aggregate answers, then substitute every remaining bare `{{key}}`. The
/// pure string half of [`render_filled_in`], shared so the render path
/// gains the iteration + dotted `row.part` capability of the form-fill path.
#[must_use]
pub fn fill(body: &str, context: &BTreeMap<String, String>) -> String {
    let expanded = expand_loops(body, context);
    let mut filled = expanded;
    for (k, v) in context {
        filled = filled.replace(&format!("{{{{{k}}}}}"), v);
    }
    filled
}

/// Expand every `{{#for <var> in <state>}} … {{/for}}` block by rendering
/// its body once per row of the aggregate answer stored under `<state>`
/// (a JSON array; parsed via `serde_yaml`, a JSON superset). `{{var.part}}`
/// inside the block resolves to that row's `part` field.
fn expand_loops(body: &str, context: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(start) = rest.find(FOR_OPEN) {
        let after_open = &rest[start + FOR_OPEN.len()..];
        let Some(header_len) = after_open.find("}}") else {
            break;
        };
        let header = after_open[..header_len].trim();
        let block_start = start + FOR_OPEN.len() + header_len + 2;
        // Match the *balanced* `{{/for}}` so a nested loop's close doesn't
        // terminate the outer one.
        let Some(close_rel) = matching_close(&rest[block_start..]) else {
            break;
        };
        let block = &rest[block_start..block_start + close_rel];
        let close_end = block_start + close_rel + FOR_CLOSE.len();

        out.push_str(&rest[..start]);
        if let Some((var, state)) = header.split_once(" in ") {
            out.push_str(&render_loop(var.trim(), state.trim(), block, context));
        }
        rest = &rest[close_end..];
    }
    out.push_str(rest);
    out
}

/// The byte offset of the `{{/for}}` that balances the loop whose body
/// starts at the front of `s` (depth 1 already open), or `None` if it never
/// closes. Nested `{{#for …}}` raise the depth so an inner close doesn't
/// terminate the outer loop.
fn matching_close(s: &str) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with(FOR_OPEN) {
            depth += 1;
            i += FOR_OPEN.len();
        } else if s[i..].starts_with(FOR_CLOSE) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
            i += FOR_CLOSE.len();
        } else {
            i += 1;
        }
    }
    None
}

/// Render `block` once per row of the aggregate answer at `state`, resolving
/// `{{var.part}}` against each row.
fn render_loop(var: &str, state: &str, block: &str, context: &BTreeMap<String, String>) -> String {
    let Some(json) = context.get(state) else {
        return String::new();
    };
    let rows: Vec<BTreeMap<String, serde_yaml::Value>> =
        serde_yaml::from_str(json).unwrap_or_default();
    let mut out = String::new();
    for row in &rows {
        let mut piece = block.to_string();
        for (part, value) in row {
            let needle = format!("{{{{{var}.{part}}}}}");
            piece = piece.replace(&needle, &yaml_scalar(value));
        }
        // Recurse so a nested `{{#for …}}` inside this row's block expands.
        out.push_str(&expand_loops(&piece, context));
    }
    out
}

/// The string form of a row field value — a YAML/JSON string unwraps to its
/// inner text; anything else falls back to its compact form.
fn yaml_scalar(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Null => String::new(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

/// Render `body` with `context` evaluated into it (bare substitution plus
/// `{{#for …}}` iteration — see [`fill`]). The result is wrapped in an
/// `<article class="notation">` container with one `<p>` per paragraph
/// (blank-line-separated).
#[must_use]
pub fn render_filled_in(body: &str, context: &BTreeMap<String, String>) -> Markup {
    let filled = fill(body, context);
    let paragraphs: Vec<&str> = filled
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    html! {
        article.notation {
            @for p in &paragraphs {
                p { (collapse_whitespace(p)) }
            }
        }
    }
}

/// Replace any run of whitespace (spaces, newlines, tabs) with a
/// single space so a multi-line paragraph in the template renders
/// as one flowing paragraph in HTML.
fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::render_filled_in;

    fn ctx(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn substitutes_single_placeholder() {
        let html = render_filled_in("Hello {{client_name}}.", &ctx(&[("client_name", "Libra")]))
            .into_string();
        assert!(html.contains("Hello Libra."), "got: {html}");
        assert!(!html.contains("{{"));
    }

    #[test]
    fn substitutes_multiple_placeholders_across_paragraphs() {
        let body = "\
I, {{client_name}}, hire the firm for {{product_description}}.

The retainer covers the project {{project_name}}.";
        let html = render_filled_in(
            body,
            &ctx(&[
                ("client_name", "Libra"),
                ("product_description", "estate planning"),
                ("project_name", "Estate planning — Libra"),
            ]),
        )
        .into_string();
        assert!(html.contains("I, Libra, hire the firm for estate planning."));
        assert!(html.contains("The retainer covers the project Estate planning — Libra."));
        // Two paragraphs → two <p> tags.
        let p_count = html.matches("<p>").count();
        assert_eq!(p_count, 2, "two paragraphs expected, html: {html}");
    }

    #[test]
    fn collapses_intra_paragraph_newlines_into_single_spaces() {
        let body = "I,\n{{client_name}},\nhire the firm.";
        let html = render_filled_in(body, &ctx(&[("client_name", "Libra")])).into_string();
        assert!(html.contains("I, Libra, hire the firm."));
    }

    #[test]
    fn leaves_unfilled_placeholders_in_output_for_grep() {
        let html = render_filled_in("Hi {{missing}}.", &ctx(&[])).into_string();
        assert!(html.contains("{{missing}}"));
    }

    #[test]
    fn empty_body_renders_empty_article() {
        let html = render_filled_in("", &ctx(&[])).into_string();
        assert!(html.contains("<article class=\"notation\">"));
        assert!(!html.contains("<p>"));
    }

    #[test]
    fn wraps_in_notation_article_class() {
        let html = render_filled_in("body", &ctx(&[])).into_string();
        assert!(html.contains("<article class=\"notation\">"), "got: {html}");
    }

    #[test]
    fn for_loop_iterates_an_aggregate_answer_with_dotted_row_part() {
        let body = "Members: {{#for m in people__members}}{{m.name}} of {{m.city}}; {{/for}}done.";
        let context = ctx(&[(
            "people__members",
            r#"[{"name": "Aries", "city": "Las Vegas"}, {"name": "Libra", "city": "Reno"}]"#,
        )]);
        let filled = super::fill(body, &context);
        assert_eq!(
            filled, "Members: Aries of Las Vegas; Libra of Reno; done.",
            "got: {filled}"
        );
    }

    #[test]
    fn for_loop_over_a_missing_aggregate_renders_nothing() {
        let filled = super::fill(
            "[{{#for m in people__members}}{{m.name}}{{/for}}]",
            &ctx(&[]),
        );
        assert_eq!(filled, "[]");
    }

    #[test]
    fn nested_for_loops_match_balanced_closes() {
        // The inner `{{/for}}` must not terminate the outer loop.
        let body = "{{#for g in groups}}[{{g.title}}: {{#for m in groups}}{{m.title}} {{/for}}]{{/for}}";
        let context = ctx(&[("groups", r#"[{"title": "A"}, {"title": "B"}]"#)]);
        let filled = super::fill(body, &context);
        assert_eq!(filled, "[A: A B ][B: A B ]", "got: {filled}");
    }

    #[test]
    fn bare_and_loop_placeholders_compose() {
        let body = "{{title}}: {{#for m in people__members}}{{m.name}} {{/for}}";
        let context = ctx(&[
            ("title", "Roster"),
            ("people__members", r#"[{"name": "Aries"}]"#),
        ]);
        assert_eq!(super::fill(body, &context), "Roster: Aries ");
    }
}
