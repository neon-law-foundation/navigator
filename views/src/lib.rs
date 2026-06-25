//! HTML view components for the Neon Law Navigator web target.
//!
//! Every page returns a `maud::Markup`. The router wires those into
//! axum responses (via maud's `axum` feature) so the handler signature
//! stays as small as `async fn home() -> Markup`. Shared chrome —
//! `<head>`, the site header, the footer — lives in [`PageLayout`]
//! so each page can focus on its own content.

pub mod assets;
pub mod brand;
pub mod components;
pub mod i18n;
pub mod layout;
pub mod lsp;
pub mod markdown;
pub mod notation;
pub mod pages;
pub mod slug;

pub use brand::SiteBrand;
pub use i18n::Locale;
pub use layout::{AuthState, PageLayout};

/// Standard 404 body. Pages can call this when a slug lookup misses.
#[must_use]
pub fn not_found_page() -> maud::Markup {
    not_found_page_with_auth(AuthState::Anonymous)
}

/// 404 with the auth-aware header (so a signed-in user still sees
/// "Admin / Sign out" in the nav).
#[must_use]
pub fn not_found_page_with_auth(auth: AuthState) -> maud::Markup {
    let body = maud::html! {
        section.not-found {
            h1 { "Not found" }
            p { "The page you asked for does not exist." }
            p { a href="/" { "Return home" } }
        }
    };
    PageLayout::new("Not found").with_auth(auth).render(&body)
}

/// Standard 403 body. Used by the policy middleware when an
/// authenticated user lacks the required role, and by the OAuth
/// callback when the IdP-supplied email has no pre-seeded `persons`
/// row.
#[must_use]
pub fn forbidden_page() -> maud::Markup {
    forbidden_page_with_auth(AuthState::Anonymous)
}

#[must_use]
pub fn forbidden_page_with_auth(auth: AuthState) -> maud::Markup {
    // Resolve the support address through the brand seam so a rebranded
    // OSS fork (or a white-label portal-only deploy) never surfaces
    // NeonLaw's `support@` on its own 403 page.
    let email = crate::brand::firm_email();
    let body = maud::html! {
        section.forbidden {
            h1 { "Forbidden" }
            p {
                "Your account is not authorized to view this page. "
                "If you think this is a mistake, contact "
                a href=(format!("mailto:{email}")) { (email) } "."
            }
            p { a href="/" { "Return home" } }
        }
    };
    PageLayout::new("Forbidden").with_auth(auth).render(&body)
}

