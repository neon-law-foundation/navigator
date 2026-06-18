//! Read a directory of `.md` marketing fragments into [`MarketingDoc`]s.
//!
//! Front-matter with `title`, `slug`, `description`; everything
//! after the closing `---` is rendered through pulldown-cmark at
//! load time so handlers can ship the HTML verbatim.

use std::fs;
use std::io;
use std::path::Path;

use pulldown_cmark::{html, Event, Options, Parser, Tag};
use views::assets::asset_url;

use super::{MarketingDoc, PricingCard};
use crate::content_loader::ContentLoadError;

/// Just the `pricing:` block of the frontmatter. serde_yaml ignores the
/// scalar keys (`title`, `slug`, …) the line parser already owns, so we
/// can deserialize the nested cards without re-modelling the whole
/// document.
#[derive(serde::Deserialize)]
struct PricingFrontmatter {
    #[serde(default)]
    pricing: Vec<PricingCard>,
}

/// Load every `*.md` file in `dir`. Returns an empty list when `dir`
/// doesn't exist so the binary's "no marketing copy yet" path is a
/// no-op. Docs come back sorted by slug so the route table is
/// deterministic in tests.
pub fn load_dir(dir: &Path) -> Result<Vec<MarketingDoc>, ContentLoadError> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(ContentLoadError::Io {
                path: dir.display().to_string(),
                source: err,
            });
        }
    };
    let mut docs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| ContentLoadError::Io {
            path: dir.display().to_string(),
            source: e,
        })?;
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(|e| ContentLoadError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        let fallback_slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string();
        docs.push(
            parse(&raw, &fallback_slug).ok_or(ContentLoadError::MissingFrontmatter {
                path: path.display().to_string(),
            })?,
        );
    }
    docs.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(docs)
}

/// Parse a single doc. `fallback_slug` is used when no `slug:`
/// is set in front-matter (typical case — file stem matches).
#[must_use]
pub fn parse(raw: &str, fallback_slug: &str) -> Option<MarketingDoc> {
    let after_open = raw.strip_prefix("---\n")?;
    let end = after_open.find("\n---")?;
    let frontmatter = &after_open[..end];
    let body = after_open[end + "\n---".len()..].trim_start_matches('\n');
    let fields = parse_frontmatter(frontmatter);

    let title = fields
        .get("title")
        .cloned()
        .unwrap_or_else(|| "Untitled".into());
    let slug = fields
        .get("slug")
        .cloned()
        .unwrap_or_else(|| fallback_slug.to_string());
    let description = fields.get("description").cloned().unwrap_or_default();
    let body_html = render_markdown(body);
    let metadata = fields
        .into_iter()
        .filter(|(k, _)| !matches!(k.as_str(), "title" | "slug" | "description" | "pricing"))
        .collect();
    // The nested `pricing:` block needs a real YAML parse; the line
    // parser above only handles top-level scalars. A malformed block
    // yields no cards rather than failing the whole page load.
    let pricing = serde_yaml::from_str::<PricingFrontmatter>(frontmatter)
        .map(|f| f.pricing)
        .unwrap_or_default();

    Some(MarketingDoc {
        slug,
        title,
        description,
        body_html,
        metadata,
        pricing,
    })
}

fn parse_frontmatter(source: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for line in source.lines() {
        // Indented lines belong to a nested structure (the `pricing:`
        // block, a folded `description: >` scalar) — serde_yaml owns
        // those; the line parser only takes top-level scalar keys.
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let trimmed = line.trim();
        let Some(colon) = trimmed.find(':') else {
            continue;
        };
        let key = trimmed[..colon].trim().to_string();
        let value = unwrap_quotes(trimmed[colon + 1..].trim());
        out.insert(key, value);
    }
    out
}

fn unwrap_quotes(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= 2 {
        let first = chars[0];
        let last = chars[chars.len() - 1];
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return chars[1..chars.len() - 1].iter().collect();
        }
    }
    s.to_string()
}

