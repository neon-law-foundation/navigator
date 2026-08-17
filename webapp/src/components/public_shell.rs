//! The public page shell, as a Dioxus component (issue #641, Phase 2).
//!
//! The composition primitive every migrated public page wraps its content in:
//! the themed page skeleton that stacks the [`crate::components::SiteHeader`]
//! chrome, the page's `main` content, and the [`crate::components::SiteFooterLegal`]
//! strip, and hoists the Dioxus Components theme stylesheet into the document
//! head. It is the public counterpart to the authenticated
//! [`crate::components::NavigatorShell`] (#792).
//!
//! The header and footer are passed in as rendered `Element`s so the caller
//! composes them from the process brand (a page port resolves the brand
//! server-side, exactly as the `PageLayout` did). The `<head>` concerns a
//! component tree cannot express — the per-response CSP nonce, the licensed
//! GORP faces, and the `<title>`/`<meta>` — are owned by the route middleware
//! (`portal::dioxus_app`).

use dioxus::prelude::*;

use crate::components::THEME_STYLESHEET_HREF;

/// The themed public page skeleton.
///
/// - `header`: the site header chrome (typically a [`crate::components::SiteHeader`]).
/// - `children`: the page's `main` content.
/// - `footer`: the footer legal strip (typically a
///   [`crate::components::SiteFooterLegal`]).
/// - `main_landmark`: render the content region as `<main>` (the default, for a
///   real page) or as a plain `<div>` (when the caller already owns the page's
///   one `<main>` landmark).
#[component]
pub fn PublicShell(
    header: Element,
    children: Element,
    footer: Element,
    #[props(default = true)] main_landmark: bool,
) -> Element {
    rsx! {
        // The theme stylesheet hoists into <head> so the shell and its content
        // are styled before hydration; the pages keep Bootstrap until they
        // move, so the two systems stay isolated.
        document::Stylesheet { href: THEME_STYLESHEET_HREF }
        div { class: "nav-theme public-shell",
            {header}
            if main_landmark {
                main { class: "public-shell__main", {children} }
            } else {
                div { class: "public-shell__main", {children} }
            }
            {footer}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssr(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    fn html() -> String {
        fn app() -> Element {
            rsx! {
                PublicShell {
                    header: rsx! { header { "SITE-HEADER" } },
                    footer: rsx! { footer { "SITE-FOOTER" } },
                    p { "PAGE-BODY" }
                }
            }
        }
        ssr(app)
    }

    #[test]
    fn stacks_header_then_main_then_footer() {
        let out = html();
        let header = out.find("SITE-HEADER").expect("header present");
        let body = out.find("PAGE-BODY").expect("body present");
        let footer = out.find("SITE-FOOTER").expect("footer present");
        assert!(
            header < body && body < footer,
            "header → main → footer: {out}"
        );
    }

    #[test]
    fn wraps_content_in_the_themed_main_region() {
        let out = html();
        assert!(
            out.contains("nav-theme"),
            "the shell carries the theme class: {out}"
        );
        // A real page defaults to the `<main>` landmark.
        assert!(
            out.contains(r#"<main class="public-shell__main""#),
            "the page body sits in the shell's <main> region: {out}",
        );
    }

    // The theme stylesheet is included via `document::Stylesheet`, which the
    // framework hoists into the document head at render time — the same seam
    // every migrated page uses. That hoisting is not observable through a bare
    // `dioxus_ssr::render`, so it is verified at the route/browser level rather
    // than asserted on the component's rendered string here.

    #[test]
    fn preview_mode_drops_the_main_landmark() {
        // A preview passes `main_landmark: false` so it does not nest a second
        // `<main>` inside the surrounding page landmark.
        fn app() -> Element {
            rsx! {
                PublicShell {
                    header: rsx! { header { "H" } },
                    footer: rsx! { footer { "F" } },
                    main_landmark: false,
                    p { "BODY" }
                }
            }
        }
        let out = ssr(app);
        assert!(!out.contains("<main"), "preview renders no <main>: {out}");
        assert!(
            out.contains(r#"<div class="public-shell__main""#),
            "content still in the region: {out}"
        );
    }
}
