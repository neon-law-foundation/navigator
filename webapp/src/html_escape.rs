//! HTML escaping for the few places Navigator builds markup by hand.
//!
//! Almost nothing should need this: `rsx!` escapes interpolated values and
//! escapes them too, so hand-built markup is the exception. The exceptions are
//! real, though — a document shell wrapped around an SSR'd component, and the
//! `<head>` fragments injected after a render — and `format!` does not escape,
//! so those call sites need something better than care.
//!
//! Two functions rather than one, because the contexts differ. Text content
//! only has to survive tag parsing; an attribute value additionally has to
//! survive the quote that delimits it, which is the escape that actually
//! prevents breaking out into markup.

/// Escape a string for use as HTML **text content**.
///
/// `&` first — reversing the order would double-escape the ampersands the later
/// replacements introduce.
#[must_use]
pub fn escape_text(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape a string for use inside a **quoted HTML attribute value**.
///
/// Escapes both quote characters, so the result is safe whether the attribute
/// is delimited by `"` or `'`. Without that a crafted value closes the
/// attribute and everything after it is parsed as markup — the whole defect
/// this exists to prevent.
#[must_use]
pub fn escape_attr(raw: &str) -> String {
    escape_text(raw)
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::{escape_attr, escape_text};

    #[test]
    fn text_escaping_closes_the_tag_boundary() {
        assert_eq!(
            escape_text("<script>alert(1)</script>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
    }

    #[test]
    fn ampersands_are_escaped_once_not_twice() {
        // The classic ordering bug: escaping `<` before `&` turns `&lt;` into
        // `&amp;lt;` and the page renders the entity as literal text.
        assert_eq!(escape_text("a & b"), "a &amp; b");
        assert_eq!(escape_text("<"), "&lt;");
        assert_eq!(escape_attr("&<>"), "&amp;&lt;&gt;");
    }

    #[test]
    fn an_attribute_value_cannot_break_out_of_either_quote() {
        // This is the case the helper exists for: a value that closes its own
        // attribute and opens a tag.
        let hostile = r#"https://x/"><script>alert(1)</script>"#;
        let escaped = escape_attr(hostile);
        assert!(!escaped.contains('"'), "no bare double quote: {escaped}");
        assert!(!escaped.contains('<'), "no bare tag open: {escaped}");
        assert_eq!(
            format!("<link href=\"{escaped}\">").matches('<').count(),
            1,
            "the attribute cannot introduce a second tag: {escaped}"
        );

        let single = escape_attr("it's");
        assert!(!single.contains('\''), "no bare single quote: {single}");
    }

    #[test]
    fn ordinary_text_is_left_alone() {
        assert_eq!(escape_text("Not found"), "Not found");
        assert_eq!(
            escape_attr("/public/css/theme.css"),
            "/public/css/theme.css"
        );
    }
}
