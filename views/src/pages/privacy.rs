//! `/privacy` — the foundation's privacy policy. Body lives in
//! [`views/content/privacy.md`] so non-engineers can edit prose
//! without touching Rust; the page wrapper just renders it.
//!
//! [`views/content/privacy.md`]: ../../content/privacy.md

use maud::Markup;

use crate::brand::FOUNDATION_BRAND;

const BODY: &str = include_str!("../../content/privacy.md");

#[must_use]
pub fn render(auth: crate::AuthState) -> Markup {
    let description = format!("Privacy Policy for the {}.", FOUNDATION_BRAND.site_name);
    super::policy::render(
        "Privacy Policy",
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
    fn privacy_renders_layout_and_title() {
        let html = render(crate::AuthState::Anonymous).into_string();
        assert!(html.contains(&format!(
            "<title>{} | Privacy Policy</title>",
            FOUNDATION_BRAND.site_name
        )));
    }

    #[test]
    fn privacy_renders_every_section_heading() {
        let html = render(crate::AuthState::Anonymous).into_string();
        for heading in [
            "Who We Are",
            "Information We Collect",
            "How We Use Your Information",
            "AI Assistance (AIDA)",
            "Who We Share Information With",
            "Your Privacy Rights",
            "Attorney-Client Privilege",
            "Donor Privacy",
            "Data Security and Retention",
            "Children's Privacy",
            "Changes to This Policy",
            "Contact Us",
        ] {
            assert!(html.contains(heading), "missing section `{heading}`");
        }
    }

    #[test]
    fn privacy_honors_ccpa_gdpr_and_right_to_delete() {
        // Privacy is a fundamental right here: the policy commits to
        // deletion and CCPA/GDPR/NRS-603A rights for everyone, not
        // only where the law strictly compels it.
        let html = collapse_ws(&render(crate::AuthState::Anonymous).into_string());
        assert!(html.contains("right to delete"));
        assert!(html.contains("CCPA"));
        assert!(html.contains("GDPR"));
        assert!(html.contains("NRS 603A"));
        assert!(html.contains("mailto:support@neonlaw.org"));
    }

    fn collapse_ws(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}
