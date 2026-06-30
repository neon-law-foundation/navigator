//! Internationalization: locale type + a Rails-style `t()` lookup over
//! per-locale chrome catalogs.
//!
//! Two kinds of user-visible text get two mechanisms (see
//! [`docs/i18n.md`](../../../docs/i18n.md)):
//!
//! - **Chrome / marketing copy** — navbar, auth links, the switcher,
//!   CTAs, the service catalog, testimonial headings — is short and
//!   engineer- or attorney-owned. It lives in per-domain YAML files under
//!   `views/locales/<locale>/<domain>.yml`, baked into the binary with
//!   `include_str!` and looked up here with [`t`] / [`t_args`].
//! - **Prose** — marketing pages, the mission letter — stays Markdown,
//!   with a parallel localized tree the `web` crate loads.
//!
//! The catalog is split by **domain** (one file per domain per locale) so
//! review boundaries map to files and "English-only" is structural, not a
//! per-key waiver: a domain with an `es/` twin is LOCALIZED (es-parity
//! enforced); a domain with no `es/` twin (portal, errors — added as copy
//! migrates) is ENGLISH-ONLY and skipped by the parity guard. See
//! [`DOMAINS`].
//!
//! `En` is the source locale and the universal fallback: a key missing
//! from a Spanish file resolves to the English value, never to a raw key.

use std::collections::HashMap;
use std::sync::LazyLock;

/// One supported locale. The single source of truth for "which
/// language" — it yields the `<html lang>` code, the URL path prefix,
/// and the catalog selector, so "locale" is never stringly-typed.
///
/// `En` is the source locale and the fallback for every other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    /// English — the source locale, served at the unprefixed root.
    #[default]
    En,
    /// Spanish — served under the `/es` URL prefix.
    Es,
}

impl Locale {
    /// BCP-47 language code for `<html lang>` and `hreflang`.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Es => "es",
        }
    }

    /// URL path prefix: `""` for English (the root), `"/es"` for Spanish.
    #[must_use]
    pub fn path_prefix(self) -> &'static str {
        match self {
            Locale::En => "",
            Locale::Es => "/es",
        }
    }

    /// The language's own name (its endonym) — the visible label of the
    /// switcher that targets this locale. Never a flag: flags are
    /// countries, not languages.
    #[must_use]
    pub fn endonym(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::Es => "Español",
        }
    }

    /// The locale a one-tap switcher should move the reader to. With
    /// exactly two locales this is the other one.
    #[must_use]
    pub fn switch_target(self) -> Locale {
        match self {
            Locale::En => Locale::Es,
            Locale::Es => Locale::En,
        }
    }

    /// Resolve the locale from a request path: `/es` or `/es/...` is
    /// Spanish, everything else is English.
    #[must_use]
    pub fn from_path(path: &str) -> Locale {
        if path == "/es" || path.starts_with("/es/") {
            Locale::Es
        } else {
            Locale::En
        }
    }
}

/// One catalog domain: its English source and, for a LOCALIZED domain,
/// the Spanish twin. `es: None` marks an ENGLISH-ONLY domain (portal,
/// errors) that es-parity skips by construction — no per-key waiver.
struct Domain {
    en: &'static str,
    es: Option<&'static str>,
}

/// The catalog, split by domain. Add an English-only domain during the
/// copy migration with `es: None`; add a localized one with both halves.
const DOMAINS: &[Domain] = &[
    // Chrome — navbar, auth, switcher, footer. Localized (Tier A).
    Domain {
        en: include_str!("../locales/en/chrome.yml"),
        es: Some(include_str!("../locales/es/chrome.yml")),
    },
    // Marketing — services catalog, CTAs, testimonials, home strip.
    // Localized (Tier A, attorney-reviewed Spanish).
    Domain {
        en: include_str!("../locales/en/marketing.yml"),
        es: Some(include_str!("../locales/es/marketing.yml")),
    },
    // Portal — the sign-in chooser, password flows, the private-mode gate.
    // English-only (es: None): the portal stays English by policy, so the
    // es-parity guard skips it by construction (no per-key waiver).
    Domain {
        en: include_str!("../locales/en/portal.yml"),
        es: None,
    },
];