fn render_markdown(src: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_FOOTNOTES);
    // Route every image `src` through the asset seam so content authors
    // write a repo-relative path (`img/thanks-apple/foo.jpg`) that
    // resolves to the `/public` mount in dev and the photo CDN bucket
    // (`NAVIGATOR_ASSET_BASE_URL`) in production. Image bytes live in
    // GCS, never in the repo (`/web/public/img/` is gitignored).
    let parser = Parser::new_ext(src, opts).map(|event| match event {
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: rewrite_image_src(&dest_url).into(),
            title,
            id,
        }),
        other => other,
    });
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// Resolve a markdown image's `src` against the asset seam. A
/// repo-relative path (`img/thanks-apple/foo.jpg`) routes through
/// [`asset_url`]; an already-absolute source (`http(s)://`, `data:`, or
/// a root-relative `/path`) passes through untouched.
fn rewrite_image_src(dest: &str) -> String {
    if dest.starts_with("http://")
        || dest.starts_with("https://")
        || dest.starts_with("data:")
        || dest.starts_with('/')
    {
        return dest.to_string();
    }
    asset_url(dest)
}

#[cfg(test)]
mod tests {
    use super::{load_dir, parse, rewrite_image_src};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn relative_image_src_routes_through_the_asset_seam() {
        // A repo-relative markdown image resolves against the asset base
        // (default `/public` in tests, the photo CDN bucket in prod), so
        // image bytes can live in GCS instead of the repo.
        assert_eq!(
            rewrite_image_src("img/thanks-apple/team-lunch.jpg"),
            "/public/img/thanks-apple/team-lunch.jpg"
        );
        // Already-absolute sources are left untouched.
        assert_eq!(
            rewrite_image_src("https://example.com/x.jpg"),
            "https://example.com/x.jpg"
        );
        assert_eq!(rewrite_image_src("/public/img/x.jpg"), "/public/img/x.jpg");
        assert_eq!(
            rewrite_image_src("data:image/png;base64,AA"),
            "data:image/png;base64,AA"
        );
    }

    #[test]
    fn markdown_image_renders_with_resolved_asset_src() {
        let raw = "---\n\
                   title: \"Post\"\n\
                   description: \"d\"\n\
                   ---\n\n\
                   ![a teammate](img/thanks-apple/team-lunch.jpg)";
        let doc = parse(raw, "post").expect("parses");
        assert!(
            doc.body_html
                .contains("src=\"/public/img/thanks-apple/team-lunch.jpg\""),
            "image src must route through the asset seam, got: {}",
            doc.body_html
        );
    }

    #[test]
    fn parse_extracts_fields_and_renders_body() {
        let raw = "---\n\
                   title: \"Flat-fee legal services\"\n\
                   slug: home\n\
                   description: \"Estate and corporate, no litigation.\"\n\
                   ---\n\n\
                   ## Lead\n\nFlat-fee.";
        let doc = parse(raw, "fallback").expect("parses");
        assert_eq!(doc.title, "Flat-fee legal services");
        assert_eq!(doc.slug, "home");
        assert_eq!(doc.description, "Estate and corporate, no litigation.");
        assert!(doc.body_html.contains("<h2>Lead</h2>"));
        assert!(doc.body_html.contains("<p>Flat-fee.</p>"));
    }

    #[test]
    fn parse_keeps_top_level_pricing_cols_in_metadata() {
        // A page forces the pricing layout with a top-level `pricing_cols:`
        // key (`1` stacks the cards, e.g. Nimbus). The line parser lands it
        // in `metadata` while serde_yaml still reads the nested cards; the
        // override is not one of the four well-known fields, so it survives.
        let raw = "---\n\
                   title: \"Nimbus\"\n\
                   slug: nimbus\n\
                   description: \"d\"\n\
                   pricing_cols: 1\n\
                   pricing:\n\
                   \x20\x20- title: \"Nimbus\"\n\
                   \x20\x20\x20\x20price: \"$11,111\"\n\
                   \x20\x20\x20\x20cta_label: \"Email\"\n\
                   \x20\x20\x20\x20cta_href: \"mailto:x@y.org\"\n\
                   ---\n\nBody.";
        let doc = parse(raw, "nimbus").expect("parses");
        assert_eq!(
            doc.metadata.get("pricing_cols").map(String::as_str),
            Some("1")
        );
        assert_eq!(doc.pricing.len(), 1, "nested pricing card still parses");
    }

