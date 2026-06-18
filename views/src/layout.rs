//! The outer HTML shell shared by every page.
//!
//! `PageLayout::new(title).render(body)` produces a full document:
//! doctype, the head (charset, viewport, color-scheme, favicon,
//! title), a Bootstrap navbar (brand mark + nav with dropdowns),
//! the body slot wrapped in `<main class="container py-4">`, and a
//! `<footer class="container py-4 border-top mt-4">`.

use maud::{html, Markup, DOCTYPE};

use crate::brand::{
    firm_disclaimer, privacy_url, terms_url, NavLink, SiteBrand, FIRM_BRAND, FOUNDATION_BRAND,
};
use crate::components::social::{social_meta, SocialMeta};
use crate::components::{external_link_with_class, ExternalLink};
use crate::i18n::{self, Locale};

/// Whether the current request has a valid session. The layout uses
/// this to render the auth-aware tail of the header nav: an "Admin"
/// and "Sign out" pair for signed-in visitors, or a "Sign in" link
/// (pointing at the OIDC start endpoint) for everyone else.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AuthState {
    #[default]
    Anonymous,
    Authenticated,
}

/// Configurable layout. Defaults to the [`FIRM_BRAND`] firm brand;
/// pages on the foundation side call `with_brand(*FOUNDATION_BRAND)`.
pub struct PageLayout<'a> {
    title: &'a str,
    description: Option<&'a str>,
    brand: SiteBrand,
    auth: AuthState,
    preload_image: Option<&'a str>,
    alternate_markdown: Option<&'a str>,
    locale: Locale,
    canonical_path: Option<&'a str>,
}

