//! Re-exports of the journal helpers from [`store::notation_events`]. Kept
//! as a thin module so existing call sites inside this crate read
//! naturally.
//!
//! `notation_events` moved to `SurrealDB` with wave five of #1093
//! (ENG-121); its own test module (`store::notation_events`) covers this
//! behavior, so it is not duplicated here.

pub use store::notation_events::{
    answer_payload, append_event, workflow_payload, TransitionRecord,
};
