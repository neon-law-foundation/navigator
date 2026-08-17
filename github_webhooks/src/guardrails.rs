//! Durable, singleton spending guardrails for GitHub engineering automation.
//!
//! The GitHub App has one webhook stream and Navigator runs it only from
//! `neon-law-stg`.  This virtual object is keyed exclusively as `global`,
//! so every invocation competes for the same concurrency and daily-token
//! budget.  It stores identifiers and counts only; issue bodies, prompts, and
//! repository content never enter its state.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const STATE_KEY: &str = "guardrail-state";
/// The sole virtual-object key that owns the global budget.
pub const GLOBAL_GUARDRAIL_KEY: &str = "global";

/// Runtime limits for GitHub engineering automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardrailConfig {
    /// Maximum simultaneous agent invocations.
    pub max_concurrent: u32,
    /// Maximum revision passes for one pull request.
    pub max_revise_rounds: u32,
    /// Maximum agent tokens reserved during one UTC day.
    pub max_daily_tokens: u64,
}

impl GuardrailConfig {
    /// Load every required GitHub automation cap from the environment.
    ///
    /// The caller only loads this configuration in the automation-home
    /// deployment. Other environments do not bind this service and therefore
    /// cannot accidentally require or consume its singleton budget.
    pub fn from_env() -> Result<Self, GuardrailConfigError> {
        Self::from_values(|name| std::env::var(name).ok())
    }

    fn from_values(get: impl Fn(&str) -> Option<String>) -> Result<Self, GuardrailConfigError> {
        Ok(Self {
            max_concurrent: parse_limit(&get, "NAVIGATOR_GITHUB_MAX_CONCURRENT")?,
            max_revise_rounds: parse_limit(&get, "NAVIGATOR_GITHUB_MAX_REVISE_ROUNDS")?,
            max_daily_tokens: parse_limit(&get, "NAVIGATOR_GITHUB_MAX_DAILY_TOKENS")?,
        })
    }
}

fn parse_limit<T>(
    get: &impl Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<T, GuardrailConfigError>
where
    T: std::str::FromStr + PartialEq + Default,
{
    let value = get(name).filter(|value| !value.is_empty());
    let Some(value) = value else {
        return Err(GuardrailConfigError::Missing(name));
    };
    let parsed = value
        .parse::<T>()
        .map_err(|_| GuardrailConfigError::Invalid(name))?;
    if parsed == T::default() {
        return Err(GuardrailConfigError::Invalid(name));
    }
    Ok(parsed)
}

/// Invalid automation-cap configuration.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum GuardrailConfigError {
    #[error("required GitHub automation cap is missing: {0}")]
    Missing(&'static str),
    #[error("GitHub automation cap must be a positive integer: {0}")]
    Invalid(&'static str),
}

/// Request to reserve budget before an agent invocation begins.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReserveRequest {
    /// Idempotency identity for this one agent invocation.
    pub invocation_id: String,
    /// Upper-bound token budget for the invocation.
    pub token_budget: u64,
}

/// The state held by the singleton virtual object.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct GuardrailState {
    /// UTC date for `daily_tokens_used` and `paused_for_day`.
    pub budget_day: Option<String>,
    /// Tokens reserved during `budget_day`.
    pub daily_tokens_used: u64,
    /// Invocation ids currently holding a concurrency slot.
    pub active_invocations: BTreeSet<String>,
    /// The UTC day during which new reservations are paused.
    pub paused_for_day: Option<String>,
}

/// Count-only operator projection of the singleton guardrail state.
///
/// Invocation identities stay in durable state solely to make a reservation
/// idempotent. They never leave the object: operators need the active count
/// and pause condition, not a list of work identifiers.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct GuardrailStatus {
    /// UTC day whose token usage is represented here.
    pub budget_day: String,
    /// Token budget reserved during `budget_day`.
    pub daily_tokens_used: u64,
    /// Unreserved tokens left for `budget_day`.
    pub remaining_tokens: u64,
    /// Number of invocations currently occupying concurrency slots.
    pub active_invocation_count: u32,
    /// Whether new reservations are paused for the current UTC day.
    pub paused: bool,
    /// Configured global concurrency limit.
    pub max_concurrent: u32,
    /// Configured per-pull-request revision limit.
    pub max_revise_rounds: u32,
    /// Configured daily token limit.
    pub max_daily_tokens: u64,
}

