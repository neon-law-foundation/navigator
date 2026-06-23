#![allow(clippy::doc_markdown)]
//! Route tests for the public, DB-backed `/services` catalog (the single
//! page that replaced the old Services dropdown).
//!
//! Drives the router via `tower::ServiceExt::oneshot` (no socket). The
//! load-bearing claims:
//!
//! - the catalog renders for an **unauthenticated** visitor on the public
//!   site, but in private mode the whole `/services` tree is auth-gated —
//!   403 for anonymous, rendered as normal for a signed-in user;
//! - every active product appears, priced from its `list_price_cents`
//!   row — Nautilus at **$66/month**, Nexus at **$2,222/month** — so the
//!   price a prospect sees is the row Xero invoices;
//! - the rendered price equals the DB row formatted, so the page can
//!   never drift from the catalog;
//! - the Spanish twin renders at `/es/services`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use store::test_support::pg;
use store::Db;
use tower::ServiceExt;
use web::AppState;

async fn seeded_db() -> Db {
    let db = pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-products-route-test"))
            .await
            .unwrap(),
    );
    store::seed::seed_canonical(&db, &storage)
        .await
        .expect("seed canonical catalog");
    db
}

async fn get(state: AppState, uri: &str) -> axum::http::Response<Body> {
    web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR))
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn catalog_lists_every_active_product_at_the_db_price() {
    let db = seeded_db().await;
    // The DB is the single source of truth — derive the expected prices.
    let nautilus = store::products::by_code(&db, "nautilus")
        .await
        .unwrap()
        .expect("nautilus seeded");
    let nexus = store::products::by_code(&db, "nexus")
        .await
        .unwrap()
        .expect("nexus seeded");
    let nook = store::products::by_code(&db, "nook")
        .await
        .unwrap()
        .expect("nook seeded");
    let node = store::products::by_code(&db, "node")
        .await
        .unwrap()
        .expect("node seeded");
    assert_eq!(nautilus.list_price_cents, 6_600, "Nautilus is $66");
    assert_eq!(nexus.list_price_cents, 222_200, "Nexus is $2,222");
    assert_eq!(nook.list_price_cents, 999_900, "Nook is $9,999");
    assert_eq!(nook.cadence, "once");
    assert_eq!(nook.billing_kind, "matter_close_flat");
    assert_eq!(node.list_price_cents, 4_400, "Node is $44");
    assert_eq!(node.cadence, "each");

    let state = web::test_support::app_state(db).await;
    let resp = get(state, "/services").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;

    // The page is titled "Services" (the renamed catalog).
    assert!(body.contains("<title>Neon Law | Services</title>"));

    // Every product name appears.
    for name in [
        "Neon Law Northstar",
        "Neon Law Nest",
        "Neon Law Nexus",
        "Neon Law Nautilus",
        "Neon Law Nook",
        "Neon Law Node",
        "Neon Law Newleaf",
        "Neon Law Namesake",
        "Neon Law Nucleus",
        "Neon Law Nerd",
        "1337 Lawyers",
        "Pro Bono",
    ] {
        assert!(body.contains(name), "missing product {name}");
    }

    // Nook is a one-time fee: $9,999, priced from the DB row, linking to
    // its own service page.
    assert!(
        body.contains(&store::products::format_price(nook.list_price_cents)),
        "Nook price must equal the DB row"
    );
    assert!(body.contains("$9,999"), "Nook reads $9,999");
    assert!(body.contains("href=\"/services/nook\""));

    // The rendered price equals the DB row formatted — never a hard-coded
    // number — so the page can't drift from what Xero bills.
    assert!(
        body.contains(&store::products::format_price(nautilus.list_price_cents)),
        "Nautilus price must equal the DB row"
    );
    assert!(body.contains("$66"), "Nautilus reads $66");
    assert!(
        body.contains(&store::products::format_price(nexus.list_price_cents)),
        "Nexus price must equal the DB row"
    );
    assert!(body.contains("$2,222"), "Nexus reads $2,222");
    // Node is the per-instance attestation: $44 each, from the DB row.
    assert!(body.contains("$44"), "Node reads $44");
    assert!(body.contains(" each"), "per-instance products show ' each'");
    // Subscriptions carry the monthly cadence suffix.
    assert!(body.contains("/month"), "recurring products show /month");
    // Each card links to the product's service page.
    assert!(body.contains("href=\"/services/nautilus\""));
    assert!(body.contains("href=\"/services/nexus\""));
    assert!(body.contains("href=\"/services/node\""));
    assert!(body.contains("href=\"/services/newleaf\""));
    assert!(body.contains("href=\"/services/namesake\""));
    assert!(body.contains("href=\"/services/nucleus\""));
    // Pro bono closes the catalog as a free card linking to its own page.
    assert!(body.contains("href=\"/services/pro-bono\""));
    assert!(body.contains("Free"), "pro bono card shows a Free price");
    // Every catalog card carries its product icon. Most are Bootstrap
    // glyphs; litigation wears the inline scales-of-justice SVG (no such
    // font glyph exists), resolved by the shared `product_icon` helper.
    assert!(
        body.contains("bi bi-star-fill") && body.contains("bi bi-heart-fill"),
        "catalog cards must render product glyphs"
    );
    assert!(
        body.contains("bi bi-eyeglasses"),
        "Neon Law Nerd card wears the eyeglasses glyph"
    );
    assert!(
        body.contains("class=\"libra-scales me-2\""),
        "the litigation card wears the inline scales-of-justice SVG"
    );

    assert!(
        body.contains("Flat fee per phase")
            && body.contains("quoted after case assessment")
            && !body.contains("$1,337/hour"),
        "litigation card should advertise quoted phase pricing instead of hourly billing"
    );

    // The catalog is a curated lineup, not alphabetical or by-price: the
    // repdigit ladder by leading digit — Nest → Nexus → Northstar → Node →
    // Newleaf → Nautilus → Namesake → Nucleus → Nook, with 1337 last.
    let pos = |needle: &str| {
        body.find(needle)
            .unwrap_or_else(|| panic!("missing {needle}"))
    };
    let order = [
        pos("Neon Law Nest"),
        pos("Neon Law Nexus"),
        pos("Neon Law Northstar"),
        pos("Neon Law Node"),
        pos("Neon Law Newleaf"),
        pos("Neon Law Nautilus"),
        pos("Neon Law Namesake"),
        pos("Neon Law Nucleus"),
        pos("Neon Law Nook"),
        pos("Neon Law Nerd"),
        pos("1337 Lawyers"),
        pos("Pro Bono"),
    ];
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "catalog cards out of curated order: {order:?}"
    );

    // One-time products carry "once" on the price line (Nook, Northstar,
    // Newleaf, Namesake, Nucleus); the word appears only from that cadence
    // suffix.
    assert!(
        body.contains("once"),
        "one-time products show 'once' on the price line"
    );
    // Each card shows a one-sentence service description, not just a
    // cadence note.
    assert!(
        body.contains("answered under your rights"),
        "Nautilus card shows its service description"
    );
    assert!(
        body.contains("no broker on either side"),
        "Nook card shows its service description"
    );
}

