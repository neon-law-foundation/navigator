//! `/terms` — the foundation's terms of service. Body lives in
//! [`views/content/terms.md`] so non-engineers can edit prose without
//! touching Rust; the page wrapper just renders it.
//!
//! [`views/content/terms.md`]: ../../content/terms.md

use maud::Markup;

use crate::brand::FOUNDATION_BRAND;

const BODY: &str = include_str!("../../content/terms.md");

#[must_use]
pub fn render(auth: crate::AuthState) -> Markup {
    let description = format!("Terms of Service for the {}.", FOUNDATION_BRAND.site_name);
    super::policy::render(
        "Terms of Service",
        &description,
        BODY,
        auth,
        *FOUNDATION_BRAND,
    )
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::brand::FOUNDATION_BRAND;

    #[test]
    fn terms_renders_layout_and_title() {
        let html = render(crate::AuthState::Anonymous).into_string();
        assert!(html.contains(&format!(
            "<title>{} | Terms of Service</title>",
            FOUNDATION_BRAND.site_name
        )));
    }

    #[test]
    fn terms_renders_every_section_heading() {
        let html = render(crate::AuthState::Anonymous).into_string();
        for heading in [
            "Acceptance of Terms",
            "Who Provides These Services",
            "No Legal Advice Without Engagement",
            "Conflicts of Interest",
            "Use of Services",
            "Trademarks",
            "Limitation of Liability and Hold Harmless",
            "Governing Law",
            "Contact Us",
        ] {
            assert!(html.contains(heading), "missing section `{heading}`");
        }
    }

    #[test]
    fn terms_governing_law_is_washoe_county_nevada() {
        let html = collapse_ws(&render(crate::AuthState::Anonymous).into_string());
        assert!(html.contains("Washoe County, Nevada"));
        assert!(html.contains("laws of the State of Nevada"));
    }

    #[test]
    fn terms_renders_hold_harmless_substance() {
        let html = collapse_ws(&render(crate::AuthState::Anonymous).into_string());
        assert!(html.contains("at your sole risk"));
        assert!(html.contains("commercially reasonable efforts"));
        // The hold-harmless clause names both entities: the Foundation
        // (the 501(c)(3)) and Shook Law PLLC (the firm trading as Neon Law).
        assert!(html.contains("Neon Law Foundation"));
        assert!(html.contains("Shook Law PLLC"));
        assert!(html.contains("gross negligence or willful misconduct"));
    }

    #[test]
    fn terms_conflicts_section_states_firm_wide_decline_posture() {
        let html = collapse_ws(&render(crate::AuthState::Anonymous).into_string());
        // Firm-wide imputed conflicts (RPC 1.10): one check across the whole firm,
        // declined rather than screened, with up-front consent to info-sharing.
        assert!(html.contains("run conflicts across the whole firm"));
        assert!(html.contains("decline the matter rather than screen it internally"));
        assert!(html.contains("share incoming-matter information"));
    }

    #[test]
    fn terms_trademark_section_names_the_mark_and_forbids_unauthorized_use() {
        let html = collapse_ws(&render(crate::AuthState::Anonymous).into_string());
        assert!(html.contains("registered trademark of Shook Law PLLC"));
        assert!(html.contains("U.S. Reg. No. 6,325,650"));
        assert!(html.contains("without our prior written permission is unauthorized"));
    }

    fn collapse_ws(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}