    #[test]
    fn parse_passes_inline_html_icons_through_verbatim() {
        // The services index denotes each product with a Bootstrap
        // Icon (`<i class="bi …">`) authored inline in the markdown.
        // pulldown-cmark must emit that raw inline HTML unescaped, or
        // the icons render as literal angle-bracket text.
        let raw = "---\n\
                   title: \"Services\"\n\
                   slug: services\n\
                   description: \"d\"\n\
                   ---\n\n\
                   - <i class=\"bi bi-star-fill\" aria-hidden=\"true\"></i> **Northstar**";
        let doc = parse(raw, "services").expect("parses");
        assert!(
            doc.body_html
                .contains("<i class=\"bi bi-star-fill\" aria-hidden=\"true\"></i>"),
            "icon markup must survive rendering, got: {}",
            doc.body_html
        );
        // And not be HTML-escaped into visible text.
        assert!(!doc.body_html.contains("&lt;i class"));
    }

    #[test]
    fn parse_uses_fallback_slug_when_omitted() {
        let raw = "---\ntitle: T\n---\nbody";
        let doc = parse(raw, "from-filename").expect("parses");
        assert_eq!(doc.slug, "from-filename");
    }

    #[test]
    fn parse_returns_none_without_frontmatter() {
        assert!(parse("just body, no frontmatter", "x").is_none());
    }

    #[test]
    fn parse_preserves_unknown_frontmatter_keys_into_metadata() {
        let raw = "---\n\
                   title: \"Partner\"\n\
                   slug: partner\n\
                   description: \"d\"\n\
                   topic: immigration\n\
                   org_name: Partner X\n\
                   phone: 1-800-555-0199\n\
                   ---\nbody";
        let doc = parse(raw, "x").expect("parses");
        assert_eq!(
            doc.metadata.get("topic").map(String::as_str),
            Some("immigration"),
        );
        assert_eq!(
            doc.metadata.get("org_name").map(String::as_str),
            Some("Partner X"),
        );
        assert_eq!(
            doc.metadata.get("phone").map(String::as_str),
            Some("1-800-555-0199"),
        );
    }

    #[test]
    fn parse_reads_nested_pricing_block_into_typed_cards() {
        let raw = "---\n\
                   title: \"Fractional GC\"\n\
                   slug: fractional-gc\n\
                   pricing:\n\
                   \x20 - title: Seed\n\
                   \x20   price: \"$3,500\"\n\
                   \x20   cadence: /mo\n\
                   \x20   blurb: A lawyer on call.\n\
                   \x20   features: [\"10 contract reviews each month\"]\n\
                   \x20   cta_label: Get your tier recommendation\n\
                   \x20   cta_href: \"mailto:support@neonlaw.com\"\n\
                   \x20 - title: Growth\n\
                   \x20   price: \"$7,500\"\n\
                   \x20   cadence: /mo\n\
                   \x20   blurb: For teams signing deals every week.\n\
                   \x20   features: [\"20 contract reviews each month\"]\n\
                   \x20   cta_label: Get your tier recommendation\n\
                   \x20   cta_href: \"mailto:support@neonlaw.com\"\n\
                   \x20   featured: true\n\
                   \x20   featured_label: Recommended\n\
                   ---\nbody";
        let doc = parse(raw, "x").expect("parses");
        assert_eq!(doc.pricing.len(), 2);
        assert_eq!(doc.pricing[0].title, "Seed");
        assert_eq!(doc.pricing[0].price, "$3,500");
        assert_eq!(doc.pricing[0].cadence.as_deref(), Some("/mo"));
        assert!(!doc.pricing[0].featured);
        assert_eq!(doc.pricing[1].title, "Growth");
        assert!(doc.pricing[1].featured);
        assert_eq!(
            doc.pricing[1].featured_label.as_deref(),
            Some("Recommended")
        );
        assert_eq!(
            doc.pricing[1].features,
            vec!["20 contract reviews each month"]
        );
        // The nested keys must not leak into the flat metadata map.
        assert!(doc.metadata.is_empty(), "got: {:?}", doc.metadata);
    }