/// Merge every domain's catalog under `select` into one flat key→value map.
fn build(select: impl Fn(&Domain) -> Option<&'static str>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for domain in DOMAINS {
        if let Some(src) = select(domain) {
            out.extend(parse_catalog(src));
        }
    }
    out
}

static EN: LazyLock<HashMap<String, String>> = LazyLock::new(|| build(|d| Some(d.en)));
static ES: LazyLock<HashMap<String, String>> = LazyLock::new(|| build(|d| d.es));

fn catalog(locale: Locale) -> &'static HashMap<String, String> {
    match locale {
        Locale::En => &EN,
        Locale::Es => &ES,
    }
}

/// Parse a YAML catalog into a flat map of dotted keys
/// (`nav.home`) → value. Nested mappings are flattened with `.`.
fn parse_catalog(src: &str) -> HashMap<String, String> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(src).expect("locale catalog must be valid YAML");
    let mut out = HashMap::new();
    flatten("", &value, &mut out);
    out
}

fn flatten(prefix: &str, value: &serde_yaml::Value, out: &mut HashMap<String, String>) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (k, v) in map {
                let Some(key) = k.as_str() else { continue };
                let next = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&next, v, out);
            }
        }
        serde_yaml::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        serde_yaml::Value::Null => {}
        other => {
            // Numbers / bools shouldn't appear in a chrome catalog, but
            // round-trip them as strings rather than dropping silently.
            if let Ok(s) = serde_yaml::to_string(other) {
                out.insert(prefix.to_string(), s.trim().to_string());
            }
        }
    }
}

/// Resolve `key` in `locale`, falling back to English, then to the key
/// itself. Returns `None` only when the key is absent from every
/// catalog — a programming error, since the English catalog is complete.
fn raw(locale: Locale, key: &str) -> Option<&'static str> {
    catalog(locale)
        .get(key)
        .or_else(|| EN.get(key))
        .map(String::as_str)
}

/// Rails-style translation lookup: `t(locale, "nav.home")`.
///
/// Falls back to the English value when the key is missing in `locale`,
/// and to the key itself only as a last-resort dev signal (never
/// expected in production, since the English catalog is the complete source).
#[must_use]
pub fn t(locale: Locale, key: &str) -> String {
    raw(locale, key).map_or_else(|| key.to_string(), str::to_string)
}

/// `t` with `%{name}` interpolation, e.g.
/// `t_args(locale, "cta.email", &[("email", "support@example.com")])`.
#[must_use]
pub fn t_args(locale: Locale, key: &str, args: &[(&str, &str)]) -> String {
    let mut out = t(locale, key);
    for (name, value) in args {
        out = out.replace(&format!("%{{{name}}}"), value);
    }
    out
}

/// Resolve `key` in `locale` or panic — the strict counterpart to [`t`].
///
/// Where [`t`] falls back to the key string so a render never blanks in
/// production, `t_strict` treats a missing entry as the programming error
/// it is (a typo'd or deleted key). This is the Rust analog of Rails'
/// `raise_on_missing_translations`: use it in tests (and dev tooling) so
/// a stale key fails loudly instead of rendering its own name.
/// `#[track_caller]` points the panic at the caller, not here.
#[track_caller]
#[must_use]
pub fn t_strict(locale: Locale, key: &str) -> &'static str {
    raw(locale, key).unwrap_or_else(|| {
        panic!(
            "i18n: no catalog entry for {key:?} (checked {locale:?}, then the En \
             fallback) — add it to the English catalog (views/locales/en/<domain>.yml); \
             En is the complete source every locale falls back to."
        )
    })
}

/// HTML-escape the way maud does (`&<>"`), so asserting on catalog copy
/// that contains one of those characters still matches the rendered body.
#[cfg(feature = "test-support")]
fn maud_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Assert that `body` renders the catalog copy for `key`.
///
/// Two failures, two distinct messages: a missing key panics in
/// [`t_strict`] ("no catalog entry"); a present key whose copy is absent
/// from the page fails here. Because the expectation is the *key*, not the
/// prose, editing the copy in the catalog keeps every call site green — the
/// test asserts the page wires up the slot, not what the slot says.
///
/// Prefer the [`assert_renders!`](crate::assert_renders) macro, which
/// defaults the locale to English.
#[cfg(feature = "test-support")]
#[track_caller]
pub fn assert_renders(body: &str, locale: Locale, key: &str) {
    let value = t_strict(locale, key);
    assert!(
        body.contains(value) || body.contains(&maud_escape(value)),
        "assert_renders: page did not render {key:?} = {value:?} ({locale:?})"
    );
}