/// The email/password sign-in page.
///
/// Rendered by `web::oauth` only when the Identity Platform password
/// front door is configured. The form posts to `/auth/password`; the
/// password the person types is validated by GCP Identity Platform, not
/// by us — Neon Law Navigator never stores or hashes a password. `csrf_token` is
/// the double-submit token also dropped as a signed cookie. When
/// `oidc_enabled`, a "Sign in with Google" link to `/auth/login/oidc`
/// sits alongside, so email/password is a first-class door rather than a
/// hidden fallback. `error` is a warm, non-enumerating message shown
/// after a rejected attempt.
/// `notice`, when set, surfaces a red Bootstrap toast on arrival — used
/// when the visitor was bounced here because a page required a login (the
/// private-mode gate), not for a voluntary sign-in.
/// A one-line banner the sign-in page floats on arrival, with a tone.
/// `Danger` (red) greets a visitor bounced here because a page required a
/// login; `Success` (green) confirms a positive outcome the user just
/// completed elsewhere (a password reset, an email confirmation).
pub enum LoginNotice<'a> {
    /// Red toast — something the visitor should heed (login required).
    Danger(&'a str),
    /// Green toast — a positive outcome just completed.
    Success(&'a str),
}

#[must_use]
pub fn login_page(
    return_to: &str,
    csrf_token: &str,
    oidc_enabled: bool,
    error: Option<&str>,
    notice: Option<LoginNotice<'_>>,
) -> maud::Markup {
    use crate::components::form::{Field, FormCard};

    let email = crate::brand::firm_email();
    let mailto = format!("mailto:{email}");
    // Trouble-signing-in help, plus the "or … Google" alternative,
    // rendered inside the same card below the form's submit row.
    let footer = maud::html! {
        @if oidc_enabled {
            div."d-flex"."align-items-center"."my-4"."text-body-secondary" {
                hr."flex-grow-1"."m-0";
                span."px-3"."small"."text-uppercase" { "or" }
                hr."flex-grow-1"."m-0";
            }
            (google_sign_in_button(return_to))
        }
        p."text-body-secondary"."small"."mt-4"."mb-1" {
            a href="/auth/password/reset" { "Forgot your password?" }
        }
        p."text-body-secondary"."small"."mb-0" {
            "Trouble signing in? Contact "
            a href=(mailto) { (email) } "."
        }
    };
    // Rebuilt on the shared FormCard + Field chrome (the admin forms'
    // card, labels, and Noto Serif typography) so the sign-in page is no
    // longer hand-rolled. The contract is unchanged: it still POSTs
    // `email` + `password` + `return_to` + `csrf_token` to
    // `/auth/password`. `csrf_token` is a hidden field (a pre-session
    // double-submit token — not the admin `_csrf`), so we pass it via
    // `hidden(...)` rather than `csrf(...)`.
    let card = FormCard::new("Sign in", "/auth/password", "Sign in")
        .centered()
        .hidden("return_to", return_to)
        .hidden("csrf_token", csrf_token)
        .error(error)
        .fields(vec![
            Field::email("Email", "email", "").required(),
            Field::input("Password", "password", "", "password").required(),
        ])
        .footer(footer);
    let body = maud::html! {
        // When the visitor was bounced here because a page required a login
        // (the private-mode gate replacing a 403), float a red toast at the
        // top-right explaining why — the shared Toast component, pinned by
        // the overlay container.
        @if let Some(notice) = notice {
            @match notice {
                LoginNotice::Danger(message) => (crate::components::toast::toast_overlay(
                    &crate::components::toast::Toast::danger(message).render())),
                LoginNotice::Success(message) => (crate::components::toast::toast_overlay(
                    &crate::components::toast::Toast::success(message).render())),
            }
        }
        (card.render())
    };
    PageLayout::new("Sign in").render(&body)
}

/// The "Forgot your password?" page — an email field that POSTs to
/// `/auth/password/reset`. `csrf_token` is the double-submit pair to the
/// signed cookie the handler sets; `error` shows a warm message after a
/// rejected submit (today only a missing/invalid CSRF token).
#[must_use]
pub fn password_reset_request_page(csrf_token: &str, error: Option<&str>) -> maud::Markup {
    use crate::components::form::{Field, FormCard};
    let intro = maud::html! {
        p."text-body-secondary"."mb-4" {
            "Enter the email address for your account and we'll send you a link to choose a new "
            "password. The link expires in 30 minutes."
        }
    };
    let footer = maud::html! {
        p."text-body-secondary"."small"."mt-4"."mb-0" {
            a href="/auth/login" { "Back to sign in" }
        }
    };
    let card = FormCard::new(
        "Reset your password",
        "/auth/password/reset",
        "Email me a reset link",
    )
    .centered()
    .hidden("csrf_token", csrf_token)
    .error(error)
    .intro(intro)
    .fields(vec![Field::email("Email", "email", "").required()])
    .footer(footer);
    PageLayout::new("Reset your password").render(&card.render())
}

/// The "Choose a new password" page reached from a valid reset link. Two
/// password fields POST to `/auth/password/reset/new` along with the
/// single-use `token` (hidden) and the `csrf_token` double-submit pair.
#[must_use]
pub fn password_reset_new_page(token: &str, csrf_token: &str, error: Option<&str>) -> maud::Markup {
    use crate::components::form::{Field, FormCard};
    let intro = maud::html! {
        p."text-body-secondary"."mb-4" {
            "Choose a new password. Use at least 8 characters."
        }
    };
    let card = FormCard::new(
        "Choose a new password",
        "/auth/password/reset/new",
        "Set new password",
    )
    .centered()
    .hidden("token", token)
    .hidden("csrf_token", csrf_token)
    .error(error)
    .intro(intro)
    .fields(vec![
        Field::input("New password", "password", "", "password").required(),
        Field::input("Confirm new password", "confirm", "", "password").required(),
    ]);
    PageLayout::new("Choose a new password").render(&card.render())
}

/// Neutral confirmation shown after a reset *request* — identical whether
/// or not an account exists, so the response can't be used to probe which
/// addresses are registered.
#[must_use]
pub fn password_reset_sent_page() -> maud::Markup {
    auth_notice_page(
        "Check your inbox",
        &maud::html! {
            p { "If an account exists for that email, we've sent a link to reset its password." }
            p."text-body-secondary"."small" {
                "The link expires in 30 minutes. Didn't get it? Check your spam folder, or "
                a href="/auth/password/reset" { "request another" } "."
            }
        },
    )
}

/// Shown when a reset / confirm link is expired, already used, or unknown
/// — one page for every dead-link case so the response leaks nothing.
#[must_use]
pub fn auth_link_invalid_page() -> maud::Markup {
    auth_notice_page(
        "This link is no longer valid",
        &maud::html! {
            p {
                "This link has expired or has already been used. Reset links can be used once and "
                "are good for 30 minutes."
            }
            p { a href="/auth/password/reset" { "Request a new reset link" } }
        },
    )
}

/// The email-confirmation gate page: a password user signed in but their
/// address isn't verified, so we sent a confirmation link and show this
/// instead of a session. `email` is echoed back into a hidden field so the
/// "resend" form (POST `/auth/email/confirm/resend`) can re-send without
/// asking again; `csrf_token` is the double-submit pair.
#[must_use]
pub fn email_confirm_required_page(email: &str, csrf_token: &str) -> maud::Markup {
    use crate::components::form::FormCard;
    let intro = maud::html! {
        p { "Almost there — please confirm your email address before signing in." }
        p."text-body-secondary" {
            "We've sent a confirmation link to your inbox. Click it and you'll be able to sign in. "
            "The link expires in 30 minutes."
        }
        p."text-body-secondary"."small" { "Didn't get it? Resend the confirmation email below." }
    };
    let card = FormCard::new(
        "Confirm your email",
        "/auth/email/confirm/resend",
        "Resend confirmation email",
    )
    .centered()
    .hidden("email", email)
    .hidden("csrf_token", csrf_token)
    .intro(intro)
    .fields(vec![]);
    PageLayout::new("Confirm your email").render(&card.render())
}

/// Shared chrome for the standalone auth notice pages (reset-sent,
/// link-invalid). A centered card with a heading and body, on the same
/// `PageLayout` as the sign-in page so the flow looks of a piece.
fn auth_notice_page(title: &str, body: &maud::Markup) -> maud::Markup {
    let inner = maud::html! {
        div."row"."justify-content-center" {
            div."col-md-6"."col-lg-5" {
                div."card"."shadow-sm" {
                    div."card-body"."p-4" {
                        h1."h4"."mb-3" { (title) }
                        (body)
                    }
                }
            }
        }
    };
    PageLayout::new(title).render(&inner)
}

/// The recognizable "Sign in with Google" button — white/surface
/// background, a 1px border, and Google's official multi-color "G" mark
/// inlined as SVG (the vendored `bi-google` glyph is monochrome). It is
/// a plain link that kicks off the existing Google OIDC redirect at
/// `/auth/login/oidc`; the button chrome lives in `.btn-google`
/// (`web/public/css/brand.css`), themed from Bootstrap surface tokens so
/// a white-label tenant's brand pack recolors it.
fn google_sign_in_button(return_to: &str) -> maud::Markup {
    let oidc_href = format!("/auth/login/oidc?return_to={return_to}");
    maud::html! {
        a."btn"."btn-google"."w-100"."d-flex"."align-items-center"."justify-content-center"."gap-2"
            href=(oidc_href)
        {
            // Google's official four-color "G", per their identity
            // branding guidelines. `aria-hidden` — the label carries
            // the accessible name.
            // `path` elements use empty `{}` (not maud's `;` void
            // syntax): inside SVG foreign content the HTML parser would
            // nest unterminated `<path>` tags instead of leaving them
            // siblings, collapsing the mark to a single stray arc.
            svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"
                width="18" height="18" aria-hidden="true" {
                path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z" {}
                path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z" {}
                path fill="#FBBC05" d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z" {}
                path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z" {}
            }
            span { "Sign in with Google" }
        }
    }
}

/// Standard 500 body for unexpected server errors. Deliberately
/// generic — never leak the underlying error to the browser.
#[must_use]
pub fn internal_error_page() -> maud::Markup {
    internal_error_page_with_auth(AuthState::Anonymous)
}

#[must_use]
pub fn internal_error_page_with_auth(auth: AuthState) -> maud::Markup {
    let body = maud::html! {
        section.server-error {
            h1 { "Something went wrong" }
            p { "We hit an unexpected error. The team has been notified." }
            p { a href="/" { "Return home" } }
        }
    };
    PageLayout::new("Server error")
        .with_auth(auth)
        .render(&body)
}

#[doc(hidden)]
pub use maud;

#[cfg(test)]
mod tests {
    use super::{
        forbidden_page, forbidden_page_with_auth, internal_error_page, login_page, not_found_page,
        AuthState,
    };

    #[test]
    fn login_page_wears_the_shared_form_card_chrome() {
        let out = login_page("/portal", "TOK123", true, None, None).into_string();
        // Built on the shared FormCard, so it inherits the admin card.
        assert!(out.contains("class=\"card shadow-sm\""), "{out}");
        // Centered standalone auth card, not the left-aligned admin column.
        assert!(out.contains("row justify-content-center"), "{out}");
        // The POST contract is intact: action, both hidden fields, and
        // the email/password controls the handler + e2e test parse.
        assert!(out.contains("action=\"/auth/password\""), "{out}");
        assert!(
            out.contains("name=\"return_to\" value=\"/portal\""),
            "{out}"
        );
        assert!(
            out.contains("name=\"csrf_token\" value=\"TOK123\""),
            "{out}"
        );
        assert!(out.contains("name=\"email\""), "{out}");
        assert!(out.contains("name=\"password\""), "{out}");
    }

    #[test]
    fn login_page_renders_the_google_button_only_when_oidc_enabled() {
        let on = login_page("/portal", "TOK", true, None, None).into_string();
        assert!(on.contains("/auth/login/oidc?return_to=/portal"), "{on}");
        assert!(on.contains("btn-google"), "{on}");
        assert!(on.contains("Sign in with Google"), "{on}");
        // The official multi-color "G" is inlined, not the mono glyph.
        assert!(on.contains("<svg") && on.contains("#4285F4"), "{on}");

        let off = login_page("/portal", "TOK", false, None, None).into_string();
        assert!(!off.contains("/auth/login/oidc"), "{off}");
        assert!(!off.contains("btn-google"), "{off}");
    }

    #[test]
    fn login_page_surfaces_the_warm_error_in_the_form_banner() {
        let out = login_page("/portal", "TOK", true, Some("Those don't match"), None).into_string();
        assert!(out.contains("class=\"alert alert-danger\""), "{out}");
        assert!(out.contains("Those don't match"), "{out}");
    }

    #[test]
    fn login_page_shows_a_red_toast_when_a_login_notice_is_set() {
        // The private-mode gate bounces an anonymous visitor here with a
        // notice instead of rendering a 403; the page greets them with a
        // red Bootstrap toast explaining why.
        let out = login_page(
            "/services",
            "TOK",
            true,
            None,
            Some(super::LoginNotice::Danger(
                "You need to log in to view that page.",
            )),
        )
        .into_string();
        assert!(out.contains("class=\"toast"), "renders a toast: {out}");
        assert!(out.contains("text-bg-danger"), "the toast is red: {out}");
        assert!(
            out.contains("You need to log in to view that page."),
            "{out}"
        );
        // No notice → no toast on a voluntary sign-in.
        let plain = login_page("/portal", "TOK", true, None, None).into_string();
        assert!(!plain.contains("toast-body"), "{plain}");
    }

    #[test]
    fn not_found_page_renders_full_document_with_heading() {
        let out = not_found_page().into_string();
        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.contains("<h1>Not found</h1>"));
        assert!(out.contains("href=\"/\""));
    }

    #[test]
    fn forbidden_page_renders_full_document_with_heading_and_support_link() {
        let out = forbidden_page().into_string();
        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.contains("<h1>Forbidden</h1>"));
        assert!(out.contains("mailto:support@neonlaw.com"));
    }

    #[test]
    fn internal_error_page_does_not_leak_specifics() {
        let out = internal_error_page().into_string();
        assert!(out.contains("<h1>Something went wrong</h1>"));
        assert!(
            !out.to_lowercase().contains("stack"),
            "500 page must not leak stack details: {out}"
        );
    }

    #[test]
    fn forbidden_page_with_authenticated_shows_portal_link_in_nav() {
        let out = forbidden_page_with_auth(AuthState::Authenticated).into_string();
        assert!(
            out.contains("href=\"/portal\">Portal</a>"),
            "authenticated 403 should still keep the portal link in the nav: {out}",
        );
    }
}