/// The result of a reservation attempt.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum Reservation {
    /// The invocation owns a concurrency slot and the daily tokens were spent.
    Granted {
        daily_tokens_used: u64,
        remaining_tokens: u64,
    },
    /// A replay of an existing reservation; it does not spend twice.
    AlreadyReserved,
    /// The concurrent-invocation cap is occupied; no state was changed.
    Deferred {
        active_invocations: u32,
        max_concurrent: u32,
    },
    /// The daily budget is exhausted, so new work is paused until the reset.
    Paused {
        budget_day: String,
        max_daily_tokens: u64,
    },
}

/// Project a normalized guardrail state for operators without exposing
/// invocation identifiers.
///
/// A read after UTC midnight has the same semantics as the next reservation:
/// daily budget and pause reset, while active concurrency slots remain held.
#[must_use]
pub fn status_at(
    mut state: GuardrailState,
    config: GuardrailConfig,
    now: DateTime<Utc>,
) -> (GuardrailState, GuardrailStatus) {
    let today = normalize_day(&mut state, now);
    let active_invocation_count = u32::try_from(state.active_invocations.len()).unwrap_or(u32::MAX);
    let status = GuardrailStatus {
        budget_day: today.clone(),
        daily_tokens_used: state.daily_tokens_used,
        remaining_tokens: config
            .max_daily_tokens
            .saturating_sub(state.daily_tokens_used),
        active_invocation_count,
        paused: state.paused_for_day.as_deref() == Some(today.as_str()),
        max_concurrent: config.max_concurrent,
        max_revise_rounds: config.max_revise_rounds,
        max_daily_tokens: config.max_daily_tokens,
    };
    (state, status)
}

fn normalize_day(state: &mut GuardrailState, now: DateTime<Utc>) -> String {
    let today = now.format("%F").to_string();
    if state.budget_day.as_deref() != Some(today.as_str()) {
        state.budget_day = Some(today.clone());
        state.daily_tokens_used = 0;
        state.paused_for_day = None;
    }
    today
}

/// Apply a reservation in a pure, testable form.
///
/// A date change resets only the daily counter and pause. Active reservations
/// survive midnight so a long-running invocation still occupies its slot.
#[must_use]
pub fn reserve(
    mut state: GuardrailState,
    config: GuardrailConfig,
    request: &ReserveRequest,
    now: DateTime<Utc>,
) -> (GuardrailState, Reservation) {
    let today = normalize_day(&mut state, now);

    if state.active_invocations.contains(&request.invocation_id) {
        return (state, Reservation::AlreadyReserved);
    }
    if state.paused_for_day.as_deref() == Some(today.as_str()) {
        return (
            state,
            Reservation::Paused {
                budget_day: today,
                max_daily_tokens: config.max_daily_tokens,
            },
        );
    }
    let active = u32::try_from(state.active_invocations.len()).unwrap_or(u32::MAX);
    if active >= config.max_concurrent {
        return (
            state,
            Reservation::Deferred {
                active_invocations: active,
                max_concurrent: config.max_concurrent,
            },
        );
    }
    if request.token_budget
        > config
            .max_daily_tokens
            .saturating_sub(state.daily_tokens_used)
    {
        state.paused_for_day = Some(today.clone());
        return (
            state,
            Reservation::Paused {
                budget_day: today,
                max_daily_tokens: config.max_daily_tokens,
            },
        );
    }

    state.daily_tokens_used += request.token_budget;
    state
        .active_invocations
        .insert(request.invocation_id.clone());
    let remaining_tokens = config.max_daily_tokens - state.daily_tokens_used;
    (
        state,
        Reservation::Granted {
            daily_tokens_used: config.max_daily_tokens - remaining_tokens,
            remaining_tokens,
        },
    )
}

