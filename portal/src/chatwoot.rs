//! The Chatwoot support-chat widget — an optional, per-deployment injection
//! into the public page shell.
//!
//! The widget is off unless the deployment names a Chatwoot inbox through
//! [`NAVIGATOR_CHATWOOT_WEBSITE_TOKEN`]. That is the whole gate: production
//! carries the token, local KIND and the staging release ring do not, so a
//! visitor reading a synthetic portfolio never opens a conversation against
//! the firm's real inbox. The absent-value answer is "no widget", which also
//! keeps the strict same-origin CSP intact everywhere the token is unset —
//! the policy widens only on a deployment that asked for it.
//!
//! Gating on the token rather than on the request `Host:` header is
//! deliberate. The token is a per-deployment coordinate already, so the
//! decision lives where every other deployment coordinate lives instead of as
//! a hostname literal compiled into a published tree, and a fork that stands
//! up its own Chatwoot inbox needs no code change to use it.
//!
//! The bootstrap itself is a first-party file
//! ([`CHATWOOT_LOADER_HREF`]) rather than an inline `<script>`: same-origin
//! scripts are already admitted by `script-src 'self'`, so the widget needs no
//! nonce of its own and the vendor's `sdk.js` is the only off-origin script
//! the policy has to name.

/// The Chatwoot inbox identifier, from the deployment environment. Unset or
/// empty means no widget.
///
/// Not a secret: Chatwoot website tokens are public identifiers that appear in
/// the page a visitor's browser receives, exactly like `OAUTH_CLIENT_ID`. It
/// rides the inline Deployment env rather than the projected Secret for that
/// reason.
pub const NAVIGATOR_CHATWOOT_WEBSITE_TOKEN: &str = "NAVIGATOR_CHATWOOT_WEBSITE_TOKEN";

/// The Chatwoot installation serving the widget. Optional; defaults to
/// [`DEFAULT_CHATWOOT_BASE_URL`]. A self-hosted installation sets it, which is
/// also what keeps the CSP derivation honest — the policy names whatever
/// origin the loader will actually talk to.
pub const NAVIGATOR_CHATWOOT_BASE_URL: &str = "NAVIGATOR_CHATWOOT_BASE_URL";

/// Chatwoot Cloud, the installation the firm uses.
pub const DEFAULT_CHATWOOT_BASE_URL: &str = "https://app.chatwoot.com";

/// The first-party loader that boots the vendor SDK.
pub const CHATWOOT_LOADER_HREF: &str = "/public/js/chatwoot.js";

/// A resolved Chatwoot widget: the inbox to open conversations against, and
/// the installation origin serving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatwootWidget {
    website_token: String,
    /// `scheme://host[:port]`, with any path from the configured base URL
    /// dropped — the loader appends `/packs/js/sdk.js` and a CSP host-source
    /// is an origin rather than a path, so both consumers want the origin.
    origin: String,
}

