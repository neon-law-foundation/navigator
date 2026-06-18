//! Welcome-email template + render.
//!
//! Three consumers today: the OAuth callback fires a welcome on a
//! brand-new `persons` insert (via the workflow worker), the
//! `/portal/admin/people` "Send welcome" button re-fires it on demand (direct
//! send from `web`), and the `workflows-service` worker dispatches
//! `email_send__welcome` steps in any workflow. Keeping the template +
//! render in one module means a change to the copy (or the subject)
//! shows up everywhere at once.

use uuid::Uuid;

use super::dispatch::EmailPayload;
use super::Template;
use crate::runtime::{StateMachineRuntime, WorkflowRuntimeError};
use crate::spec::MachineKind;
use crate::specs::welcome_spec;

/// Default subject for the welcome email when the firm brand is
/// unbranded (`NAVIGATOR_BRAND_FIRM` unset). Mirrors the template's
/// `subject:` frontmatter default; kept as a constant so a rename in
/// the template has to update this line too (visible in the diff).
/// Brand-aware sends use [`welcome_subject`].
pub const WELCOME_SUBJECT: &str = "Welcome to Neon Law";

/// Subject for the welcome email, resolved through the firm brand seam
/// (`NAVIGATOR_BRAND_FIRM`) so a rebranded fork greets its own clients
/// by name. Defaults to [`WELCOME_SUBJECT`] when the brand is unset.
#[must_use]
pub fn welcome_subject() -> String {
    format!("Welcome to {}", super::layout::EmailBrand::Firm.alt())
}

/// Raw welcome template body (markdown with YAML frontmatter).
/// Bundled via `include_str!` so the binary doesn't need to read the
/// file off disk to send mail.
pub const WELCOME_TEMPLATE: &str = include_str!("../../content/email/welcome.md");

/// Static [`Template`] entry used by [`super::template_for_slug`].
pub const TEMPLATE: Template = Template {
    subject: WELCOME_SUBJECT,
    raw: WELCOME_TEMPLATE,
};

/// Render the welcome email body: strip the YAML frontmatter, then
/// substitute the recipient tokens (`{{client_name}}`,
/// `{{client_email}}`) and the brand tokens (`{{brand}}`,
/// `{{support_email}}`, `{{site_url}}`). The brand tokens resolve
/// through the same firm-brand env seams as the rest of the email shell
/// (`NAVIGATOR_BRAND_FIRM`, `NAVIGATOR_SUPPORT_EMAIL`, `NAV_BASE_URL`)
/// so a rebranded fork's welcome never carries NeonLaw's name, address,
/// or domain.
#[must_use]
pub fn render_welcome_body(name: &str, email: &str) -> String {
    let brand = super::layout::EmailBrand::Firm.alt();
    let support = super::layout::EmailBrand::Firm.support_email();
    let site_url = super::layout::base_url_from_env();
    let body = super::strip_frontmatter(WELCOME_TEMPLATE);
    body.replace("{{client_name}}", name)
        .replace("{{client_email}}", email)
        .replace("{{brand}}", &brand)
        .replace("{{support_email}}", &support)
        .replace("{{site_url}}", &site_url)
        .replace("{{foundation_blurb}}", &foundation_blurb(&site_url))
}

/// The Foundation plug in the welcome email — the firm's 501(c)(3) arm
/// publishing the open-source corpus. Parameterized through the
/// foundation brand seam (`NAVIGATOR_BRAND_FOUNDATION`) so it greets
/// under the deployer's own foundation name. Omitted entirely on a
/// white-label app-only deploy (`NAVIGATOR_PORTAL_ONLY`), where the
/// deployer typically has no foundation to plug.
fn foundation_blurb(site_url: &str) -> String {
    if portal_only() {
        return String::new();
    }
    let foundation = super::layout::EmailBrand::Foundation.alt();
    format!(
        "- The {foundation} hosts open-source legal templates and attorney AI training at \
         <{site_url}/foundation>.\n"
    )
}

/// True when `NAVIGATOR_PORTAL_ONLY` is set — the white-label app-only
/// signal (mirrors `web::PortalOnly::from_env`; the worker reads the env
/// directly rather than depend on `web`).
fn portal_only() -> bool {
    matches!(
        std::env::var("NAVIGATOR_PORTAL_ONLY").ok().as_deref(),
        Some("true" | "1")
    )
}

/// Render the welcome email's HTML alternative: the same substituted
/// markdown body as [`render_welcome_body`], wrapped in the
/// inline-styled email layout with the firm logo. The welcome is a
/// firm email, so it carries [`EmailBrand::Firm`]. `base_url` is the
/// public origin serving `/logo-firm.png` (see
/// [`super::layout::base_url_from_env`]).
#[must_use]
pub fn render_welcome_html(name: &str, email: &str, base_url: &str) -> String {
    super::layout::render_email_html(
        &render_welcome_body(name, email),
        base_url,
        super::layout::EmailBrand::Firm,
    )
}