/// Release a concurrency slot after an invocation finishes.
#[must_use]
pub fn release(mut state: GuardrailState, invocation_id: &str) -> GuardrailState {
    state.active_invocations.remove(invocation_id);
    state
}

/// The durable singleton guardrail service, bound only at the automation home.
#[derive(Clone)]
pub struct GitHubGuardrailsService {
    config: GuardrailConfig,
}

impl GitHubGuardrailsService {
    #[must_use]
    pub const fn new(config: GuardrailConfig) -> Self {
        Self { config }
    }
}

#[restate_sdk::object(name = "devx-guardrails")]
impl GitHubGuardrailsService {
    /// Reserve a global concurrency slot and daily token budget.
    #[restate_sdk::handler]
    async fn reserve(
        &self,
        ctx: ObjectContext<'_>,
        request: Json<ReserveRequest>,
    ) -> Result<Json<Reservation>, HandlerError> {
        ensure_global_key(ctx.key())?;
        let now = ctx
            .run(|| async { Ok(Json(Utc::now())) })
            .name("read-clock")
            .await?
            .0;
        let state = load_state(&ctx).await?;
        let (state, outcome) = reserve(state, self.config, &request.0, now);
        store_state(&ctx, &state)?;
        Ok(Json(outcome))
    }

    /// Release a previously reserved concurrency slot. Token budget remains
    /// spent for the UTC day, even if the invocation later fails.
    #[restate_sdk::handler]
    async fn release(
        &self,
        ctx: ObjectContext<'_>,
        request: Json<ReserveRequest>,
    ) -> Result<(), HandlerError> {
        ensure_global_key(ctx.key())?;
        let state = release(load_state(&ctx).await?, &request.0.invocation_id);
        store_state(&ctx, &state)
    }

    /// Return the count-only singleton state for operator diagnostics.
    #[restate_sdk::handler]
    async fn status(&self, ctx: ObjectContext<'_>) -> Result<Json<GuardrailStatus>, HandlerError> {
        ensure_global_key(ctx.key())?;
        let now = ctx
            .run(|| async { Ok(Json(Utc::now())) })
            .name("read-clock")
            .await?
            .0;
        let (state, status) = status_at(load_state(&ctx).await?, self.config, now);
        store_state(&ctx, &state)?;
        Ok(Json(status))
    }
}

fn ensure_global_key(key: &str) -> Result<(), HandlerError> {
    if key == GLOBAL_GUARDRAIL_KEY {
        Ok(())
    } else {
        Err(TerminalError::new("GitHub guardrails require the global key").into())
    }
}

async fn load_state(ctx: &ObjectContext<'_>) -> Result<GuardrailState, HandlerError> {
    let Some(raw) = ctx.get::<String>(STATE_KEY).await? else {
        return Ok(GuardrailState::default());
    };
    serde_json::from_str(&raw).map_err(|error| {
        TerminalError::new(format!("invalid GitHub guardrail state: {error}")).into()
    })
}

