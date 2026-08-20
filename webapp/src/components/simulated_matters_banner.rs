//! The site-wide banner a deployment holding simulated matters publishes.
//!
//! One deployment carries invented clients: the persistent staging row at
//! `staging.neonlaw.com`, whose whole purpose is to be a link somebody can be
//! shown. Everything in it — every case, client, document, and roster — is
//! made up, and a visitor looking at a matter detail page has no way to know
//! that from the page itself. It looks exactly like the real thing, because it
//! *is* the real application; only the data plane is synthetic.
//!
//! So the deployment says so, on every page. This is not a debug affordance or
//! a developer convenience: it is the one thing standing between a
//! demonstration and somebody believing they were shown a client's file.
//!
//! ## Why it renders through the response rather than through each page
//!
//! It is injected once into every HTML response by
//! `portal::dioxus_app::dioxus_document_head`, which already rewrites the
//! rendered document to stamp `<html lang>` and the font faces. That is the
//! only seam that reaches *every* page — including the error pages, which are
//! exactly where a reader is most likely to be confused about what they are
//! looking at, and which no shell wraps.
//!
//! The alternative was a component every page's shell renders, which would
//! have meant threading a flag through thirty server functions and leaving the
//! banner absent from whichever page was added next without it. A banner with
//! holes in it is worse than no banner, because the pages that have it teach a
//! reader to trust its absence.
//!
//! It still lives here, as a real component, so it is written in tokens and
//! covered by the same tests and the same colour guard as every other
//! component. `portal` renders it to a string once at startup.

use dioxus::prelude::*;

/// The banner's DOM id, and the hook the browser walkthrough asserts on.
pub const SIMULATED_MATTERS_BANNER_ID: &str = "simulated-matters-banner";

/// The site-wide notice that this deployment's matters are invented.
///
/// Deliberately not a landmark and not a live region. `role="status"` would
/// make an assistive technology announce it on every navigation, which is
/// nagging rather than informing; a `<aside>` would add a second complementary
/// landmark to every page and compete with the real ones. It is the first
/// content in the document instead, so it is the first thing read, once.
///
/// The copy names no hostname. A deployment that carries simulated matters is
/// identified by its configuration, not by its address, and hardcoding
/// `staging.neonlaw.com` here would make the sentence false the moment a
/// second such deployment existed.
#[component]
pub fn SimulatedMattersBanner() -> Element {
    rsx! {
        div { id: SIMULATED_MATTERS_BANNER_ID, class: "simulated-matters-banner",
            strong { class: "simulated-matters-banner__label", "Simulated matters" }
            span { class: "simulated-matters-banner__body",
                "Every client, case, and document on this deployment is invented for
                 demonstration. Nothing here is a real client's file."
            }
        }
    }
}

/// Render the banner to a standalone HTML string.
///
/// `portal` injects this into every HTML response rather than composing the
/// component into a page tree, so it needs the markup as bytes. Server-only:
/// `dioxus-ssr` is not in the wasm client bundle, and nothing in the browser
/// renders this — the string arrives already in the document.
///
/// Cheap to call, but call it once: the markup carries no props and no
/// brand-dependent copy, so it is the same on every page of both faces.
#[cfg(feature = "server")]
#[must_use]
pub fn render_simulated_matters_banner() -> String {
    fn banner() -> Element {
        rsx! {
            SimulatedMattersBanner {}
        }
    }
    let mut dom = VirtualDom::new(banner);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html() -> String {
        fn app() -> Element {
            rsx! {
                SimulatedMattersBanner {}
            }
        }
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    /// The banner says the two things a reader needs: that the matters are
    /// simulated, and that none of them belongs to a real client. The second
    /// sentence is the one that matters — "simulated" alone is jargon a
    /// visitor can read past.
    #[test]
    fn the_banner_says_the_matters_are_invented_and_not_a_real_clients() {
        let out = html();
        assert!(out.contains("Simulated matters"), "{out}");
        assert!(
            out.contains("invented for") && out.contains("real client&#39;s file"),
            "the banner must say outright that nothing here is a real client's file: {out}"
        );
    }

    /// The id is the hook the browser walkthrough looks for, so it is asserted
    /// here rather than left to a selector in a test that would fail with a
    /// less obvious message.
    #[test]
    fn the_banner_carries_the_id_the_walkthrough_asserts_on() {
        assert!(
            html().contains(&format!("id=\"{SIMULATED_MATTERS_BANNER_ID}\"")),
            "{}",
            html()
        );
    }

    /// No landmark and no live region. Both would be worse than plain content:
    /// a live region nags on every navigation, and a landmark competes with
    /// the page's own.
    #[test]
    fn the_banner_is_plain_content_rather_than_a_landmark_or_live_region() {
        let out = html();
        for attribute in ["role=", "aria-live", "<aside"] {
            assert!(
                !out.contains(attribute),
                "the banner declares no `{attribute}`: {out}"
            );
        }
    }

    /// A deployment is identified by its configuration, not its address. A
    /// hostname in this copy would be false the moment a second simulated
    /// deployment existed.
    #[test]
    fn the_banner_names_no_hostname() {
        let out = html();
        assert!(!out.contains("neonlaw.com"), "{out}");
    }
}
