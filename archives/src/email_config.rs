//! Worker-side `EmailService` factory for `archives`.
//!
//! Mirrors `workflows-service::email_config` exactly — same env vars,
//! same default sender, same backend selection rule. Kept separate
//! because the `archives` binary doesn't depend on the
//! `workflows-service` crate (and shouldn't — it's a peer, not a
//! parent).
//!
//! Backend selection is driven by `NAVIGATOR_EMAIL_BACKEND`:
//!
//! - `NAVIGATOR_EMAIL_BACKEND=sendgrid` + `SENDGRID_API_KEY` set →
//!   [`workflows::SendGridEmail::new`].
//! - Any other value (including unset) →
//!   [`workflows::CapturingEmail::new`].
//!
//! `SENDGRID_FROM_EMAIL` defaults to `workflows::DEFAULT_FROM_EMAIL`
//! (`support@neonlaw.com`).

use std::sync::Arc;

use thiserror::Error;
use workflows::{CapturingEmail, EmailService, SendGridEmail, DEFAULT_FROM_EMAIL};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EmailConfigError {
    #[error("SENDGRID_API_KEY must be set when NAVIGATOR_EMAIL_BACKEND=sendgrid")]
    MissingApiKey,
}

/// Build the worker's `EmailService` from the process environment.
pub fn from_env() -> Result<Arc<dyn EmailService>, EmailConfigError> {
    from_lookup(|k| std::env::var(k).ok())
}

/// Testable seam: any `key -> Option<value>` lookup.
pub fn from_lookup<F: Fn(&str) -> Option<String>>(
    get: F,
) -> Result<Arc<dyn EmailService>, EmailConfigError> {
    let (svc, _sender) = select_backend(get)?;
    Ok(svc)
}

/// Pure inner step: pick the backend and resolve the default sender.
pub fn select_backend<F: Fn(&str) -> Option<String>>(
    get: F,
) -> Result<(Arc<dyn EmailService>, String), EmailConfigError> {
    if get("NAVIGATOR_EMAIL_BACKEND").as_deref() != Some("sendgrid") {
        return Ok((Arc::new(CapturingEmail::new()), DEFAULT_FROM_EMAIL.into()));
    }
    let api_key = get("SENDGRID_API_KEY")
        .filter(|s| !s.is_empty())
        .ok_or(EmailConfigError::MissingApiKey)?;
    let from = get("SENDGRID_FROM_EMAIL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_FROM_EMAIL.into());
    Ok((Arc::new(SendGridEmail::new(api_key, from.clone())), from))
}

#[cfg(test)]
mod tests {
    use super::{select_backend, EmailConfigError};
    use std::collections::HashMap;
    use workflows::{OutboundEmail, DEFAULT_FROM_EMAIL};

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[tokio::test]
    async fn select_backend_returns_capturing_when_env_var_unset() {
        let (svc, sender) = select_backend(|_| None).expect("dev factory cannot fail");
        assert_eq!(sender, DEFAULT_FROM_EMAIL);
        svc.send(OutboundEmail::new("ops@example.invalid", "Hi", "Body"))
            .await
            .expect("capturing backend always succeeds");
    }

    #[test]
    fn select_backend_requires_api_key_for_sendgrid_backend() {
        match select_backend(lookup(&[("NAVIGATOR_EMAIL_BACKEND", "sendgrid")])) {
            Err(EmailConfigError::MissingApiKey) => {}
            Ok(_) => panic!("expected MissingApiKey"),
        }
    }

    #[test]
    fn select_backend_honors_sendgrid_from_email_override() {
        let (_, sender) = select_backend(lookup(&[
            ("NAVIGATOR_EMAIL_BACKEND", "sendgrid"),
            ("SENDGRID_API_KEY", "SG.test"),
            ("SENDGRID_FROM_EMAIL", "ops@example.com"),
        ]))
        .expect("sendgrid factory with override");
        assert_eq!(sender, "ops@example.com");
    }
}