fn store_state(ctx: &ObjectContext<'_>, state: &GuardrailState) -> Result<(), HandlerError> {
    let raw = serde_json::to_string(state)
        .map_err(|error| TerminalError::new(format!("encode GitHub guardrail state: {error}")))?;
    ctx.set(STATE_KEY, raw);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        release, reserve, status_at, GuardrailConfig, GuardrailConfigError, GuardrailState,
        Reservation, ReserveRequest,
    };
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeSet;

    const CONFIG: GuardrailConfig = GuardrailConfig {
        max_concurrent: 2,
        max_revise_rounds: 8,
        max_daily_tokens: 100,
    };

    fn at(day: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, day, 23, 59, 0).unwrap()
    }

    fn request(id: &str, tokens: u64) -> ReserveRequest {
        ReserveRequest {
            invocation_id: id.into(),
            token_budget: tokens,
        }
    }

    #[test]
    fn configuration_requires_positive_integer_limits() {
        let missing = GuardrailConfig::from_values(|_| None).unwrap_err();
        assert_eq!(
            missing,
            GuardrailConfigError::Missing("NAVIGATOR_GITHUB_MAX_CONCURRENT")
        );

        let zero = GuardrailConfig::from_values(|name| {
            Some(
                if name == "NAVIGATOR_GITHUB_MAX_REVISE_ROUNDS" {
                    "0"
                } else {
                    "1"
                }
                .into(),
            )
        })
        .unwrap_err();
        assert_eq!(
            zero,
            GuardrailConfigError::Invalid("NAVIGATOR_GITHUB_MAX_REVISE_ROUNDS")
        );
    }

    #[test]
    fn duplicate_reservation_does_not_spend_twice() {
        let (state, first) = reserve(
            GuardrailState::default(),
            CONFIG,
            &request("inv-a", 40),
            at(26),
        );
        assert!(matches!(
            first,
            Reservation::Granted {
                daily_tokens_used: 40,
                ..
            }
        ));
        let (state, replay) = reserve(state, CONFIG, &request("inv-a", 40), at(26));
        assert_eq!(replay, Reservation::AlreadyReserved);
        assert_eq!(state.daily_tokens_used, 40);
    }

    #[test]
    fn concurrency_cap_defers_without_changing_state() {
        let state = GuardrailState {
            budget_day: Some("2026-07-26".into()),
            daily_tokens_used: 40,
            active_invocations: BTreeSet::from(["inv-a".into(), "inv-b".into()]),
            paused_for_day: None,
        };
        let (next, outcome) = reserve(state.clone(), CONFIG, &request("inv-c", 20), at(26));
        assert_eq!(next, state);
        assert_eq!(
            outcome,
            Reservation::Deferred {
                active_invocations: 2,
                max_concurrent: 2,
            }
        );
    }

    #[test]
    fn over_budget_pauses_until_the_utc_rollover() {
        let (state, outcome) = reserve(
            GuardrailState::default(),
            CONFIG,
            &request("inv-a", 101),
            at(26),
        );
        assert_eq!(
            outcome,
            Reservation::Paused {
                budget_day: "2026-07-26".into(),
                max_daily_tokens: 100,
            }
        );
        let (_, still_paused) = reserve(state.clone(), CONFIG, &request("inv-b", 1), at(26));
        assert_eq!(still_paused, outcome);

        let (next, tomorrow) = reserve(state, CONFIG, &request("inv-b", 1), at(27));
        assert!(matches!(
            tomorrow,
            Reservation::Granted {
                daily_tokens_used: 1,
                ..
            }
        ));
        assert_eq!(next.budget_day.as_deref(), Some("2026-07-27"));
        assert_eq!(next.paused_for_day, None);
    }

    #[test]
    fn rollover_resets_budget_but_not_an_active_slot() {
        let (state, _) = reserve(
            GuardrailState::default(),
            CONFIG,
            &request("inv-a", 90),
            at(26),
        );
        let (state, outcome) = reserve(state, CONFIG, &request("inv-b", 20), at(27));
        assert!(matches!(
            outcome,
            Reservation::Granted {
                daily_tokens_used: 20,
                ..
            }
        ));
        assert!(state.active_invocations.contains("inv-a"));
        assert!(state.active_invocations.contains("inv-b"));
        let released = release(state, "inv-a");
        assert!(!released.active_invocations.contains("inv-a"));
        assert_eq!(released.daily_tokens_used, 20);
    }

    #[test]
    fn status_is_count_only_and_normalizes_the_daily_budget() {
        let state = GuardrailState {
            budget_day: Some("2026-07-26".into()),
            daily_tokens_used: 99,
            active_invocations: BTreeSet::from(["inv-secret-a".into(), "inv-secret-b".into()]),
            paused_for_day: Some("2026-07-26".into()),
        };

        let (state, status) = status_at(state, CONFIG, at(27));

        assert_eq!(state.budget_day.as_deref(), Some("2026-07-27"));
        assert_eq!(state.daily_tokens_used, 0);
        assert_eq!(state.active_invocations.len(), 2);
        assert!(!status.paused);
        assert_eq!(status.active_invocation_count, 2);
        assert_eq!(status.remaining_tokens, 100);
        let encoded = serde_json::to_string(&status).unwrap();
        assert!(!encoded.contains("inv-secret"));
    }
}
