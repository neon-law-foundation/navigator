//! Cucumber runner for `features/spanish_client_journey.feature`.
//!
//! The Spanish-language journey: a Spanish-speaking client walks the same
//! entity-formation funnel an English speaker does — `/es`, `/es/services`,
//! `/es/services/corporate` (Neon Law Nest), `/es/foundation/mission` —
//! entirely under the `/es` locale. It proves the transcreated copy
//! carries the same flow (`project_i18n_spanish_phase1`). The
//! questionnaire-in-Spanish tail is already covered by `intake_language`;
//! this journey covers the public funnel that leads into it.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use cucumber::{given, then, when, World};
use features::journey::{Captured, Journey};

#[derive(Default, World)]
#[world(init = Self::default)]
struct SpanishWorld {
    journey: Option<Journey>,
    last: Option<Captured>,
}

impl std::fmt::Debug for SpanishWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpanishWorld").finish_non_exhaustive()
    }
}

impl SpanishWorld {
    fn last(&self) -> &Captured {
        self.last.as_ref().expect("no page visited")
    }
}

#[given("a fresh Navigator app with the canonical templates seeded")]
async fn build_app(world: &mut SpanishWorld) {
    world.journey = Some(Journey::open("spanish").await);
}

#[when(regex = r#"^a Spanish-speaking client opens "([^"]+)"$"#)]
async fn visit(world: &mut SpanishWorld, path: String) {
    let resp = world.journey.as_ref().expect("journey").visit(&path).await;
    world.last = Some(resp);
}

#[then("the page is served in Spanish")]
async fn assert_spanish(world: &mut SpanishWorld) {
    let page = world.last();
    assert!(page.status.is_success(), "status was {}", page.status);
    // `<html lang="es">` is the locale's load-bearing signal for screen
    // readers and SEO; the layout sets it only for the Es locale.
    assert!(
        page.body.contains("lang=\"es\""),
        "page is not rendered in the Spanish locale",
    );
    // The chrome (baked-in i18n catalog) is transcreated, independent of
    // marketing content and of which brand (firm vs Foundation) owns the
    // page: the auth link reads in Spanish.
    assert!(
        page.body.contains("Iniciar sesi"),
        "the sign-in chrome should read in Spanish",
    );
}

#[then(regex = r#"^the navigation stays within the "([^"]+)" funnel$"#)]
async fn assert_funnel(world: &mut SpanishWorld, prefix: String) {
    // Internal nav hrefs keep the locale prefix, so a reader never falls
    // back out of Spanish by clicking through.
    assert!(
        world.last().body.contains(&format!("href=\"{prefix}/")),
        "navigation should keep the {prefix} locale prefix",
    );
}

#[tokio::main]
async fn main() {
    SpanishWorld::cucumber()
        .run("tests/features/spanish_client_journey.feature")
        .await;
}
