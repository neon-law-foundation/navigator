//! Inline-styled HTML wrapper for outbound email.
//!
//! Email clients (Gmail, Outlook) strip `<style>` blocks and external
//! stylesheets and never load SVG `<img>` sources, so this layout uses
//! **inline** styles on a table skeleton and references the firm logo
//! as a hosted **PNG** (`/logo-firm.png`, served by `web`'s static
//! asset route). It deliberately shares nothing with the `views`
//! page shell — an email is not a web page, and we don't want the
//! site nav/footer landing in someone's inbox.
//!
//! ## Typeface
//!
//! The body is set in **Noto Serif**, the firm typeface, declared via
//! a `@font-face` in a `<head>` `<style>` block pointing at the same
//! self-hosted woff2 `web` serves. Be honest about reach: web fonts in
//! email are best-effort — Apple Mail honors `@font-face`, but Gmail
//! and Outlook strip it. Every cell therefore also carries an
//! **inline** `font-family` that leads with `Noto Serif` and falls
//! back to a serif stack (Georgia), so a client that ignores the
//! webfont still renders a serif close in feel, not the default
//! sans-serif. The webfont URL is absolute (built from `base_url`)
//! because an inbox has no notion of our origin.
//!
//! Callers render their markdown body (the same source as the
//! plain-text part) through [`render_email_html`] so the two stay in
//! lockstep; the markdown is the single source of truth.

use pulldown_cmark::{html, Options, Parser};

/// Env var carrying the public origin the logo PNG is served from.
/// Mirrors `web::openapi`'s base-URL knob so a single value drives
/// both the OpenAPI doc and email assets.
const BASE_URL_ENV: &str = "NAV_BASE_URL";

/// OSS placeholder origin. Real deploys set [`BASE_URL_ENV`]; this
/// default matches the one in `web::openapi` so the repo ships no
/// hard-coded NeonLaw hostname.
const DEFAULT_BASE_URL: &str = "https://www.your-domain.example";

/// Resolve the public origin for email assets from [`BASE_URL_ENV`],
/// falling back to [`DEFAULT_BASE_URL`]. Any trailing slash is left
/// for [`render_email_html`] to trim.
#[must_use]
pub fn base_url_from_env() -> String {
    std::env::var(BASE_URL_ENV).unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
}

/// Which brand's logo heads the email. Neon Law (the firm) and the
/// Neon Law Foundation (the 501(c)(3)) are distinct legal entities
/// with distinct marks: a Foundation notification — the NRS statutes
/// digest, say — must wear the Foundation logo, and a firm email the
/// firm's. Conflating them misattributes the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailBrand {
    /// Neon Law, the law firm. Logo: `/logo-firm.png`.
    Firm,
    /// Neon Law Foundation, the 501(c)(3). Logo: `/logo-foundation.png`.
    Foundation,
}

impl EmailBrand {
    /// Path (relative to the email `base_url`) of the brand's PNG logo.
    /// PNG, never SVG — email clients won't render an SVG `<img>` src.
    ///
    /// Served under `/public/` (the `ServeDir` static mount), **not** at
    /// the site root: `/public` is the path that exists as a route, so an
    /// email client fetching the logo unauthenticated gets the PNG (200).
    /// A root `/logo-*.png` is unrouted — every email's logo silently
    /// broke on it.
    #[must_use]
    pub fn logo_path(self) -> &'static str {
        match self {
            EmailBrand::Firm => "/public/logo-firm.png",
            EmailBrand::Foundation => "/public/logo-foundation.png",
        }
    }

    /// Accessible name / `alt` text for the logo image. Resolved through
    /// the same brand env vars as `views::brand` (`NAVIGATOR_BRAND_FIRM` /
    /// `NAVIGATOR_BRAND_FOUNDATION`) so a rebranded fork's email logo never
    /// carries NeonLaw's name as its accessible label. Defaults mirror the
    /// site brand.
    #[must_use]
    pub fn alt(self) -> String {
        match self {
            EmailBrand::Firm => std::env::var("NAVIGATOR_BRAND_FIRM")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "Neon Law".to_string()),
            EmailBrand::Foundation => std::env::var("NAVIGATOR_BRAND_FOUNDATION")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "Neon Law Foundation".to_string()),
        }
    }

    /// Inbound support address for the brand's footer, env-overridable.
    /// Defaults mirror `views::brand` (the firm at `support@neonlaw.com`,
    /// the Foundation at `support@neonlaw.org`) so the footer and the
    /// site agree on the real reply addresses.
    #[must_use]
    pub fn support_email(self) -> String {
        match self {
            EmailBrand::Firm => std::env::var("NAVIGATOR_SUPPORT_EMAIL")
                .unwrap_or_else(|_| "support@neonlaw.com".to_string()),
            EmailBrand::Foundation => std::env::var("NAVIGATOR_FOUNDATION_EMAIL")
                .unwrap_or_else(|_| "support@neonlaw.org".to_string()),
        }
    }
}

