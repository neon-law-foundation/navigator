//! `<p class="freshness">` footer for long-lived content pages.
//!
//! Renders the git-derived edit date as `Last edited in main MMM D,
//! YYYY`. Returns an empty fragment when the date is absent (the
//! distroless prod image has no git history, so the line is silently
//! dropped there).

use chrono::NaiveDate;
use maud::{html, Markup};

/// Last-edited-in-main freshness footer. Returns an empty `Markup`
/// when the input is `None` so callers can splice unconditionally.
#[must_use]
pub fn render(last_edited: Option<NaiveDate>) -> Markup {
    let Some(date) = last_edited else {
        return html! {};
    };
    html! {
        p.freshness {
            small {
                "Last edited in main " (format_human(date))
            }
        }
    }
}

/// `Apr 12, 2026` — the format we render dates in for humans. Short,
/// unambiguous, locale-stable.
fn format_human(d: NaiveDate) -> String {
    d.format("%b %e, %Y")
        .to_string()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{format_human, render};
    use chrono::NaiveDate;

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn empty_when_input_is_none() {
        let out = render(None).into_string();
        assert!(out.is_empty(), "got: {out}");
    }

    #[test]
    fn renders_last_edited_when_present() {
        let out = render(Some(ymd(2026, 5, 22))).into_string();
        assert!(
            out.contains("Last edited in main May 22, 2026"),
            "got: {out}"
        );
    }

    #[test]
    fn date_format_uses_no_leading_zero_on_day() {
        // `%e` gives a space-padded day; format_human collapses the
        // double-space so single-digit days render as "Apr 1, 2026"
        // rather than "Apr  1, 2026".
        assert_eq!(format_human(ymd(2026, 4, 1)), "Apr 1, 2026");
        assert_eq!(format_human(ymd(2026, 4, 12)), "Apr 12, 2026");
    }
}