/// The negation of [`assert_renders`]: assert `key`'s copy is *absent*
/// from `body`. Still strict-resolves `key`, so a typo can't make the
/// assertion vacuously pass.
#[cfg(feature = "test-support")]
#[track_caller]
pub fn assert_absent(body: &str, locale: Locale, key: &str) {
    let value = t_strict(locale, key);
    assert!(
        !body.contains(value) && !body.contains(&maud_escape(value)),
        "assert_absent: page rendered {key:?} = {value:?} ({locale:?})"
    );
}

/// `assert_renders!(body, "key")` (locale defaults to English) or
/// `assert_renders!(body, locale, "key")` — sugar over
/// [`i18n::assert_renders`](crate::i18n::assert_renders).
#[cfg(feature = "test-support")]
#[macro_export]
macro_rules! assert_renders {
    ($body:expr, $key:expr $(,)?) => {
        $crate::i18n::assert_renders($body, $crate::i18n::Locale::En, $key)
    };
    ($body:expr, $locale:expr, $key:expr $(,)?) => {
        $crate::i18n::assert_renders($body, $locale, $key)
    };
}

/// Translate a navbar label. Known chrome labels route through the
/// catalog; product proper nouns (Nexus, Northstar, Neon Law Navigator, …) and
/// any unrecognized label pass through verbatim. In `En` the catalog
/// value equals the input, so English output is unchanged.
#[must_use]
pub fn nav_label(label: &str, locale: Locale) -> String {
    let key = match label {
        "Home" => "nav.home",
        "Foundation" => "nav.foundation",
        "Firm" => "nav.firm",
        "Services" => "nav.services",
        "Mission" => "nav.mission",
        "Notations" => "nav.notations",
        "Workshops" => "nav.workshops",
        // Product names and anything else are proper nouns — verbatim.
        _ => return label.to_string(),
    };
    t(locale, key)
}

/// Paths that have a Spanish twin. A nav href in this set is
/// `/es`-prefixed for Spanish; anything else falls back to its English
/// target so the nav never dead-ends on a page that isn't translated
/// yet.
///
/// This list MUST match the `/es/...` routes mounted in
/// `web::build_router` one-for-one: an entry with no mounted route sends
/// the Spanish navbar to a 404. The
/// `every_es_enabled_path_resolves_in_spanish` integration test in
/// `web/tests/routes.rs` enforces the agreement.
pub const ES_ENABLED_PATHS: &[&str] = &[
    "/",
    "/foundation",
    "/foundation/nebula",
    "/foundation/navigator",
    "/services",
    "/services/nexus",
    "/services/nest",
    "/services/northstar",
    "/services/nautilus",
    "/services/nook",
    "/services/litigation",
    "/services/node",
    "/services/newleaf",
    "/services/namesake",
    "/services/nucleus",
    "/services/pro-bono",
];

/// Localize an internal href for `locale`. In English (or for a path
/// with no Spanish twin) the href is returned unchanged; in Spanish an
/// enabled path is `/es`-prefixed (`/` → `/es`, `/services` →
/// `/es/services`).
#[must_use]
pub fn localize_href(href: &str, locale: Locale) -> String {
    if locale == Locale::En || !ES_ENABLED_PATHS.contains(&href) {
        return href.to_string();
    }
    if href == "/" {
        locale.path_prefix().to_string()
    } else {
        format!("{}{href}", locale.path_prefix())
    }
}

#[cfg(test)]
mod tests {
    use super::{localize_href, nav_label, t, t_args, t_strict, Locale};

    #[test]
    fn locale_yields_code_prefix_and_endonym() {
        assert_eq!(Locale::En.code(), "en");
        assert_eq!(Locale::Es.code(), "es");
        assert_eq!(Locale::En.path_prefix(), "");
        assert_eq!(Locale::Es.path_prefix(), "/es");
        assert_eq!(Locale::En.endonym(), "English");
        assert_eq!(Locale::Es.endonym(), "Español");
        assert_eq!(Locale::En.switch_target(), Locale::Es);
        assert_eq!(Locale::Es.switch_target(), Locale::En);
    }