    #[test]
    fn parse_without_pricing_block_yields_no_cards() {
        let raw = "---\ntitle: T\nslug: s\n---\nbody";
        let doc = parse(raw, "x").expect("parses");
        assert!(doc.pricing.is_empty());
    }

    #[test]
    fn pricing_marker_survives_markdown_as_the_splice_token() {
        // The `[[pricing]]` paragraph is how content authors place the
        // cards. pulldown-cmark must render it verbatim so the view's
        // `<p>[[pricing]]</p>` split point matches — this pins that
        // contract end to end.
        let raw = "---\ntitle: T\nslug: s\n---\nlead\n\n[[pricing]]\n\nrest";
        let doc = parse(raw, "x").expect("parses");
        assert!(
            doc.body_html.contains("<p>[[pricing]]</p>"),
            "got: {}",
            doc.body_html
        );
    }

    #[test]
    fn parse_does_not_duplicate_well_known_keys_into_metadata() {
        // title/slug/description are first-class fields on MarketingDoc;
        // they must NOT leak back into the metadata map or callers will
        // have two sources of truth for the same value.
        let raw = "---\ntitle: T\nslug: s\ndescription: D\n---\nbody";
        let doc = parse(raw, "x").expect("parses");
        assert!(doc.metadata.is_empty(), "got: {:?}", doc.metadata);
    }

    #[test]
    fn load_dir_returns_empty_when_directory_missing() {
        let docs = load_dir(std::path::Path::new("/no/such/dir/abcdef")).unwrap();
        assert!(docs.is_empty());
    }

    #[test]
    fn bundled_foundation_md_carries_justice_gap_and_training_pillars() {
        // Pins the foundation marketing page's full narrative: the
        // justice-gap stats and the three "Training attorneys for
        // tomorrow" pillars must both make it into the rendered body
        // at `/foundation`.
        let dir = std::path::Path::new(crate::DEFAULT_MARKETING_DIR);
        let docs = load_dir(dir).expect("bundled marketing dir loads");
        let foundation = docs
            .iter()
            .find(|d| d.slug == "foundation")
            .expect("foundation doc present in bundled content");
        // Justice-gap stats.
        assert!(
            foundation.body_html.contains("92%"),
            "missing 92% justice-gap stat: {}",
            foundation.body_html
        );
        assert!(
            foundation.body_html.contains("5.1 billion"),
            "missing 5.1 billion access-to-justice stat"
        );
        // Training pillars.
        assert!(foundation.body_html.contains("AI-Assisted Legal Research"));
        assert!(foundation.body_html.contains("Workflow Automation"));
        assert!(foundation
            .body_html
            .contains("Expanding Legal Aid Capacity"));
        // Support / contact CTA.
        assert!(foundation
            .body_html
            .contains("github.com/neon-law-foundation"));
        assert!(foundation.body_html.contains("mailto:support@neonlaw.org"));
    }

    #[test]
    fn bundled_nautilus_md_advertises_flat_66_and_takes_no_cut() {
        // Pins the debt-collection product page: the $66/month flat fee
        // and the load-bearing trust line ("we never take a percentage
        // of your debt") must survive in both the parsed pricing card
        // and the rendered body, and the page must keep referring
        // litigation out rather than implying we go to court.
        let dir = std::path::Path::new(crate::DEFAULT_MARKETING_DIR);
        let docs = load_dir(dir).expect("bundled marketing dir loads");
        let nautilus = docs
            .iter()
            .find(|d| d.slug == "nautilus")
            .expect("nautilus doc present in bundled content");
        let card = nautilus
            .pricing
            .first()
            .expect("nautilus advertises a price");
        // Standardized price shape: a bare amount plus a small cadence,
        // the same as the other products (Nexus `/month`, Nest `/year`).
        assert_eq!(card.price, "$66");
        assert_eq!(card.cadence.as_deref(), Some("/month"));
        assert!(
            card.features
                .iter()
                .any(|f| f.contains("never take a percentage of your debt")),
            "missing no-cut pledge in pricing card: {:?}",
            card.features
        );
        assert!(
            nautilus
                .body_html
                .contains("we never take a percentage of your debt"),
            "missing no-cut pledge in body: {}",
            nautilus.body_html
        );
        assert!(
            nautilus.body_html.contains("/services/litigation"),
            "nautilus must refer litigation out, not imply a courtroom"
        );
    }

