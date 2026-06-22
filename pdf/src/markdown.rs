//! Convert a notation template's Markdown body into Typst markup.
//!
//! Notation templates are authored in **Markdown** (`##` headings,
//! `**bold**`, `-` lists) — that is what the `rules` validator checks.
//! The [`crate::render`] pipeline, however, compiles **Typst**, whose
//! markup is close but not identical: emphasis is `*x*` not `**x**`,
//! headings are `=` not `#`, and a stray `#` or `$` in prose is a
//! function call or math delimiter. Feeding raw Markdown to Typst
//! therefore renders wrong or fails to compile.
//!
//! [`to_typst`] walks the [`pulldown_cmark`] event stream and emits the
//! equivalent Typst markup, escaping every character Typst would
//! otherwise treat as syntax. It covers the constructs that appear in
//! notation bodies (headings, paragraphs, strong/emphasis, ordered and
//! unordered lists, block quotes, inline code, links, horizontal
//! rules); inline raw HTML is dropped rather than leaked as literal
//! tags. Placeholder tokens (`{{name}}`) pass through verbatim — the
//! caller substitutes them before conversion.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Convert a Markdown `body` to Typst markup suitable for
/// [`crate::render`].
///
/// The output is fragment markup (no page setup or font rule) — the
/// caller wraps it in an [`crate::OutputFormat`] chrome preamble.
#[must_use]
pub fn to_typst(body: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(body, opts);

    let mut out = String::with_capacity(body.len() + body.len() / 8);
    // Ordered-list counters, one per nesting level. `None` marks an
    // unordered list at that depth.
    let mut list_stack: Vec<Option<u64>> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => start_tag(&mut out, &tag, &mut list_stack),
            Event::End(tag) => end_tag(&mut out, tag, &mut list_stack),
            Event::Text(text) => out.push_str(&escape_text(&text)),
            Event::Code(code) => {
                out.push_str("#raw(");
                out.push_str(&typst_string(&code));
                out.push(')');
            }
            Event::SoftBreak => out.push(' '),
            Event::HardBreak => out.push_str(" \\\n"),
            Event::Rule => out.push_str("\n#line(length: 100%)\n\n"),
            // Raw/inline HTML, footnotes, math, task markers: notation
            // bodies don't use them; drop rather than leak literal tags.
            _ => {}
        }
    }
    // Collapse any run of 3+ newlines the structure handlers produced
    // into the canonical paragraph break.
    normalize_blank_lines(&out)
}

fn start_tag(out: &mut String, tag: &Tag, list_stack: &mut Vec<Option<u64>>) {
    match tag {
        Tag::Heading { level, .. } => {
            out.push('\n');
            for _ in 0..heading_depth(*level) {
                out.push('=');
            }
            out.push(' ');
        }
        Tag::Strong => out.push('*'),
        Tag::Emphasis => out.push('_'),
        Tag::Strikethrough => out.push_str("#strike["),
        Tag::List(first) => list_stack.push(*first),
        Tag::Item => {
            // Indent nested items two spaces per level below the top.
            let depth = list_stack.len().saturating_sub(1);
            for _ in 0..depth {
                out.push_str("  ");
            }
            match list_stack.last_mut() {
                Some(Some(n)) => {
                    out.push_str(&n.to_string());
                    out.push_str(". ");
                    *n += 1;
                }
                _ => out.push_str("- "),
            }
        }
        Tag::BlockQuote(_) => out.push_str("#quote(block: true)[\n"),
        Tag::Link { dest_url, .. } => {
            out.push_str("#link(");
            out.push_str(&typst_string(dest_url));
            out.push_str(")[");
        }
        // Headings/paragraphs inside other blocks and unhandled tags
        // contribute their text via Text events; no wrapper needed.
        _ => {}
    }
}

fn end_tag(out: &mut String, tag: TagEnd, list_stack: &mut Vec<Option<u64>>) {
    match tag {
        TagEnd::Heading(_) | TagEnd::Paragraph => out.push_str("\n\n"),
        TagEnd::Strong => out.push('*'),
        TagEnd::Emphasis => out.push('_'),
        TagEnd::Strikethrough | TagEnd::Link => out.push(']'),
        TagEnd::List(_) => {
            list_stack.pop();
            if list_stack.is_empty() {
                out.push('\n');
            }
        }
        TagEnd::Item => out.push('\n'),
        TagEnd::BlockQuote(_) => out.push_str("]\n\n"),
        _ => {}
    }
}