/// Render `content_markdown` into a self-contained, inline-styled HTML
/// email document headed by `brand`'s logo. `base_url` is the public
/// origin where the brand PNG (`/logo-firm.png` or
/// `/logo-foundation.png`) is served (e.g. from [`base_url_from_env`]);
/// a trailing slash is tolerated.
#[must_use]
pub fn render_email_html(content_markdown: &str, base_url: &str, brand: EmailBrand) -> String {
    let parser = Parser::new_ext(content_markdown, Options::empty());
    let mut content_html = String::new();
    html::push_html(&mut content_html, parser);

    let base = base_url.trim_end_matches('/');
    let logo = brand.logo_path();
    let alt = brand.alt();
    let support = brand.support_email();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
@font-face {{
  font-family: 'Noto Serif';
  font-style: normal;
  font-weight: 400;
  src: url('{base}/public/fonts/noto-serif/noto-serif-latin-400-normal.woff2') format('woff2');
}}
@font-face {{
  font-family: 'Noto Serif';
  font-style: normal;
  font-weight: 700;
  src: url('{base}/public/fonts/noto-serif/noto-serif-latin-700-normal.woff2') format('woff2');
}}
</style>
</head>
<body style="margin:0;padding:0;background:#f4f4f5;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f4f4f5;">
<tr><td align="center" style="padding:24px 12px;">
<table role="presentation" width="600" cellpadding="0" cellspacing="0" style="width:100%;max-width:600px;background:#ffffff;border-radius:8px;">
<tr><td style="padding:32px 32px 0;">
<img src="{base}{logo}" alt="{alt}" width="120" style="display:block;width:120px;height:auto;border:0;">
</td></tr>
<tr><td style="padding:8px 32px 24px;font-family:'Noto Serif',Georgia,'Times New Roman',serif;font-size:16px;line-height:1.5;color:#18181b;">
{content_html}</td></tr>
<tr><td style="padding:16px 32px 28px;border-top:1px solid #e4e4e7;font-family:'Noto Serif',Georgia,'Times New Roman',serif;font-size:13px;line-height:1.5;color:#71717a;">
{alt} · Reach us any time at <a href="mailto:{support}" style="color:#71717a;">{support}</a>.
</td></tr>
</table>
</td></tr>
</table>
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::{base_url_from_env, render_email_html, EmailBrand};

    #[test]
    fn renders_markdown_body_into_html() {
        let html = render_email_html("Hi **Aries**", "https://example.test", EmailBrand::Firm);
        assert!(
            html.contains("<strong>Aries</strong>"),
            "markdown is rendered"
        );
        assert!(html.starts_with("<!doctype html>"), "full document");
    }

    #[test]
    fn embeds_logo_png_at_base_url_and_trims_trailing_slash() {
        let html = render_email_html("body", "https://example.test/", EmailBrand::Firm);
        // Served from the exempt `/public` mount, not a gated site root.
        assert!(html.contains(r#"src="https://example.test/public/logo-firm.png""#));
        assert!(html.contains(r#"alt="Neon Law""#));
        // No double slash from a trailing-slash base.
        assert!(!html.contains("example.test//public/logo-firm.png"));
        // Never reference the SVG — clients won't render it.
        assert!(!html.contains("logo-firm.svg"));
    }

    #[test]
    fn foundation_brand_uses_foundation_logo_and_name() {
        // A Foundation notification (e.g. the NRS statutes digest) must
        // wear the Foundation mark, never the firm's.
        let html = render_email_html("body", "https://example.test", EmailBrand::Foundation);
        assert!(html.contains(r#"src="https://example.test/public/logo-foundation.png""#));
        assert!(html.contains(r#"alt="Neon Law Foundation""#));
        assert!(
            !html.contains("logo-firm.png"),
            "Foundation email must not carry the firm logo: {html}"
        );
    }

    #[test]
    fn body_is_set_in_noto_serif_with_serif_fallback() {
        let html = render_email_html("body", "https://mail.test", EmailBrand::Firm);
        // @font-face declared against the self-hosted woff2…
        assert!(
            html.contains(
                "src: url('https://mail.test/public/fonts/noto-serif/\
                 noto-serif-latin-400-normal.woff2') format('woff2')"
            ),
            "expected absolute @font-face src for Noto Serif regular: {html}"
        );
        // …and the body cell leads with Noto Serif, serif fallback for
        // clients (Gmail/Outlook) that strip the webfont.
        assert!(
            html.contains("font-family:'Noto Serif',Georgia,'Times New Roman',serif;"),
            "expected Noto-Serif-first serif font stack on the body cell: {html}"
        );
    }

    #[test]
    fn footer_carries_the_brand_support_address() {
        let firm = render_email_html("body", "https://b.test", EmailBrand::Firm);
        assert!(
            firm.contains("mailto:support@neonlaw.com"),
            "firm footer should carry the firm support address: {firm}"
        );
        let foundation = render_email_html("body", "https://b.test", EmailBrand::Foundation);
        assert!(
            foundation.contains("mailto:support@neonlaw.org"),
            "foundation footer should carry the foundation support address",
        );
    }

    #[test]
    fn autolinks_in_angle_brackets_become_anchors() {
        let html = render_email_html(
            "Visit <https://neonlaw.example> today",
            "https://b.test",
            EmailBrand::Firm,
        );
        assert!(html.contains(r#"href="https://neonlaw.example""#));
    }

    #[test]
    fn base_url_from_env_defaults_to_oss_placeholder_when_unset() {
        // The real value is injected via NAV_BASE_URL in deploys; the
        // default must stay a generic placeholder (no NeonLaw host).
        if std::env::var("NAV_BASE_URL").is_err() {
            assert_eq!(base_url_from_env(), "https://www.your-domain.example");
        }
    }
}