    #[test]
    fn bundled_nook_md_advertises_flat_9999_and_offers_either_or_both_sides() {
        // Pins the brokerless real-estate closing page: the $9,999 flat
        // fee with no recurring cadence, the representation choice (buyer,
        // seller, or both), and the load-bearing RPC 1.7 promise — joint
        // representation only with written consent, and an exit to
        // independent counsel if the parties' interests turn adverse.
        let dir = std::path::Path::new(crate::DEFAULT_MARKETING_DIR);
        let docs = load_dir(dir).expect("bundled marketing dir loads");
        let nook = docs
            .iter()
            .find(|d| d.slug == "nook")
            .expect("nook doc present in bundled content");
        let card = nook.pricing.first().expect("nook advertises a price");
        // A one-time fee: a bare amount and no recurring cadence suffix.
        assert_eq!(card.price, "$9,999");
        assert_eq!(card.cadence.as_deref(), None);
        // The representation choice survives in the pricing card.
        assert!(
            card.features
                .iter()
                .any(|f| f.contains("buyer") && f.contains("seller") && f.contains("both")),
            "missing buyer/seller/both representation option: {:?}",
            card.features
        );
        // The body carries the dual-representation safeguards.
        assert!(
            nook.body_html.contains("informed consent in writing"),
            "missing written-consent safeguard in body: {}",
            nook.body_html
        );
        assert!(
            nook.body_html.contains("independent counsel"),
            "missing dual-counsel exit in body"
        );
        // One flat fee, no percentage of the sale.
        assert!(
            card.features
                .iter()
                .any(|f| f.contains("no percentage of the sale price")),
            "missing no-percentage pledge: {:?}",
            card.features
        );
    }

    #[test]
    fn load_dir_reads_and_sorts_by_slug() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("home.md"),
            "---\ntitle: \"Home\"\nslug: home\n---\nh",
        )
        .unwrap();
        fs::write(
            dir.path().join("foundation.md"),
            "---\ntitle: \"Foundation\"\nslug: foundation\n---\nf",
        )
        .unwrap();
        fs::write(dir.path().join("ignored.txt"), "not markdown").unwrap();
        let docs = load_dir(dir.path()).unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].slug, "foundation");
        assert_eq!(docs[1].slug, "home");
    }

    #[test]
    fn bundled_mission_letter_loads_in_both_locales_as_a_marketing_doc() {
        // The mission letter lives with the other marketing fragments
        // (`web/content/marketing/mission.md` + its `es/` twin), loaded
        // from disk like any other doc — no special-case bake. Both
        // locales must surface the `mission` slug and the letter's
        // opening line so `/foundation/mission` renders the real prose.
        let en = std::path::Path::new(crate::DEFAULT_MARKETING_DIR);
        let mission = load_dir(en)
            .expect("English marketing dir loads")
            .into_iter()
            .find(|d| d.slug == "mission")
            .expect("English mission doc present in bundled content");
        assert!(
            mission.body_html.contains("Hey friend"),
            "English mission must render its opening line: {}",
            mission.body_html
        );

        let es_mission = load_dir(&en.join("es"))
            .expect("Spanish marketing dir loads")
            .into_iter()
            .find(|d| d.slug == "mission")
            .expect("Spanish mission doc present in bundled content");
        assert!(
            es_mission.body_html.contains("Hola, amigo"),
            "Spanish mission must render its transcreated opening line: {}",
            es_mission.body_html
        );
    }
}