/// Typst supports six heading levels; clamp deeper Markdown headings to
/// the deepest Typst level rather than emitting an over-long `=` run.
fn heading_depth(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Escape the characters Typst treats as markup syntax so prose text
/// renders verbatim. The set is the markup sigils that can fire
/// mid-line: a function/label call (`#`, `<`), math (`$`), emphasis
/// (`*`, `_`), raw (`` ` ``), reference (`@`), content brackets
/// (`[`, `]`), and the escape char itself (`\`).
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '#' | '$' | '*' | '_' | '`' | '<' | '@' | '[' | ']'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Render `s` as a Typst double-quoted string literal, escaping the two
/// characters significant inside one. Used for `#raw(..)` / `#link(..)`
/// arguments, where the content is a string expression, not markup.
fn typst_string(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Squeeze runs of 3+ newlines down to exactly two (one blank line),
/// and trim leading/trailing whitespace, so the emitted Typst has
/// stable paragraph spacing regardless of how the handlers stacked
/// their `\n`s.
fn normalize_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newline_run = 0usize;
    for c in s.chars() {
        if c == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push(c);
            }
        } else {
            newline_run = 0;
            out.push(c);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::to_typst;

    #[test]
    fn headings_become_equals_runs() {
        assert_eq!(to_typst("# Title"), "= Title");
        assert_eq!(to_typst("## Section"), "== Section");
        assert_eq!(to_typst("### Sub"), "=== Sub");
    }

    #[test]
    fn strong_collapses_to_single_asterisks() {
        // The crux: Markdown `**x**` must NOT survive as `**x**`, which
        // renders un-bolded in Typst.
        assert_eq!(to_typst("**bold**"), "*bold*");
    }

    #[test]
    fn emphasis_becomes_underscores() {
        assert_eq!(to_typst("*italic*"), "_italic_");
        assert_eq!(to_typst("_italic_"), "_italic_");
    }

    #[test]
    fn unordered_list_uses_typst_dash_markers() {
        let out = to_typst("- one\n- two\n");
        assert_eq!(out, "- one\n- two");
    }

    #[test]
    fn ordered_list_numbers_explicitly() {
        let out = to_typst("1. first\n2. second\n");
        assert_eq!(out, "1. first\n2. second");
    }

    #[test]
    fn inline_code_becomes_raw_call_not_backticks() {
        // A Typst backtick run would need matched delimiters; `#raw(..)`
        // is unambiguous and escapes nothing in the prose stream.
        assert_eq!(to_typst("`code`"), "#raw(\"code\")");
    }

    #[test]
    fn placeholder_tokens_pass_through_verbatim() {
        // `{{name}}` carries no Typst meaning; it must survive so the
        // caller can substitute it (before or after conversion).
        assert_eq!(to_typst("Hello `{{name}}`"), "Hello #raw(\"{{name}}\")");
    }

    #[test]
    fn typst_sigils_in_prose_are_escaped() {
        // A bare `#`/`$` in prose would otherwise start a Typst function
        // call or math block and break compilation.
        assert_eq!(to_typst(r"Pay $9,999 to #1"), r"Pay \$9,999 to \#1");
    }

    #[test]
    fn link_becomes_typst_link_call() {
        assert_eq!(
            to_typst("[neon](https://neon.law)"),
            "#link(\"https://neon.law\")[neon]"
        );
    }

    #[test]
    fn paragraphs_are_separated_by_one_blank_line() {
        assert_eq!(to_typst("one\n\ntwo"), "one\n\ntwo");
    }

    #[test]
    fn blockquote_wraps_in_typst_quote() {
        let out = to_typst("> noted");
        assert!(out.starts_with("#quote(block: true)["), "got: {out}");
        assert!(out.contains("noted"));
    }

    #[test]
    fn output_is_typst_compilable() {
        // The real safety net: whatever we emit must compile.
        let md = "# Demand\n\nPay **now** to `{{party}}`:\n\n- item one\n- item two\n\n> heed this";
        let typ = to_typst(md);
        crate::render(&typ).expect("converted markdown must compile through Typst");
    }
}