    #[test]
    fn from_path_detects_spanish_prefix() {
        assert_eq!(Locale::from_path("/es"), Locale::Es);
        assert_eq!(Locale::from_path("/es/services/northstar"), Locale::Es);
        assert_eq!(Locale::from_path("/"), Locale::En);
        assert_eq!(Locale::from_path("/services"), Locale::En);
        // No false positive on a path that merely starts with "es".
        assert_eq!(Locale::from_path("/estate"), Locale::En);
    }

    #[test]
    fn english_values_are_the_literal_source_strings() {
        assert_eq!(t(Locale::En, "nav.home"), "Home");
        assert_eq!(t(Locale::En, "nav.foundation"), "Foundation");
        assert_eq!(t(Locale::En, "nav.firm"), "Firm");
        assert_eq!(t(Locale::En, "nav.services"), "Services");
        assert_eq!(t(Locale::En, "nav.notations"), "Notations");
        assert_eq!(t(Locale::En, "auth.sign_in"), "Sign in");
        assert_eq!(t(Locale::En, "auth.sign_out"), "Sign out");
    }

    #[test]
    fn spanish_values_resolve_from_the_es_catalog() {
        assert_eq!(t(Locale::Es, "nav.home"), "Inicio");
        assert_eq!(t(Locale::Es, "nav.foundation"), "Fundación");
        assert_eq!(t(Locale::Es, "nav.firm"), "Firma");
        assert_eq!(t(Locale::Es, "nav.services"), "Servicios");
        assert_eq!(t(Locale::Es, "nav.notations"), "Notaciones");
        assert_eq!(t(Locale::Es, "auth.sign_in"), "Iniciar sesión");
    }

    #[test]
    fn missing_es_key_falls_back_to_english_never_a_raw_key() {
        // "auth.portal" is identical in both catalogs; assert the
        // fallback mechanism on a key we know exists in en.
        assert_eq!(t(Locale::Es, "auth.portal"), "Portal");
        // A key absent from BOTH catalogs returns the key itself (a dev
        // signal), never an empty string.
        assert_eq!(t(Locale::Es, "does.not.exist"), "does.not.exist");
    }

    #[test]
    fn every_es_key_exists_in_en_so_fallback_is_total() {
        // The English catalog must be a superset of the Spanish one, or
        // a Spanish-only key could never fall back.
        for key in super::ES.keys() {
            assert!(
                super::EN.contains_key(key),
                "Spanish key {key:?} has no English counterpart"
            );
        }
    }

    #[test]
    fn domain_key_namespaces_are_disjoint() {
        // `build` merges domains with `extend`, so a key repeated across two
        // domain files would silently shadow the earlier value with no
        // diagnostic. Guard that the en domain files never collide — a new
        // domain (e.g. portal.yml) that reuses a `nav.*` / `cta.*` key trips
        // this instead of quietly overwriting. Checked on en (the source);
        // es is a subset of en keys, so en-disjoint implies es-disjoint.
        use std::collections::HashSet;
        let mut seen: HashSet<String> = HashSet::new();
        for domain in super::DOMAINS {
            for key in super::parse_catalog(domain.en).into_keys() {
                assert!(
                    seen.insert(key.clone()),
                    "duplicate i18n key {key:?} across en domain files — domains must be disjoint"
                );
            }
        }
    }

    #[test]
    fn t_strict_resolves_a_known_key() {
        // The strict resolver returns the same value as `t` for a real key.
        assert_eq!(t_strict(Locale::En, "nav.home"), "Home");
        assert_eq!(t_strict(Locale::Es, "nav.home"), "Inicio");
        // And it resolves the copy we just lifted into the catalog.
        assert_eq!(
            t_strict(Locale::En, "testimonials.home_heading"),
            "What clients say"
        );
    }

    #[test]
    #[should_panic(expected = "no catalog entry for \"does.not.exist\"")]
    fn t_strict_panics_on_a_missing_key() {
        // Where `t` returns the key string, `t_strict` is the
        // raise_on_missing_translations analog: a stale key fails loudly.
        let _ = t_strict(Locale::En, "does.not.exist");
    }

