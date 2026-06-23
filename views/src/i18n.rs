//! Internationalization: locale type + a Rails-style `t()` lookup over
//! per-locale chrome catalogs.
//!
//! Two kinds of user-visible text get two mechanisms (see
//! [`docs/i18n.md`](../../../docs/i18n.md)):
//!
//! - **Chrome** — navbar, auth links, the language switcher,
//!   CTAs — is short, repeated, and engineer-owned. It lives in the YAML
//!   catalogs under `views/locales/{en,es}.yml`, baked into the binary
//!   with `include_str!` and looked up here with [`t`] / [`t_args`].
//! - **Prose** — marketing pages, the mission letter — stays Markdown,
//!   with a parallel localized tree the `web` crate loads.
//!
//! `En` is the source locale and the universal fallback: a key missing
//! from `es.yml` resolves to the English value, never to a raw key.

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

const EN_CATALOG: &str = include_str!("../locales/en.yml");
const ES_CATALOG: &str = include_str!("../locales/es.yml");

static EN: LazyLock<HashMap<String, String>> = LazyLock::new(|| parse_catalog(EN_CATALOG));
static ES: LazyLock<HashMap<String, String>> = LazyLock::new(|| parse_catalog(ES_CATALOG));

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
/// catalog — which is a programming error, since `en.yml` is complete.
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
/// expected in production, since `en.yml` is the complete source).
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

/// Translate a navbar label. Known chrome labels route through the
/// catalog; product proper nouns (Nexus, Northstar, Navigator, …) and
/// any unrecognized label pass through verbatim. In `En` the catalog
/// value equals the input, so English output is unchanged.
#[must_use]
pub fn nav_label(label: &str, locale: Locale) -> String {
    let key = match label {
        "Home" => "nav.home",
        "The Foundation" => "nav.foundation",
        "The Firm" => "nav.firm",
        "Services" => "nav.services",
        "Mission" => "nav.mission",
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
    "/foundation/mission",
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
    use super::{localize_href, nav_label, t, t_args, Locale};

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
        // English output must be byte-identical to the legacy literals.
        assert_eq!(t(Locale::En, "nav.home"), "Home");
        assert_eq!(t(Locale::En, "nav.foundation"), "The Foundation");
        assert_eq!(t(Locale::En, "nav.firm"), "The Firm");
        assert_eq!(t(Locale::En, "nav.services"), "Services");
        assert_eq!(t(Locale::En, "auth.sign_in"), "Sign in");
        assert_eq!(t(Locale::En, "auth.sign_out"), "Sign out");
    }

    #[test]
    fn spanish_values_resolve_from_the_es_catalog() {
        assert_eq!(t(Locale::Es, "nav.home"), "Inicio");
        assert_eq!(t(Locale::Es, "nav.foundation"), "La Fundación");
        assert_eq!(t(Locale::Es, "nav.firm"), "El bufete");
        assert_eq!(t(Locale::Es, "nav.services"), "Servicios");
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
                "es.yml key {key:?} has no en.yml counterpart"
            );
        }
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
        assert_eq!(nav_label("The Foundation", Locale::Es), "La Fundación");
        assert_eq!(nav_label("The Firm", Locale::Es), "El bufete");
        // English is unchanged.
        assert_eq!(nav_label("Home", Locale::En), "Home");
        assert_eq!(nav_label("The Foundation", Locale::En), "The Foundation");
        assert_eq!(nav_label("The Firm", Locale::En), "The Firm");
        // Product proper nouns are never translated.
        assert_eq!(nav_label("Nexus", Locale::Es), "Nexus");
        assert_eq!(nav_label("Northstar", Locale::Es), "Northstar");
        assert_eq!(nav_label("1337 Lawyers", Locale::Es), "1337 Lawyers");
    }

    #[test]
    fn every_en_key_is_translated_in_es_or_explicitly_waived() {
        // The silent gap Capricorn flagged: a key in en.yml but missing
        // from es.yml renders English on a Spanish page with nobody
        // noticing. Every en key must be EITHER translated in es OR named
        // in the waiver below — a deliberate "English for now" choice,
        // never an accident. A new en key trips this until someone makes
        // that call.
        //
        // Waiver = product-catalog descriptions awaiting attorney-reviewed
        // Spanish (CLAUDE.md: marketing translations are reviewed
        // in-language, so we never ship machine Spanish here). They fall
        // back to English on /es/services until reviewed. Add a key ONLY
        // with that justification — the point is to force the choice.
        const PENDING_ATTORNEY_REVIEW: &[&str] = &[
            "products.desc_node",
            "products.desc_newleaf",
            "products.desc_namesake",
            "products.desc_nucleus",
            "products.desc_probono",
        ];
        let mut untranslated: Vec<&str> = super::EN
            .keys()
            .map(String::as_str)
            .filter(|k| !super::ES.contains_key(*k))
            .filter(|k| !PENDING_ATTORNEY_REVIEW.contains(k))
            .collect();
        untranslated.sort_unstable();
        assert!(
            untranslated.is_empty(),
            "en.yml keys with no Spanish translation and no waiver: {untranslated:?}"
        );
    }

    #[test]
    fn localize_href_prefixes_enabled_paths_in_spanish_only() {
        assert_eq!(localize_href("/", Locale::Es), "/es");
        assert_eq!(
            localize_href("/services/northstar", Locale::Es),
            "/es/services/northstar"
        );
        assert_eq!(
            localize_href("/foundation/mission", Locale::Es),
            "/es/foundation/mission"
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