/// Run the ephemeral `onboarding__welcome` workflow against the
/// given runtime: `start_ephemeral` with the welcome spec keyed
/// off `person_id`, then `signal_ephemeral("signup_recorded", …)`
/// to advance into `email_send__welcome` (the worker dispatches
/// the email there), then `signal_ephemeral("email_sent", None)`
/// to close out to `END`. The `person_id` doubles as the Restate
/// invocation key so repeated triggers idempotently no-op on the
/// broker side. Errors surface as [`WorkflowRuntimeError`]; the
/// caller (`web/src/oauth.rs`) wraps the whole call in
/// fire-and-forget so a flaky broker doesn't block the OAuth
/// redirect.
pub async fn trigger_welcome(
    runtime: &dyn StateMachineRuntime,
    person_id: Uuid,
    name: &str,
    email: &str,
) -> Result<(), WorkflowRuntimeError> {
    let spec = welcome_spec();
    runtime
        .start_ephemeral(MachineKind::Workflow, person_id, &spec)
        .await?;

    let payload = EmailPayload::new(name.to_string(), email.to_string());
    let payload_json = serde_json::to_string(&payload)
        .map_err(|e| WorkflowRuntimeError::Transport(format!("payload encode: {e}")))?;

    runtime
        .signal_ephemeral(
            MachineKind::Workflow,
            person_id,
            "signup_recorded",
            Some(&payload_json),
        )
        .await?;
    runtime
        .signal_ephemeral(MachineKind::Workflow, person_id, "email_sent", None)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        render_welcome_body, render_welcome_html, trigger_welcome, welcome_subject, WELCOME_SUBJECT,
    };
    use crate::runtime::{InMemoryRuntime, StateMachineRuntime};
    use crate::spec::{MachineKind, StateName};
    use uuid::Uuid;

    #[test]
    fn render_substitutes_client_name_and_email_and_drops_frontmatter() {
        let body = render_welcome_body("Aries", "aries@example.com");
        assert!(!body.starts_with("---"), "frontmatter must be stripped");
        assert!(body.contains("Aries"));
        assert!(body.contains("aries@example.com"));
        // No template placeholder of any kind survives into the body —
        // the recipient tokens and the brand tokens (`{{brand}}`,
        // `{{support_email}}`, `{{site_url}}`) must all be substituted.
        assert!(
            !body.contains("{{"),
            "no `{{{{` placeholder may survive: {body}"
        );
    }

    #[test]
    fn render_substitutes_brand_tokens_with_defaults() {
        // With the brand env unset, the brand tokens resolve to NeonLaw's
        // defaults — proving the placeholders are wired without depending
        // on a mutated (race-prone) process env.
        let body = render_welcome_body("Aries", "aries@example.com");
        // `{{brand}}` → the firm brand name (default "Neon Law").
        assert!(
            body.contains("Welcome to Neon Law") || std::env::var("NAVIGATOR_BRAND_FIRM").is_ok(),
            "brand token substituted: {body}"
        );
        // `{{support_email}}` → the firm support address (default).
        assert!(
            body.contains("support@neonlaw.com")
                || std::env::var("NAVIGATOR_SUPPORT_EMAIL").is_ok(),
            "support token substituted: {body}"
        );
    }

    #[test]
    fn welcome_includes_the_foundation_plug_by_default() {
        // With NAVIGATOR_PORTAL_ONLY unset (the full-site deploy), the
        // welcome email plugs the Foundation's open-source corpus, named
        // through the foundation brand seam (default "Neon Law Foundation").
        let body = render_welcome_body("Aries", "aries@example.com");
        assert!(
            (body.contains("open-source legal templates") && body.contains("/foundation>"))
                || std::env::var("NAVIGATOR_PORTAL_ONLY").is_ok(),
            "default welcome email carries the Foundation plug: {body}"
        );
    }

    #[test]
    fn welcome_subject_defaults_to_brand_greeting() {
        // The brand-aware subject mirrors WELCOME_SUBJECT when the firm
        // brand env is unset.
        assert!(
            welcome_subject() == WELCOME_SUBJECT || std::env::var("NAVIGATOR_BRAND_FIRM").is_ok(),
            "welcome_subject defaults to '{WELCOME_SUBJECT}', got '{}'",
            welcome_subject()
        );
    }

    #[test]
    fn render_html_wraps_substituted_body_with_logo() {
        let html = render_welcome_html("Aries", "aries@example.com", "https://example.test");
        assert!(html.starts_with("<!doctype html>"), "full HTML document");
        assert!(html.contains("Aries"), "name substituted into HTML");
        assert!(
            html.contains(r#"src="https://example.test/public/logo-firm.png""#),
            "logo PNG embedded at the exempt /public base URL",
        );
        // The frontmatter must not survive into the rendered HTML.
        assert!(!html.contains("subject:"));
    }

    #[test]
    fn render_keeps_signature_footer_with_support_address() {
        // Pins the from-address on the body so the template change
        // doesn't quietly drift away from the inbound mailbox.
        let body = render_welcome_body("X", "x@y");
        assert!(body.contains("support@neonlaw.com"));
    }

    #[test]
    fn welcome_subject_matches_template_title() {
        // Frontmatter `subject:` is the authoritative subject. Pin it
        // so a template rename also has to update the constant.
        assert_eq!(WELCOME_SUBJECT, "Welcome to Neon Law");
    }

    #[tokio::test]
    async fn trigger_welcome_drives_inmemory_runtime_through_to_end() {
        // Smoke-tests the trigger orchestration: start + two signals
        // land the welcome workflow at END. The in-memory runtime
        // ignores the ephemeral flag (no journal), so this only
        // pins the state-transition shape — wire-level ephemeral
        // bits are covered in `runtime_restate` tests.
        let rt = InMemoryRuntime::new();
        let person_id = Uuid::from_u128(7);
        trigger_welcome(&rt, person_id, "Aries", "aries@example.com")
            .await
            .expect("welcome trigger drives in-memory runtime to END");
        let final_state = rt.current_state(MachineKind::Workflow, person_id).await;
        assert_eq!(final_state, Some(StateName::end()));
    }
}
