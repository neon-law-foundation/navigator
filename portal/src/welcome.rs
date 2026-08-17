//! Welcome-email template + render — re-exports.
//!
//! The template lives in `workflows::email::welcome` so the
//! `workflows-service` worker can render the same body when it
//! dispatches an `email_send__welcome` step. `web` keeps two direct
//! callers (the admin "Send welcome" button and, transitionally, the
//! OAuth callback) so the re-exports below preserve their import
//! paths.

pub use workflows::email::welcome::{
    render_welcome_body, render_welcome_html, welcome_subject, WELCOME_SUBJECT,
};