impl ChatwootWidget {
    /// Resolve from the process environment, or `None` when this deployment
    /// carries no widget.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// [`Self::from_env`] with the environment read through `get`, so every
    /// shape is unit-testable without mutating process state.
    ///
    /// A base URL that is not an absolute `http(s)` origin resolves the widget
    /// to `None` rather than falling back to the default: a deployment that
    /// set the key meant to point somewhere, and silently serving the firm's
    /// Cloud inbox instead would be the wrong recovery.
    pub(crate) fn from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> Option<Self> {
        let non_empty = |key: &str| {
            get(key)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let website_token = non_empty(NAVIGATOR_CHATWOOT_WEBSITE_TOKEN)?;
        let origin = match non_empty(NAVIGATOR_CHATWOOT_BASE_URL) {
            Some(base) => crate::csp_asset_origin_from(&base)?,
            None => DEFAULT_CHATWOOT_BASE_URL.to_string(),
        };
        Some(Self {
            website_token,
            origin,
        })
    }

    /// The installation origin, for the CSP directives that must name it.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The same origin as a WebSocket scheme. The widget holds an ActionCable
    /// connection open for incoming messages, and `connect-src` treats
    /// `wss://` as a distinct source from `https://` — naming only the HTTPS
    /// origin leaves the bubble rendered but permanently silent.
    #[must_use]
    pub fn websocket_origin(&self) -> String {
        match self.origin.split_once("://") {
            Some(("https", authority)) => format!("wss://{authority}"),
            Some(("http", authority)) => format!("ws://{authority}"),
            _ => self.origin.clone(),
        }
    }

    /// The `<script>` element that boots the widget, for injection at the end
    /// of a public page's `<body>`.
    ///
    /// Both values ride `data-` attributes rather than an inline literal so
    /// the loader stays a static, cacheable, same-origin file. They are
    /// attribute-escaped even though a website token is alphanumeric and the
    /// origin has already been parsed: the values come from deployment
    /// configuration, and an injection sink is not the place to reason about
    /// whether the input could have been hostile.
    #[must_use]
    pub fn script_tag(&self) -> String {
        format!(
            "<script src=\"{href}\" data-website-token=\"{token}\" data-base-url=\"{base}\" defer></script>",
            href = CHATWOOT_LOADER_HREF,
            token = webapp::html_escape::escape_attr(&self.website_token),
            base = webapp::html_escape::escape_attr(&self.origin),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChatwootWidget, DEFAULT_CHATWOOT_BASE_URL, NAVIGATOR_CHATWOOT_BASE_URL,
        NAVIGATOR_CHATWOOT_WEBSITE_TOKEN,
    };

    fn resolve(pairs: &[(&str, &str)]) -> Option<ChatwootWidget> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        ChatwootWidget::from_lookup(|key| {
            owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
        })
    }

    /// The gate: no token, no widget. This is the row that keeps the firm's
    /// live inbox off local KIND and off the staging release ring.
    #[test]
    fn absent_or_blank_token_resolves_to_no_widget() {
        assert_eq!(resolve(&[]), None);
        assert_eq!(resolve(&[(NAVIGATOR_CHATWOOT_WEBSITE_TOKEN, "")]), None);
        assert_eq!(resolve(&[(NAVIGATOR_CHATWOOT_WEBSITE_TOKEN, "   ")]), None);
        // A base URL alone is not a widget: the token is what names an inbox.
        assert_eq!(
            resolve(&[(NAVIGATOR_CHATWOOT_BASE_URL, "https://chat.example.com")]),
            None
        );
    }

    #[test]
    fn a_token_alone_resolves_against_chatwoot_cloud() {
        let widget = resolve(&[(NAVIGATOR_CHATWOOT_WEBSITE_TOKEN, "tok3n")])
            .expect("a token resolves a widget");
        assert_eq!(widget.origin(), DEFAULT_CHATWOOT_BASE_URL);
        assert_eq!(widget.websocket_origin(), "wss://app.chatwoot.com");
    }

    /// A configured base URL is reduced to its origin — the loader appends its
    /// own path, and a CSP host-source cannot carry one.
    #[test]
    fn a_configured_base_url_is_reduced_to_its_origin() {
        let widget = resolve(&[
            (NAVIGATOR_CHATWOOT_WEBSITE_TOKEN, "tok3n"),
            (NAVIGATOR_CHATWOOT_BASE_URL, "https://chat.example.com/sub/"),
        ])
        .expect("an absolute base URL resolves");
        assert_eq!(widget.origin(), "https://chat.example.com");
        assert_eq!(widget.websocket_origin(), "wss://chat.example.com");
    }

    #[test]
    fn a_plain_http_installation_keeps_its_scheme_on_both_directives() {
        let widget = resolve(&[
            (NAVIGATOR_CHATWOOT_WEBSITE_TOKEN, "tok3n"),
            (NAVIGATOR_CHATWOOT_BASE_URL, "http://localhost:3000"),
        ])
        .expect("an absolute base URL resolves");
        assert_eq!(widget.origin(), "http://localhost:3000");
        assert_eq!(widget.websocket_origin(), "ws://localhost:3000");
    }

    /// A base URL that is not an absolute origin is refused rather than
    /// silently replaced by Chatwoot Cloud — a deployment that set the key
    /// meant to point somewhere else, and pointing it at the firm's own inbox
    /// instead is the one recovery nobody wants.
    #[test]
    fn a_relative_or_schemeless_base_url_refuses_the_widget() {
        for base in [
            "/chat",
            "chat.example.com",
            "ftp://chat.example.com",
            "https://",
        ] {
            assert_eq!(
                resolve(&[
                    (NAVIGATOR_CHATWOOT_WEBSITE_TOKEN, "tok3n"),
                    (NAVIGATOR_CHATWOOT_BASE_URL, base),
                ]),
                None,
                "`{base}` is not an absolute origin"
            );
        }
    }

    /// The loader is same-origin and the vendor values ride `data-`
    /// attributes, so the tag carries no inline JavaScript to nonce.
    #[test]
    fn the_script_tag_is_a_same_origin_loader_carrying_data_attributes() {
        let widget = resolve(&[(NAVIGATOR_CHATWOOT_WEBSITE_TOKEN, "tok3n")]).expect("resolves");
        let tag = widget.script_tag();
        assert_eq!(
            tag,
            "<script src=\"/public/js/chatwoot.js\" data-website-token=\"tok3n\" \
             data-base-url=\"https://app.chatwoot.com\" defer></script>"
        );
    }

    /// A hostile token cannot break out of the attribute it is written into.
    #[test]
    fn the_script_tag_escapes_its_attribute_values() {
        let widget = resolve(&[(
            NAVIGATOR_CHATWOOT_WEBSITE_TOKEN,
            "\"><script>alert(1)</script>",
        )])
        .expect("resolves");
        let tag = widget.script_tag();
        assert!(
            !tag.contains("<script>alert"),
            "the token cannot open a tag: {tag}"
        );
        assert!(tag.contains("&quot;&gt;"), "the token is escaped: {tag}");
    }
}