#[tokio::test]
async fn new_service_detail_pages_render_with_their_marketing_copy() {
    // Each new product has its own `/services/<slug>` detail page, rendered
    // from `web/content/marketing/<slug>.md`. Drive each English page and
    // confirm it returns 200 with its headline + DB-priced figure.
    for (path, needle, price) in [
        ("/services/node", "recorded on-chain", "$44"),
        ("/services/newleaf", "uncontested divorce", "$555"),
        ("/services/namesake", "filed with the USPTO", "$777"),
        ("/services/nucleus", "Nevada fund", "$8,888"),
        (
            "/services/litigation",
            "Flat fee per phase",
            "pro-rata refund",
        ),
        ("/services/nerd", "put a nerd on the stand", "$1,337"),
        ("/services/pro-bono", "Statement of Legal Aid", "Free"),
    ] {
        let mut state = web::test_support::app_state(seeded_db().await).await;
        // Detail pages render from the bundled marketing markdown; the
        // default test state ships an empty index, so load the real dir.
        let dir = std::path::Path::new(web::DEFAULT_MARKETING_DIR);
        state.marketing = web::MarketingIndex::new(web::marketing::loader::load_dir(dir).unwrap());
        let resp = get(state, path).await;
        assert_eq!(resp.status(), StatusCode::OK, "{path} should render");
        let body = body_string(resp).await;
        assert!(body.contains(needle), "{path} missing copy {needle:?}");
        assert!(body.contains(price), "{path} missing price {price}");
    }
}

#[tokio::test]
async fn services_catalog_renders_real_db_prices() {
    // The `/services` catalog is public — an anonymous visitor sees it
    // with the real DB-backed prices.
    let state = web::test_support::app_state(seeded_db().await).await;
    let resp = get(state, "/services").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("$44"), "catalog renders real DB prices");
}

#[tokio::test]
async fn spanish_catalog_renders_at_es_services() {
    let state = web::test_support::app_state(seeded_db().await).await;
    let resp = get(state, "/es/services").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // Spanish chrome, real DB prices.
    assert!(body.contains("Servicios"), "Spanish heading");
    assert!(body.contains("$44"));
    // The Spanish service links are `/es`-prefixed.
    assert!(body.contains("href=\"/es/services/nautilus\""));
}