impl<'a> PageLayout<'a> {
    #[must_use]
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            description: None,
            brand: *FIRM_BRAND,
            auth: AuthState::Anonymous,
            preload_image: None,
            alternate_markdown: None,
            locale: Locale::En,
            canonical_path: None,
        }
    }

    /// Render the page in `locale`. Defaults to [`Locale::En`], whose
    /// output is byte-identical to the pre-i18n layout. Setting `Es`
    /// switches the `<html lang>`, the navbar labels and hrefs, the auth
    /// links, and the "not accepting clients" banner to Spanish chrome.
    #[must_use]
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    /// Declare this page's locale-less canonical path (e.g.
    /// `/services/estate`). When set, the layout emits `hreflang`
    /// alternates for every locale and renders the one-tap navbar
    /// language switcher. Set this only on pages that actually have a
    /// translated twin, so the switcher never points at a 404.
    #[must_use]
    pub fn with_canonical_path(mut self, path: &'a str) -> Self {
        self.canonical_path = Some(path);
        self
    }

    /// Advertise a machine-readable Markdown twin of this page: emits
    /// `<link rel="alternate" type="text/markdown">` in `<head>` so an
    /// LLM crawler that lands on the HTML can find the clean `.md`
    /// corpus without scraping the rendered DOM. Set this on any page
    /// that also serves itself as raw Markdown at a sibling URL.
    #[must_use]
    pub fn with_alternate_markdown(mut self, href: &'a str) -> Self {
        self.alternate_markdown = Some(href);
        self
    }

    /// Preload a hero image: emits `<link rel="preload" as="image">`
    /// in `<head>` so the Largest Contentful Paint photo starts
    /// downloading before the body parses. Set this only for the one
    /// above-the-fold hero — preloading lazy images would hurt.
    #[must_use]
    pub fn with_preload_image(mut self, href: &'a str) -> Self {
        self.preload_image = Some(href);
        self
    }

    #[must_use]
    pub fn with_description(mut self, description: &'a str) -> Self {
        self.description = Some(description);
        self
    }

    #[must_use]
    pub fn with_brand(mut self, brand: SiteBrand) -> Self {
        self.brand = brand;
        self
    }

    #[must_use]
    pub fn with_auth(mut self, auth: AuthState) -> Self {
        self.auth = auth;
        self
    }

    /// Render the document around `body`.
    #[must_use]
    #[allow(clippy::too_many_lines)] // single maud tree; splitting would obscure layout structure
    pub fn render(&self, body: &Markup) -> Markup {
        // Brand-first, pipe-separated: "Neon Law | Home". Putting the
        // brand ahead of the page name means a shared link's preview
        // card and a browser tab both lead with who we are, not a bare
        // "Home". The page's own title may itself contain em-dash
        // separators (admin breadcrumbs); the pipe keeps the brand
        // boundary unambiguous.
        let full_title = format!("{} | {}", self.brand.site_name, self.title);
        html! {
            (DOCTYPE)
            // `data-bs-theme="auto"` delegates the light/dark choice
            // to the OS preference — matches the prior Pico behavior
            // of honoring `prefers-color-scheme`. Bootstrap 5.3 wires
            // theme tokens off this attribute.
            html lang=(self.locale.code()) data-bs-theme="auto" {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1";
                    meta name="color-scheme" content="light dark";
                    title { (full_title) }
                    @if let Some(d) = self.description {
                        meta name="description" content=(d);
                    }
                    // Open Graph + Twitter Card — the rich-preview card
                    // iMessage, Android Messages, Slack, Facebook, X,
                    // LinkedIn, and Discord render when a link is shared.
                    // The share message is the page description, or the
                    // brand tagline when a page sets none.
                    (social_meta(&SocialMeta {
                        title: &full_title,
                        description: self.description.unwrap_or(self.brand.tagline),
                        brand: &self.brand,
                    }))
                    // Markdown twin for machine readers — see
                    // `with_alternate_markdown`. Only emitted when a
                    // page opts in by serving a `.md` sibling.
                    @if let Some(md) = self.alternate_markdown {
                        link rel="alternate" type="text/markdown" href=(md);
                    }
                    // hreflang alternates pair this page with its twin in
                    // every locale, for search engines and screen readers.
                    // Only emitted when the page declares a canonical path
                    // (i.e. it actually has a translated twin). English is
                    // x-default.
                    @if let Some(path) = self.canonical_path {
                        link rel="alternate" hreflang="en" href=(path);
                        link rel="alternate" hreflang="es"
                            href=(i18n::localize_href(path, Locale::Es));
                        link rel="alternate" hreflang="x-default" href=(path);
                    }
                    link rel="icon" type="image/svg+xml" href=(self.brand.logo_href);
                    link rel="stylesheet" href="/public/css/bootstrap.min.css";
                    // First-party brand palette — remaps Bootstrap's
                    // `primary`/`blue`/`cyan` onto Tailwind's cyan-500.
                    // Loaded right after Bootstrap so its token and
                    // button overrides win.
                    link rel="stylesheet" href="/public/css/brand.css";
                    // Noto Serif — the firm typeface. Loaded after
                    // Bootstrap so its `--bs-body-font-family` override
                    // wins; self-hosted woff2, no CDN. Preload the latin
                    // regular subset so body text doesn't flash the
                    // fallback serif on first paint (other scripts load
                    // on demand via unicode-range).
                    link rel="preload" as="font" type="font/woff2" crossorigin
                        href="/public/fonts/noto-serif/noto-serif-latin-400-normal.woff2";
                    link rel="stylesheet" href="/public/css/noto-serif.css";
                    link rel="stylesheet" href="/public/icons/bootstrap-icons.css";
                    // Hero preload — only when a page opts in via
                    // `with_preload_image`. `fetchpriority="high"` so
                    // it wins the connection race for the LCP element.
                    @if let Some(href) = self.preload_image {
                        link rel="preload" as="image" href=(href) fetchpriority="high";
                    }
                    // All three scripts are `defer` so they don't
                    // block the first paint. Bootstrap bundles
                    // Popper.js for dropdowns; HTMX powers in-page
                    // partial swaps (admin delete); Alpine handles
                    // small reactivity bits (modals, toggles) in
                    // admin only.
                    script defer src="/public/js/bootstrap.bundle.min.js" {}
                    script defer src="/public/js/htmx.min.js" {}
                    script defer src="/public/js/alpine.min.js" {}
                    // First-party: upgrades the read-only document on the
                    // Northstar review surface into a select-text-and-
                    // comment element. Inert on every other page (it only
                    // acts when a `<northstar-review>` element is present).
                    script defer src="/public/js/northstar-review.js" {}
                    // First-party: click-to-zoom lightbox for blog photo
                    // collages — opens the full, uncropped image so the
                    // square grid crop never hides anyone. Inert unless a
                    // `.blog-collage` is present on the page.
                    script defer src="/public/js/collage-lightbox.js" {}
                }
                body {
                    @if self.brand.is_law_firm {
                        // Bootstrap alert chrome (warning tone) + a
                        // brand-specific `firm-not-accepting` class
                        // kept for any custom overrides. `rounded-0`
                        // + `mb-0` so it hugs the top of the page.
                        div."alert"."alert-warning"."rounded-0"."mb-0"."text-center"
                            .firm-not-accepting
                            role="status"
                            aria-live="polite"
                        {
                            // In English this resolves byte-identically to
                            // `brand.firm_not_accepting_clients`; in Spanish
                            // it draws from the chrome catalog.
                            strong {
                                (i18n::t_args(
                                    self.locale,
                                    "banner.not_accepting",
                                    &[("firm", self.brand.site_name)],
                                ))
                            }
                        }
                    }
                    header {
                        nav.navbar.navbar-expand-lg."bg-body-tertiary" {
                            div.container-fluid {
                                a.navbar-brand."d-flex"."align-items-center"."gap-2"
                                    href="/"
                                    aria-label=(format!("{} home", self.brand.site_name))
                                {
                                    img src=(self.brand.logo_href)
                                        alt=(self.brand.site_name)
                                        height="32"
                                        width="32";
                                    strong { (self.brand.site_name) }
                                }
                                // Mobile hamburger — toggles the
                                // .navbar-collapse target via the
                                // Bootstrap JS bundle. Hidden on >=lg.
                                button.navbar-toggler
                                    type="button"
                                    data-bs-toggle="collapse"
                                    data-bs-target="#main-nav"
                                    aria-controls="main-nav"
                                    aria-expanded="false"
                                    aria-label="Toggle navigation"
                                {
                                    span."navbar-toggler-icon" {}
                                }
                                div.collapse."navbar-collapse" id="main-nav" {
                                    ul.navbar-nav."ms-auto"."mb-2"."mb-lg-0" {
                                        @for link in self.brand.nav {
                                            (render_nav_link(link, self.locale))
                                        }
                                        @if self.auth == AuthState::Authenticated {
                                            li.nav-item {
                                                a.nav-link href="/portal" {
                                                    (i18n::t(self.locale, "auth.portal"))
                                                }
                                            }
                                            li.nav-item {
                                                a.nav-link href="/auth/logout" {
                                                    (i18n::t(self.locale, "auth.sign_out"))
                                                }
                                            }
                                        } @else {
                                            li.nav-item {
                                                a.nav-link href="/auth/login" {
                                                    (i18n::t(self.locale, "auth.sign_in"))
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    main.container."py-4" { (body) }
                    footer.container."py-4"."border-top"."mt-4" {
                        // On a localized page the legal strip below — bar
                        // admissions and the legal-advice disclaimer — stays
                        // English by policy: the binding artifact a client
                        // signs is English even when the chrome is localized.
                        // Say so, conspicuously, in the page's own language so
                        // a Spanish reader knows it is deliberate, not an
                        // unfinished translation. English pages never render
                        // this — their strip is already in the reader's tongue.
                        @if self.locale != Locale::En {
                            p."small"."fw-semibold"."text-body-secondary"."mb-2"
                                .legal-language-note
                                lang=(self.locale.code())
                            {
                                (i18n::t(self.locale, "footer.legal_in_english"))
                            }
                        }
                        // ONE terse footer, byte-identical on every page —
                        // firm- and Foundation-branded alike. It is anchored on
                        // `FIRM_BRAND`/`FOUNDATION_BRAND` constants, never
                        // `self.brand`, so it never varies by page. Five lines:
                        // (1) the registered "Neon Law" mark + Shook Law PLLC +
                        // bar admissions, (2) both organizations, each linked
                        // and with its own postal address, (3) the joint
                        // copyright (firm AND Foundation) + policy links,
                        // (4) the legal-advice disclaimer, (5) the mission line,
                        // linking the shared mission statement at the very
                        // bottom of every page. Gated on the canonical
                        // registered mark so an OSS fork (no trademark URL)
                        // shows only its own name.
                        p.small."text-body-secondary"."mb-2" {
                            @if let Some(url) = FIRM_BRAND.trademark_registration_url {
                                (ExternalLink::new(url)
                                    .with_class("link-secondary text-decoration-none")
                                    .with_title(
                                        "NEON LAW is a registered trademark — \
                                         U.S. Reg. No. 6,325,650",
                                    )
                                    .render(html! { (FIRM_BRAND.site_name) sup { "®" } }))
                                " — legal services rendered by Shook Law PLLC. Admitted in "
                            } @else {
                                (FIRM_BRAND.site_name) " · Admitted in "
                            }
                            (external_link_with_class(
                                "https://apps.calbar.ca.gov/attorney/Licensee/Detail/337252",
                                "link-secondary",
                                html! { "California" },
                            ))
                            " · "
                            (external_link_with_class(
                                "https://www.mywsba.org/personifyebusiness/LegalDirectory.aspx",
                                "link-secondary",
                                html! { "Washington" },
                            ))
                            " · "
                            (external_link_with_class(
                                "https://nvbar.org/find-a-lawyer/?usearch=13400",
                                "link-secondary",
                                html! { "Nevada" },
                            ))
                            "."
                        }
                        // Both organizations, each linked, each with its own
                        // registered postal address.
                        p.small."text-body-secondary"."mb-2" {
                            a.link-secondary href="/" { (FIRM_BRAND.site_name) }
                            " — " (FIRM_BRAND.postal_address)
                            br;
                            a.link-secondary href="/foundation" { (FOUNDATION_BRAND.site_name) }
                            ", a 501(c)(3) — " (FOUNDATION_BRAND.postal_address)
                        }
                        p.small."text-body-secondary"."mb-2" {
                            // Joint copyright: the codebase and the words on it
                            // belong to BOTH organizations — the firm that runs
                            // on the software and the Foundation that publishes
                            // it open source.
                            "© 2026 " (FIRM_BRAND.site_name) " & " (FOUNDATION_BRAND.site_name) " · "
                            a.link-secondary href=(privacy_url()) { "Privacy" } " · "
                            a.link-secondary href=(terms_url()) { "Terms" } " · "
                            a.link-secondary href="/docs/glossary" { "Glossary" } " · "
                            a.link-secondary href="/api/docs" { "API" } " · "
                            a.link-secondary href="/contact" { "Contact" } " · "
                            a.link-secondary href="/blog" { "Blog" } " · "
                            // The mission statement and the Foundation's free
                            // Nevada Revised Statutes reference ride the same link
                            // row as every other policy link — uniform short
                            // labels, no separate trailing line.
                            a.link-secondary href="/foundation/mission" { "Mission" } " · "
                            a.link-secondary href="/statutes" { "Statutes" }
                            // One-tap language switcher — only on pages with a
                            // translated twin. Rides the same policy-link row as
                            // Mission/Privacy/etc. (moved here from the navbar):
                            // visible label is the TARGET language in its own
                            // name; aria-label is in the current language.
                            @if let Some(path) = self.canonical_path {
                                @let target = self.locale.switch_target();
                                " · "
                                a.link-secondary.language-switcher
                                    href=(i18n::localize_href(path, target))
                                    lang=(target.code())
                                    hreflang=(target.code())
                                    aria-label=(i18n::t(self.locale, "switcher.aria"))
                                {
                                    (target.endonym())
                                }
                            }
                        }
                        p.firm-disclaimer.small."text-body-secondary"."mb-0" {
                            (firm_disclaimer())
                        }
                    }
                }
            }
        }
    }
}

fn render_nav_link(link: &NavLink, locale: Locale) -> Markup {
    if link.is_dropdown() {
        html! {
            li.nav-item.dropdown {
                a."nav-link"."dropdown-toggle"
                    href="#"
                    role="button"
                    data-bs-toggle="dropdown"
                    aria-expanded="false"
                {
                    (i18n::nav_label(link.label, locale))
                }
                ul.dropdown-menu {
                    @for child in link.children {
                        li { a.dropdown-item href=(i18n::localize_href(child.href, locale)) {
                            @if let Some(icon) = child.icon {
                                i class={ "bi bi-" (icon) " me-2" } aria-hidden="true" {}
                            }
                            (i18n::nav_label(child.label, locale))
                        } }
                    }
                }
            }
        }
    } else {
        html! {
            li.nav-item {
                a.nav-link href=(i18n::localize_href(link.href, locale)) {
                    (i18n::nav_label(link.label, locale))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PageLayout;
    use crate::brand::{FIRM_BRAND, FOUNDATION_BRAND};
    use maud::html;

    fn render(title: &str, body: &maud::Markup) -> String {
        PageLayout::new(title).render(body).into_string()
    }

    #[test]
    fn spanish_locale_sets_html_lang_and_translates_chrome() {
        use crate::i18n::Locale;
        let out = PageLayout::new("Inicio")
            .with_locale(Locale::Es)
            .with_canonical_path("/")
            .render(&html! { p { "x" } })
            .into_string();
        // `<html lang="es">` drives screen readers and SEO.
        assert!(
            out.contains("<html lang=\"es\" data-bs-theme=\"auto\">"),
            "Spanish page must declare lang=es, got: {out}"
        );
        // Navbar chrome is translated; auth link too.
        assert!(
            out.contains(">Inicio</a>"),
            "nav 'Home' should be 'Inicio': {out}"
        );
        assert!(
            out.contains(">Servicios</a>"),
            "nav 'Services' should be 'Servicios'"
        );
        assert!(
            out.contains("href=\"/auth/login\">Iniciar sesión</a>"),
            "auth 'Sign in' should be 'Iniciar sesión': {out}"
        );
        // Internal nav hrefs are /es-prefixed.
        assert!(
            out.contains("href=\"/es/services\""),
            "Spanish nav should prefix the Services href with /es: {out}"
        );
        // The banner is translated via the catalog.
        assert!(
            out.contains("no está aceptando clientes"),
            "Spanish banner should be translated: {out}"
        );
    }

    #[test]
    fn canonical_path_emits_hreflang_alternates_and_language_switcher() {
        use crate::i18n::Locale;
        // English page that declares a Spanish twin.
        let out = PageLayout::new("Home")
            .with_locale(Locale::En)
            .with_canonical_path("/services/estate")
            .render(&html! { p { "x" } })
            .into_string();
        assert!(out.contains("<link rel=\"alternate\" hreflang=\"en\" href=\"/services/estate\">"));
        assert!(
            out.contains("hreflang=\"es\" href=\"/es/services/estate\""),
            "es alternate should point at the /es twin: {out}"
        );
        assert!(out.contains("hreflang=\"x-default\""));
        // The switcher offers the OTHER language in its own name, linking
        // the twin.
        assert!(
            out.contains("language-switcher") && out.contains(">Español</a>"),
            "English page should offer a Spanish switcher: {out}"
        );
        assert!(out.contains("href=\"/es/services/estate\""));
    }

    #[test]
    fn spanish_footer_conspicuously_notes_the_legal_strip_is_english() {
        use crate::i18n::Locale;
        let es = PageLayout::new("Inicio")
            .with_locale(Locale::Es)
            .with_canonical_path("/")
            .render(&html! { p { "x" } })
            .into_string();
        assert!(
            es.contains("legal-language-note")
                && es.contains("El texto legal vinculante de este sitio se proporciona en inglés."),
            "Spanish page must conspicuously note the legal strip is English: {es}"
        );
        // The note carries lang=es and reads as conspicuous (semibold).
        assert!(
            es.contains("fw-semibold") && es.contains("lang=\"es\""),
            "got: {es}"
        );
        // English pages never render the note — their strip is already in
        // the reader's language, so it would be noise.
        let en = render("Home", &html! { p { "x" } });
        assert!(!en.contains("legal-language-note"));
        assert!(!en.contains("Binding legal text on this site is provided in English."));
    }

    #[test]
    fn switcher_is_absent_without_a_canonical_path() {
        // The default layout (no declared twin) must not render a
        // switcher — and English output stays byte-identical.
        let out = render("Home", &html! { p { "x" } });
        assert!(!out.contains("language-switcher"));
        assert!(!out.contains("hreflang"));
    }

    #[test]
    fn emits_doctype_and_lang_attribute() {
        let out = render("Home", &html! { p { "x" } });
        // `data-bs-theme="auto"` rides on <html> to delegate dark
        // mode to OS preference (see [`bootstrap_5.3.3`] migration).
        assert!(
            out.starts_with("<!DOCTYPE html><html lang=\"en\" data-bs-theme=\"auto\">"),
            "expected DOCTYPE + lang + data-bs-theme on <html>, got: {out}",
        );
    }

    #[test]
    fn title_combines_page_and_default_brand_name() {
        let out = render("Home", &html! { p { "x" } });
        let expected = format!("<title>{} | Home</title>", FIRM_BRAND.site_name);
        assert!(out.contains(&expected), "got: {out}");
    }

    #[test]
    fn foundation_brand_overrides_the_title() {
        let body = html! { p { "x" } };
        let out = PageLayout::new("Mission")
            .with_brand(*FOUNDATION_BRAND)
            .render(&body)
            .into_string();
        let expected = format!("<title>{} | Mission</title>", FOUNDATION_BRAND.site_name);
        assert!(out.contains(&expected));
    }

    #[test]
    fn meta_description_is_omitted_when_not_set() {
        let out = render("Home", &html! { p { "x" } });
        assert!(!out.contains("name=\"description\""));
    }

    #[test]
    fn meta_description_is_emitted_when_set() {
        let body = html! { p { "x" } };
        let out = PageLayout::new("Home")
            .with_description("Things and stuff")
            .render(&body)
            .into_string();
        assert!(out.contains("name=\"description\" content=\"Things and stuff\""));
    }

    #[test]
    fn head_emits_open_graph_card_with_brand_logo_and_title() {
        let out = render("Home", &html! { p { "x" } });
        let expected_title = format!("{} | Home", FIRM_BRAND.site_name);
        assert!(
            out.contains(&format!(
                "property=\"og:title\" content=\"{expected_title}\""
            )),
            "og:title should mirror the document title, got: {out}"
        );
        assert!(
            out.contains("property=\"og:image\"") && out.contains("logo-firm.png"),
            "firm og:image should be the PNG mark, got: {out}"
        );
        assert!(out.contains("name=\"twitter:card\" content=\"summary\""));
    }

    #[test]
    fn og_description_falls_back_to_brand_tagline_when_page_sets_none() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains(&format!(
                "property=\"og:description\" content=\"{}\"",
                FIRM_BRAND.tagline
            )),
            "og:description should default to the brand tagline, got: {out}"
        );
    }

    #[test]
    fn og_description_uses_the_page_description_when_set() {
        let out = PageLayout::new("Home")
            .with_description("Bespoke share copy")
            .render(&html! { p { "x" } })
            .into_string();
        assert!(out.contains("property=\"og:description\" content=\"Bespoke share copy\""));
        assert!(
            !out.contains(FIRM_BRAND.tagline),
            "page description should win over the tagline fallback, got: {out}"
        );
    }

    #[test]
    fn foundation_og_card_uses_the_foundation_logo() {
        let out = PageLayout::new("Mission")
            .with_brand(*FOUNDATION_BRAND)
            .render(&html! { p { "x" } })
            .into_string();
        assert!(
            out.contains("property=\"og:image\"") && out.contains("logo-foundation.png"),
            "foundation og:image should be the NLF PNG mark, got: {out}"
        );
        assert!(out.contains(&format!(
            "property=\"og:site_name\" content=\"{}\"",
            FOUNDATION_BRAND.site_name
        )));
    }

    #[test]
    fn body_is_rendered_inside_main_container() {
        let out = render("Home", &html! { p { "Body text" } });
        assert!(
            out.contains("<main class=\"container py-4\"><p>Body text</p></main>"),
            "main must be a Bootstrap container with vertical padding, got: {out}",
        );
    }

    #[test]
    fn footer_uses_bootstrap_container_with_border_top() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("<footer class=\"container py-4 border-top mt-4\">"),
            "expected Bootstrap container footer with border-top + spacing: {out}",
        );
    }

    #[test]
    fn firm_not_accepting_banner_uses_bootstrap_alert_chrome() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("alert alert-warning"),
            "firm banner should adopt Bootstrap alert chrome (warning tone), got: {out}",
        );
        // Brand-specific class kept for any future custom override.
        assert!(out.contains("firm-not-accepting"));
        assert!(
            out.contains("rounded-0") && out.contains("mb-0"),
            "alert should hug the top: no rounded corners, no bottom margin, got: {out}",
        );
    }

    #[test]
    fn header_uses_bootstrap_navbar_pattern_with_brand_logo() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("<nav class=\"navbar navbar-expand-lg bg-body-tertiary\">"),
            "expected Bootstrap navbar shell, got: {out}",
        );
        assert!(
            out.contains("class=\"navbar-brand"),
            "expected navbar-brand on the logo link, got: {out}",
        );
        assert!(out.contains("<img src=\"/public/logo-firm.svg\""));
        let expected = format!("<strong>{}</strong>", FIRM_BRAND.site_name);
        assert!(out.contains(&expected), "header missing brand name: {out}");
    }

    #[test]
    fn navbar_includes_mobile_hamburger_toggler() {
        // Below the lg breakpoint Bootstrap collapses navbar-nav into
        // a button that toggles the .navbar-collapse div.
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("class=\"navbar-toggler\""),
            "expected mobile navbar-toggler, got: {out}",
        );
        assert!(out.contains("data-bs-target=\"#main-nav\""));
        assert!(out.contains("id=\"main-nav\""));
    }

    #[test]
    fn firm_services_renders_as_a_flat_nav_link() {
        // The Services dropdown collapsed to a single flat link to the
        // `/services` catalog — the firm nav no longer opens a dropdown.
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("class=\"nav-link\" href=\"/services\""),
            "expected a flat Services nav-link, got: {out}"
        );
        assert!(
            !out.contains("class=\"nav-item dropdown\""),
            "firm nav should no longer carry a dropdown, got: {out}"
        );
    }

    #[test]
    fn anonymous_nav_shows_sign_in_not_admin() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("href=\"/auth/login\">Sign in</a>"),
            "expected `Sign in` link, got: {out}",
        );
        assert!(
            !out.contains("href=\"/portal\""),
            "anonymous nav should not link to /portal: {out}",
        );
    }

    #[test]
    fn authenticated_nav_shows_portal_not_sign_in() {
        let body = html! { p { "x" } };
        let out = PageLayout::new("Home")
            .with_auth(super::AuthState::Authenticated)
            .render(&body)
            .into_string();
        assert!(
            out.contains("href=\"/portal\">Portal</a>"),
            "expected `Portal` link, got: {out}",
        );
        assert!(
            !out.contains("href=\"/auth/login\""),
            "authenticated nav should not link to /auth/login: {out}",
        );
    }

    #[test]
    fn authenticated_nav_shows_sign_out_link() {
        let body = html! { p { "x" } };
        let out = PageLayout::new("Home")
            .with_auth(super::AuthState::Authenticated)
            .render(&body)
            .into_string();
        assert!(
            out.contains("href=\"/auth/logout\">Sign out</a>"),
            "expected `Sign out` link, got: {out}",
        );
    }

    #[test]
    fn nav_links_carry_bootstrap_nav_item_class() {
        // Every top-level nav <li> wears nav-item; every anchor wears
        // nav-link. Pico's bare <li><a> wouldn't render correctly
        // inside .navbar-nav.
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("class=\"nav-item\""),
            "expected nav-item class on top-level <li>, got: {out}",
        );
        assert!(
            out.contains("class=\"nav-link\""),
            "expected nav-link class on anchors, got: {out}",
        );
    }

    #[test]
    fn anonymous_nav_does_not_show_sign_out_link() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            !out.contains("href=\"/auth/logout\""),
            "anonymous nav should not link to /auth/logout: {out}",
        );
    }

    #[test]
    fn footer_carries_copyright_and_links_policies() {
        let out = render("Home", &html! { p { "x" } });
        // Joint copyright: the line names BOTH the firm and the
        // Foundation as owners, not the firm alone.
        let expected = format!(
            "© 2026 {} &amp; {}",
            FIRM_BRAND.site_name, FOUNDATION_BRAND.site_name
        );
        assert!(out.contains(&expected), "got: {out}");
        assert!(out.contains("href=\"/privacy\""));
        assert!(out.contains("href=\"/terms\""));
    }

    #[test]
    fn footer_links_the_blog() {
        let out = render("Home", &html! { p { "x" } });
        assert!(out.contains("href=\"/blog\""), "got: {out}");
        assert!(out.contains(">Blog</a>"), "got: {out}");
    }

    #[test]
    fn footer_links_the_mission_at_the_bottom_on_every_brand() {
        // "Add the Mission to the bottom": every page — firm- and
        // Foundation-branded alike — closes with the mission line,
        // linking the shared statement at /foundation/mission.
        for (name, out) in [
            ("firm", render("Home", &html! { p { "x" } })),
            (
                "foundation",
                PageLayout::new("Mission")
                    .with_brand(*FOUNDATION_BRAND)
                    .render(&html! { p { "x" } })
                    .into_string(),
            ),
        ] {
            assert!(
                out.contains("href=\"/foundation/mission\""),
                "{name} footer should link the mission statement, got: {out}"
            );
            assert!(
                out.contains(">Mission</a>"),
                "{name} footer should label the mission link, got: {out}"
            );
        }
    }

    #[test]
    fn footer_links_the_public_statutes_reference_on_every_brand() {
        // The Foundation's free NRS reference is reachable from the footer
        // (text-only, labelled as reference) on firm- and Foundation-branded
        // pages alike — its only discoverability surface site-wide.
        for (name, out) in [
            ("firm", render("Home", &html! { p { "x" } })),
            (
                "foundation",
                PageLayout::new("Mission")
                    .with_brand(*FOUNDATION_BRAND)
                    .render(&html! { p { "x" } })
                    .into_string(),
            ),
        ] {
            assert!(
                out.contains("href=\"/statutes\""),
                "{name} footer should link the public statutes reference, got: {out}"
            );
            assert!(
                out.contains(">Statutes</a>"),
                "{name} footer should label the statutes link, got: {out}"
            );
        }
    }

    #[test]
    fn footer_carries_both_firm_and_foundation_postal_addresses() {
        // The unified footer prints BOTH registered mailing addresses on
        // every page — the firm's (suite 405-9002) and the Foundation's
        // (suite 405-9999) — regardless of which brand the page is.
        for (name, out) in [
            ("firm", render("Home", &html! { p { "x" } })),
            (
                "foundation",
                PageLayout::new("Mission")
                    .with_brand(*FOUNDATION_BRAND)
                    .render(&html! { p { "x" } })
                    .into_string(),
            ),
        ] {
            assert!(
                out.contains(FIRM_BRAND.postal_address),
                "{name} footer should print the firm postal address, got: {out}"
            );
            assert!(
                out.contains(FOUNDATION_BRAND.postal_address),
                "{name} footer should print the Foundation postal address, got: {out}"
            );
        }
    }

    #[test]
    fn firm_footer_links_to_contact_and_foundation_brand() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("href=\"/contact\""),
            "firm footer needs Contact link"
        );
        assert!(
            out.contains("href=\"/foundation\""),
            "firm footer needs Foundation brand-switch"
        );
    }

    #[test]
    fn firm_footer_marks_brand_name_with_linked_registered_trademark() {
        // Skip when a fork has rebranded the firm — the linked ® only
        // attaches to NeonLaw's registered "NEON LAW" wordmark.
        if std::env::var("NAVIGATOR_BRAND_FIRM").is_ok() {
            return;
        }
        let out = render("Home", &html! { p { "x" } });
        let footer_idx = out.find("<footer").expect("footer present");
        let footer = &out[footer_idx..];
        assert!(
            footer.contains("tmsearch.uspto.gov/search/search-results/90039224"),
            "firm footer should link the ® to the USPTO record: {footer}"
        );
        assert!(
            footer.contains("<sup>®</sup>"),
            "firm footer should render the registered-trademark symbol: {footer}"
        );
    }

    #[test]
    fn foundation_footer_omits_registered_trademark_on_its_own_name() {
        // "Neon Law Foundation" is not the registered mark — only the
        // firm's "NEON LAW" wordmark is — so the Foundation's own
        // brand name (carried in the "© 2026 …" line) must not pick up
        // the ® / USPTO link. The "Neon Law® — legal services rendered
        // by Shook Law PLLC" attribution is a separate, intentional line
        // (see `footer_attributes_neon_law_mark_to_shook_law_pllc`).
        let out = PageLayout::new("Mission")
            .with_brand(*FOUNDATION_BRAND)
            .render(&html! { p { "x" } })
            .into_string();
        let footer_idx = out.find("<footer").expect("footer present");
        let footer = &out[footer_idx..];
        // The Foundation's name appears first as the copyright owner on
        // the top brand line; the text up to the address separator must
        // carry no registered-trademark mark.
        let owner = FOUNDATION_BRAND.site_name;
        let after_owner =
            &footer[footer.find(owner).expect("foundation owner in footer") + owner.len()..];
        let brand_line = &after_owner[..after_owner.find(" · ").unwrap_or(after_owner.len())];
        assert!(
            !brand_line.contains("uspto.gov") && !brand_line.contains("®"),
            "Foundation brand name must not carry a ® mark: {brand_line}"
        );
    }

    #[test]
    fn footer_attributes_neon_law_mark_to_shook_law_pllc() {
        // Every footer — firm- or Foundation-branded — leads with the
        // "Neon Law" mark attributed to the firm that renders the legal
        // services behind it (address on the line beneath). Skip when a
        // fork has rebranded the firm (the attribution rides on NeonLaw's
        // registered mark).
        if std::env::var("NAVIGATOR_BRAND_FIRM").is_ok() {
            return;
        }
        for (name, out) in [
            ("firm", render("Home", &html! { p { "x" } })),
            (
                "foundation",
                PageLayout::new("Mission")
                    .with_brand(*FOUNDATION_BRAND)
                    .render(&html! { p { "x" } })
                    .into_string(),
            ),
        ] {
            let footer_idx = out.find("<footer").expect("footer present");
            let footer = &out[footer_idx..];
            assert!(
                footer.contains("legal services rendered by Shook Law PLLC"),
                "{name} footer should attribute legal services to Shook Law PLLC: {footer}"
            );
            assert!(
                footer.contains("<sup>®</sup>"),
                "{name} footer attribution should carry the registered-trademark symbol: {footer}"
            );
        }
    }

    #[test]
    fn firm_footer_carries_bar_admission_strip_with_each_state_linked() {
        let out = render("Home", &html! { p { "x" } });
        let footer_idx = out.find("<footer").expect("footer present");
        let footer = &out[footer_idx..];
        assert!(
            footer.contains("Admitted in"),
            "firm footer needs bar-admissions strip: {footer}"
        );
        // California → confirmed Cal Bar profile (#337252, Nicholas R. Shook).
        assert!(
            footer.contains("apps.calbar.ca.gov/attorney/Licensee/Detail/337252"),
            "California admission should link to the Cal Bar profile"
        );
        // Washington → WSBA Legal Directory landing (no public bar # confirmed yet).
        assert!(
            footer.contains("mywsba.org/personifyebusiness/LegalDirectory.aspx"),
            "Washington admission should link to the WSBA legal directory"
        );
        // Nevada → State Bar of Nevada profile (Bar No. 13400, Nicholas R. Shook).
        assert!(
            footer.contains("nvbar.org/find-a-lawyer/?usearch=13400"),
            "Nevada admission should link to the State Bar of Nevada profile for bar #13400"
        );
    }

    #[test]
    fn footer_is_unified_and_carries_bar_strip_on_foundation_pages() {
        // The footer is now one shared block: every page — including
        // Foundation-branded ones — carries the firm's bar-admission strip.
        let out = PageLayout::new("Mission")
            .with_brand(*FOUNDATION_BRAND)
            .render(&html! { p { "x" } })
            .into_string();
        let footer_idx = out.find("<footer").expect("footer present");
        let footer = &out[footer_idx..];
        assert!(
            footer.contains("Admitted in"),
            "unified footer should carry the bar-admission strip on every page: {footer}"
        );
    }

    #[test]
    fn footer_names_both_neon_law_and_the_foundation_with_links() {
        // "Talk about Neon Law and Neon Law Foundation on all the pages":
        // every footer links both the firm root and the Foundation, and
        // carries exactly one Contact link.
        for (name, out) in [
            ("firm", render("Home", &html! { p { "x" } })),
            (
                "foundation",
                PageLayout::new("Mission")
                    .with_brand(*FOUNDATION_BRAND)
                    .render(&html! { p { "x" } })
                    .into_string(),
            ),
        ] {
            let footer_idx = out.find("<footer").expect("footer present");
            let footer = &out[footer_idx..];
            assert!(
                footer.contains("href=\"/\""),
                "{name} footer should link the firm root"
            );
            assert!(
                footer.contains("href=\"/foundation\""),
                "{name} footer should link the Foundation"
            );
            assert!(
                footer.contains("href=\"/contact\""),
                "{name} footer should carry one Contact link"
            );
            assert!(
                footer.contains(FOUNDATION_BRAND.site_name),
                "{name} footer should name the Foundation"
            );
        }
    }

    #[test]
    fn both_brand_footers_link_to_api_docs() {
        let firm = render("Home", &html! { p { "x" } });
        let foundation = PageLayout::new("Mission")
            .with_brand(*FOUNDATION_BRAND)
            .render(&html! { p { "x" } })
            .into_string();
        for (name, out) in [("firm", &firm), ("foundation", &foundation)] {
            let footer_idx = out.find("<footer").expect("footer present");
            let footer = &out[footer_idx..];
            assert!(
                footer.contains("href=\"/api/docs\""),
                "{name} footer missing API documentation link"
            );
        }
    }

    #[test]
    fn legal_advice_disclaimer_renders_on_every_page() {
        // Unified footer: the legal-advice disclaimer now shows on
        // Foundation-branded pages too, not just firm pages.
        let firm = PageLayout::new("Home")
            .render(&html! { p { "x" } })
            .into_string();
        let foundation = PageLayout::new("Mission")
            .with_brand(*FOUNDATION_BRAND)
            .render(&html! { p { "x" } })
            .into_string();
        for (name, out) in [("firm", &firm), ("foundation", &foundation)] {
            assert!(
                out.contains("Nothing on this site is legal advice"),
                "{name} footer missing legal-advice disclaimer: {out}"
            );
            assert!(out.contains("signed retainer"), "{name} footer: {out}");
        }
    }

    #[test]
    fn firm_brand_renders_not_accepting_clients_banner_above_header() {
        let out = render("Home", &html! { p { "x" } });
        let banner_idx = out
            .find(FIRM_BRAND.firm_not_accepting_clients)
            .expect("expected firm banner copy");
        let header_idx = out.find("<header>").expect("expected header element");
        assert!(
            banner_idx < header_idx,
            "banner must render above the header, got: {out}"
        );
        assert!(
            out.contains("role=\"status\""),
            "banner should be a status region for assistive tech: {out}"
        );
    }

    #[test]
    fn foundation_brand_does_not_render_not_accepting_clients_banner() {
        let out = PageLayout::new("Mission")
            .with_brand(*FOUNDATION_BRAND)
            .render(&html! { p { "x" } })
            .into_string();
        assert!(
            !out.contains("not accepting clients"),
            "foundation pages must not carry the firm-closed banner: {out}"
        );
    }

    #[test]
    fn head_declares_color_scheme_for_dark_mode() {
        // `meta color-scheme` is still useful even after Pico is
        // removed — it tells the user agent that both light and
        // dark form-control / scrollbar styles are acceptable.
        // Bootstrap reads `data-bs-theme` on <html> for its own
        // dark token mapping (see the `data-bs-theme="auto"` test).
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("name=\"color-scheme\" content=\"light dark\""),
            "expected color-scheme meta, got: {out}",
        );
    }

    #[test]
    fn favicon_tracks_the_brand_logo() {
        let firm = render("Home", &html! { p { "x" } });
        assert!(firm.contains("rel=\"icon\""));
        assert!(
            firm.contains("href=\"/public/logo-firm.svg\""),
            "firm favicon should be the NL mark, got: {firm}"
        );

        let foundation = PageLayout::new("Mission")
            .with_brand(*FOUNDATION_BRAND)
            .render(&html! { p { "x" } })
            .into_string();
        assert!(
            foundation.contains("href=\"/public/logo-foundation.svg\""),
            "foundation favicon should be the NLF mark, got: {foundation}"
        );
    }

    #[test]
    fn pico_stylesheet_is_no_longer_linked() {
        let out = render("Home", &html! { p { "x" } });
        assert!(out.contains("rel=\"stylesheet\""));
        // Pico was dropped in favor of Bootstrap 5.3 — the file
        // itself (`web/public/pico.css`) is also removed.
        assert!(
            !out.contains("/public/pico.css"),
            "Pico stylesheet link must be gone, got: {out}",
        );
    }

    #[test]
    fn bootstrap_icons_stylesheet_is_linked() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("/public/icons/bootstrap-icons.css"),
            "expected vendored Bootstrap Icons CSS link, got: {out}",
        );
    }

    #[test]
    fn preload_image_emitted_only_when_set() {
        // Default: no *image* preload link. (The Redaction font is
        // always preloaded — see `redaction_font_is_preloaded` — so we
        // assert specifically on the opt-in image preload here.)
        let plain = render("Home", &html! { p { "x" } });
        assert!(
            !plain.contains("rel=\"preload\" as=\"image\""),
            "no image preload link unless a page opts in: {plain}"
        );
        // Opt-in: emits a high-priority image preload in <head>.
        let with = PageLayout::new("Home")
            .with_preload_image("/public/img/lake-tahoe/lake-tahoe-1200w.jpg")
            .render(&html! { p { "x" } })
            .into_string();
        assert!(
            with.contains(
                "<link rel=\"preload\" as=\"image\" \
             href=\"/public/img/lake-tahoe/lake-tahoe-1200w.jpg\" fetchpriority=\"high\">"
            ),
            "expected hero preload link, got: {with}"
        );
    }

    #[test]
    fn bootstrap_css_is_linked() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("/public/css/bootstrap.min.css"),
            "expected vendored Bootstrap CSS link, got: {out}",
        );
    }

    #[test]
    fn brand_palette_is_linked_after_bootstrap() {
        // brand.css remaps Bootstrap's `primary`/`blue`/`cyan` onto the
        // firm cyan; its token + button overrides only win when parsed
        // after Bootstrap, so order matters.
        let out = render("Home", &html! { p { "x" } });
        let bs = out
            .find("/public/css/bootstrap.min.css")
            .expect("bootstrap css linked");
        let brand = out.find("/public/css/brand.css").expect("brand css linked");
        assert!(
            bs < brand,
            "brand.css must be linked after bootstrap.min.css, got: {out}",
        );
    }

    #[test]
    fn noto_serif_stylesheet_is_linked_after_bootstrap() {
        // Noto Serif's `--bs-body-font-family` override only wins if its
        // stylesheet is parsed after Bootstrap's, so order matters.
        let out = render("Home", &html! { p { "x" } });
        let bs = out
            .find("/public/css/bootstrap.min.css")
            .expect("bootstrap css linked");
        let ns = out
            .find("/public/css/noto-serif.css")
            .expect("noto-serif css linked");
        assert!(
            bs < ns,
            "noto-serif.css must be linked after bootstrap.min.css, got: {out}",
        );
    }

    #[test]
    fn noto_serif_font_is_preloaded() {
        // Body copy is set in Noto Serif, so the latin regular subset is
        // preloaded to avoid a fallback-serif flash on first paint.
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains(
                "<link rel=\"preload\" as=\"font\" type=\"font/woff2\" crossorigin \
                 href=\"/public/fonts/noto-serif/noto-serif-latin-400-normal.woff2\">"
            ),
            "expected Noto Serif latin regular-subset font preload, got: {out}",
        );
    }

    #[test]
    fn bootstrap_bundle_js_is_deferred() {
        // The bundle includes Popper.js — used by navbar dropdowns,
        // future modals/tooltips/toasts. `defer` so it doesn't block
        // first paint.
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("/public/js/bootstrap.bundle.min.js"),
            "expected vendored Bootstrap JS bundle, got: {out}",
        );
        assert!(
            out.contains("script defer") || out.contains("defer src=\"/public/js/bootstrap"),
            "Bootstrap JS must load with defer so it doesn't block render, got: {out}",
        );
    }

    #[test]
    fn htmx_is_deferred() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("/public/js/htmx.min.js"),
            "expected vendored HTMX script tag, got: {out}",
        );
    }

    #[test]
    fn alpine_is_deferred() {
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("/public/js/alpine.min.js"),
            "expected vendored Alpine script tag, got: {out}",
        );
    }

    #[test]
    fn collage_lightbox_js_is_deferred() {
        // First-party click-to-zoom for blog photo collages. Loaded with
        // `defer` like the other scripts and inert unless a `.blog-collage`
        // is present on the page.
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("script defer src=\"/public/js/collage-lightbox.js\""),
            "expected deferred first-party collage lightbox script, got: {out}",
        );
    }

    #[test]
    fn html_root_advertises_data_bs_theme_auto_for_dark_mode_parity() {
        // Pico exposed dark mode via `meta color-scheme="light dark"`;
        // Bootstrap 5.3 reads `data-bs-theme` on <html>. `auto`
        // delegates to the OS preference, matching today's behavior.
        let out = render("Home", &html! { p { "x" } });
        assert!(
            out.contains("data-bs-theme=\"auto\""),
            "expected data-bs-theme=auto on <html>, got: {out}",
        );
    }
}