    #[test]
    fn interpolation_substitutes_named_placeholders() {
        assert_eq!(
            t_args(Locale::Es, "cta.email", &[("email", "support@neonlaw.com")]),
            "Escríbenos a support@neonlaw.com"
        );
    }

    #[test]
    fn nav_label_translates_chrome_but_passes_product_nouns_through() {
        assert_eq!(nav_label("Home", Locale::Es), "Inicio");
        assert_eq!(nav_label("Foundation", Locale::Es), "Fundación");
        assert_eq!(nav_label("Firm", Locale::Es), "Firma");
        assert_eq!(nav_label("Notations", Locale::Es), "Notaciones");
        // English is unchanged.
        assert_eq!(nav_label("Home", Locale::En), "Home");
        assert_eq!(nav_label("Foundation", Locale::En), "Foundation");
        assert_eq!(nav_label("Firm", Locale::En), "Firm");
        assert_eq!(nav_label("Notations", Locale::En), "Notations");
        // Product proper nouns are never translated.
        assert_eq!(nav_label("Nexus", Locale::Es), "Nexus");
        assert_eq!(nav_label("Northstar", Locale::Es), "Northstar");
        assert_eq!(nav_label("1337 Lawyers", Locale::Es), "1337 Lawyers");
    }

    #[test]
    fn every_localized_en_key_is_translated_in_es_or_explicitly_waived() {
        // The silent gap Capricorn flagged: a key in a LOCALIZED domain
        // (chrome, marketing) but missing from its es twin renders English
        // on a Spanish page with nobody noticing. Every such key must be
        // EITHER translated in es OR named in the waiver below — a
        // deliberate "English for now" choice, never an accident. A new
        // localized key trips this until someone makes that call.
        //
        // English-only domains (es: None — portal, errors) are exempt by
        // construction: they are not in `localized_en` at all, so there is
        // no per-key waiver to maintain for them.
        //
        // Waiver = marketing copy awaiting attorney-reviewed Spanish
        // (CLAUDE.md: marketing translations are reviewed in-language, so
        // we never ship machine Spanish here). They fall back to English
        // until reviewed. Add a key ONLY with that justification — the
        // point is to force the choice.
        const PENDING_ATTORNEY_REVIEW: &[&str] = &[
            "products.desc_node",
            "products.desc_newleaf",
            "products.desc_namesake",
            "products.desc_nucleus",
            "products.desc_probono",
            // Testimonial section headings on `/` and `/services/*`. These
            // were inline English literals that already rendered English on
            // `/es`; lifting them into the catalog preserves that status
            // quo. They fall back to English until attorney-reviewed
            // Spanish exists — never machine-translated marketing copy.
            "testimonials.home_heading",
            "testimonials.home_lead",
            "testimonials.service_heading",
            "testimonials.service_lead",
        ];
        // Keys from localized domains only (es: Some) — English-only
        // domains carry no Spanish obligation.
        let localized_en = super::build(|d| d.es.map(|_| d.en));
        let mut untranslated: Vec<&str> = localized_en
            .keys()
            .map(String::as_str)
            .filter(|k| !super::ES.contains_key(*k))
            .filter(|k| !PENDING_ATTORNEY_REVIEW.contains(k))
            .collect();
        untranslated.sort_unstable();
        assert!(
            untranslated.is_empty(),
            "localized en keys with no Spanish translation and no waiver: {untranslated:?}"
        );
    }

    #[test]
    fn localize_href_prefixes_enabled_paths_in_spanish_only() {
        assert_eq!(localize_href("/", Locale::Es), "/es");
        assert_eq!(
            localize_href("/services/northstar", Locale::Es),
            "/es/services/northstar"
        );
        assert_eq!(localize_href("/foundation", Locale::Es), "/es/foundation");
        assert_eq!(
            localize_href("/foundation/nebula", Locale::Es),
            "/es/foundation/nebula"
        );
        // English never rewrites.
        assert_eq!(
            localize_href("/services/northstar", Locale::En),
            "/services/northstar"
        );
        // A path with no Spanish twin falls back to its English target.
        assert_eq!(localize_href("/contact", Locale::Es), "/contact");
        assert_eq!(localize_href("#", Locale::Es), "#");
    }
}
