//! `/foundation/mission` — the project's mission statement.
//!
//! The mission body is loaded from `web/content/marketing/mission.md`
//! alongside the other marketing fragments and injected into
//! [`MarketingIndex`] with slug `mission`. This page is the letter alone
//! — *why we exist* — so it reads start-to-finish as one unbroken
//! letter.
//!
//! [`MarketingIndex`]: crate
//! [`FOUNDATION_BRAND`]: crate::brand::FOUNDATION_BRAND

use chrono::NaiveDate;
use maud::{html, Markup, PreEscaped};

use crate::brand::FOUNDATION_BRAND;
use crate::components::freshness;
use crate::{i18n, AuthState, Locale, PageLayout};

pub struct MissionContent<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub body_html: &'a str,
    /// Git-derived "last edited in main" date for `mission.md`. None
    /// in production (distroless image has no git) and silently
    /// omitted from the footer in that case.
    pub last_edited: Option<NaiveDate>,
}

/// Lazily-leaked description string so the env-driven brand names
/// are resolved once per process, not once per `default()` call.
static DEFAULT_DESCRIPTION: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    Box::leak(
        format!(
            "How {} and the {} make routine legal services cheap \
             without sacrificing correctness, and what a licensed \
             attorney in the loop actually buys you.",
            crate::brand::FIRM_BRAND.site_name,
            FOUNDATION_BRAND.site_name,
        )
        .into_boxed_str(),
    )
});

impl Default for MissionContent<'_> {
    fn default() -> Self {
        Self {
            title: "Mission",
            description: *DEFAULT_DESCRIPTION,
            body_html:
                "<p>Mission copy is loaded from <code>web/content/marketing/mission.md</code>.</p>",
            last_edited: None,
        }
    }
}

/// Render the mission letter in English.
#[must_use]
pub fn render(content: &MissionContent<'_>, auth: AuthState) -> Markup {
    render_in(content, auth, Locale::En)
}

/// Render the mission letter in `locale`. The letter body itself comes
/// from `content` (English `marketing/mission.md` or the transcreated
/// `marketing/es/mission.md`); this view localizes the chrome and
/// declares `/foundation/mission` as the canonical twin. English output
/// is byte-identical to the pre-i18n page.
#[must_use]
pub fn render_in(content: &MissionContent<'_>, auth: AuthState, locale: Locale) -> Markup {
    // The mission reads as a letter, so we cap its measure at ~65
    // characters and center it. Design studios put comfortable prose
    // measure at 45–75 characters per line; 65ch keeps the letter
    // readable on a phone and stops it sprawling across a wide desktop.
    // `ch` tracks the body font, so the cap holds as the type scales.
    let body = html! {
        article.mission-letter style="max-width: 65ch; margin-inline: auto;" {
            (PreEscaped(content.body_html))
            (freshness::render(content.last_edited))
        }
    };
    // English keeps the literal "Mission"; Spanish gets "Misión".
    let title = i18n::nav_label("Mission", locale);
    PageLayout::new(&title)
        .with_description(content.description)
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .with_locale(locale)
        .with_canonical_path("/foundation/mission")
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render, MissionContent};
    use crate::brand::{FIRM_BRAND, FOUNDATION_BRAND};

    #[test]
    fn mission_renders_layout_under_foundation_brand() {
        let html = render(&MissionContent::default(), crate::AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!(
            "<title>{} | Mission</title>",
            FOUNDATION_BRAND.site_name
        )));
    }

    #[test]
    fn mission_uses_caller_body_html_verbatim() {
        let content = MissionContent {
            title: "T",
            description: "D",
            body_html: "<h2>Why Rust</h2><p>Type-safe workflows.</p>",
            last_edited: None,
        };
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("<h2>Why Rust</h2>"));
        assert!(html.contains("<p>Type-safe workflows.</p>"));
    }

    #[test]
    fn mission_renders_freshness_footer_when_last_edited_present() {
        let content = MissionContent {
            title: "T",
            description: "D",
            body_html: "<p>body</p>",
            last_edited: chrono::NaiveDate::from_ymd_opt(2026, 5, 22),
        };
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains("Last edited in main May 22, 2026"),
            "got: {html}"
        );
    }

    #[test]
    fn mission_omits_freshness_footer_when_last_edited_absent() {
        let html = render(&MissionContent::default(), crate::AuthState::Anonymous).into_string();
        assert!(!html.contains("Last edited"), "got: {html}");
    }

    #[test]
    fn mission_header_uses_foundation_brand_nav() {
        let html = render(&MissionContent::default(), crate::AuthState::Anonymous).into_string();
        // Foundation brand nav links back to the firm at "/".
        assert!(
            html.contains(&format!(">{}</a>", FIRM_BRAND.site_name)),
            "got: {html}"
        );
        // And does NOT carry the firm's Services dropdown.
        assert!(!html.contains(">Services</summary>"), "got: {html}");
    }

    #[test]
    fn mission_letter_is_capped_at_a_readable_measure() {
        // The letter is constrained to a ~65-character measure and
        // centered so it reads as a letter on every viewport.
        let html = render(&MissionContent::default(), crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains("class=\"mission-letter\""),
            "mission body should carry the letter class, got: {html}"
        );
        assert!(
            html.contains("max-width: 65ch"),
            "mission letter should be capped at a 65ch measure, got: {html}"
        );
    }
}
