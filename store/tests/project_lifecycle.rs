//! Matter lifecycle transitions (#770): `open` → `closed` → `archived`,
//! and the `status` / `closed_at` invariant they exist to own.
//!
//! The invariant is the whole point. Open-matter routing keys off
//! `status` while the ten-year retention purge keys off `closed_at`, so a
//! row where the two disagree is routed as open while retention treats it
//! as closed, or the reverse. These tests assert the pair together after
//! every transition, never `status` alone.

use store::projects::{transition_project, NewProject, Project, ProjectCommandError, Transition};
use store::test_support::{mem_surreal, seed_entity};
use uuid::Uuid;

async fn open_matter(surreal: &store::surreal::SurrealDb, code: &str) -> Uuid {
    store::projects::create(
        surreal,
        &NewProject {
            code: code.to_string(),
            name: code.to_string(),
            status: "open".into(),
            entity_id: seed_entity(surreal).await,
            ..Default::default()
        },
    )
    .await
    .expect("insert matter")
    .id
}

async fn reload(surreal: &store::surreal::SurrealDb, id: Uuid) -> Project {
    store::projects::find_by_id(surreal, id)
        .await
        .expect("query")
        .expect("matter still exists")
}

/// The invariant, asserted as a pair: an `open` matter carries no close
/// date, and a `closed`/`archived` one carries exactly one.
fn assert_invariant(row: &Project) {
    match row.status.as_str() {
        "open" => assert!(
            row.closed_at.is_none(),
            "an open matter must carry no close date, got {:?}",
            row.closed_at
        ),
        "closed" | "archived" => assert!(
            row.closed_at.is_some(),
            "a {} matter must carry a close date — retention keys off it",
            row.status
        ),
        other => panic!("unexpected status {other}"),
    }
}

#[tokio::test]
async fn closing_stamps_the_close_date_that_starts_retention() {
    let surreal = mem_surreal().await;
    let id = open_matter(&surreal, "close-me").await;

    let closed = transition_project(&surreal, id, Transition::Close)
        .await
        .expect("close");
    assert_eq!(closed.status, "closed");
    assert!(closed.closed_at.is_some());
    assert_invariant(&closed);
}

#[tokio::test]
async fn reopening_clears_the_close_date() {
    let surreal = mem_surreal().await;
    let id = open_matter(&surreal, "reopen-me").await;

    transition_project(&surreal, id, Transition::Close)
        .await
        .expect("close");
    let reopened = transition_project(&surreal, id, Transition::Reopen)
        .await
        .expect("reopen");

    assert_eq!(reopened.status, "open");
    assert!(
        reopened.closed_at.is_none(),
        "a reopened matter that kept its close date would be purged by retention while routed as open"
    );
    assert_invariant(&reopened);
}

/// Archiving after a close keeps the date retention already knows about,
/// rather than restarting the ten-year window.
#[tokio::test]
async fn archiving_a_closed_matter_preserves_its_original_close_date() {
    let surreal = mem_surreal().await;
    let id = open_matter(&surreal, "archive-closed").await;

    let closed = transition_project(&surreal, id, Transition::Close)
        .await
        .expect("close");
    let original = closed.closed_at.clone().expect("stamped");

    let archived = transition_project(&surreal, id, Transition::Archive)
        .await
        .expect("archive");
    assert_eq!(archived.status, "archived");
    assert_eq!(
        archived.closed_at.as_deref(),
        Some(original.as_str()),
        "archiving must not restart the retention window"
    );
    assert_invariant(&archived);
}

/// Archiving straight from `open` still has to satisfy the invariant, so
/// it stamps a close date on the way.
#[tokio::test]
async fn archiving_an_open_matter_stamps_a_close_date() {
    let surreal = mem_surreal().await;
    let id = open_matter(&surreal, "archive-open").await;

    let archived = transition_project(&surreal, id, Transition::Archive)
        .await
        .expect("archive");
    assert_eq!(archived.status, "archived");
    assert!(
        archived.closed_at.is_some(),
        "an archived matter with no close date is invisible to the retention purge"
    );
    assert_invariant(&archived);
}

/// Archived is terminal. Reopening one would resurrect a matter whose
/// retention clock is already running.
#[tokio::test]
async fn archived_is_terminal() {
    let surreal = mem_surreal().await;
    let id = open_matter(&surreal, "terminal").await;
    transition_project(&surreal, id, Transition::Archive)
        .await
        .expect("archive");

    for forbidden in [Transition::Reopen, Transition::Close] {
        let err = transition_project(&surreal, id, forbidden)
            .await
            .expect_err("archived refuses this transition");
        assert!(
            matches!(err, ProjectCommandError::Invalid(_)),
            "expected an Invalid transition, got {err:?}"
        );
    }

    let row = reload(&surreal, id).await;
    assert_eq!(row.status, "archived", "the refused calls changed nothing");
    assert_invariant(&row);
}

/// Re-applying a transition the matter already made is a no-op, so a
/// double-submitted lawyer form does not churn the row or restamp a date.
#[tokio::test]
async fn re_applying_a_transition_is_a_no_op() {
    let surreal = mem_surreal().await;
    let id = open_matter(&surreal, "idempotent").await;

    let first = transition_project(&surreal, id, Transition::Close)
        .await
        .expect("close");
    let stamp = first.closed_at.clone().expect("stamped");

    let second = transition_project(&surreal, id, Transition::Close)
        .await
        .expect("close again");
    assert_eq!(
        second.closed_at.as_deref(),
        Some(stamp.as_str()),
        "a second close must not restamp the retention start"
    );

    // Re-archiving is the one no-op permitted out of the terminal state.
    transition_project(&surreal, id, Transition::Archive)
        .await
        .expect("archive");
    let again = transition_project(&surreal, id, Transition::Archive)
        .await
        .expect("re-archive is a no-op, not a failure");
    assert_eq!(again.status, "archived");
    assert_invariant(&again);
}

/// A full round trip: closing after a reopen starts a *fresh* retention
/// window, because the reopen cleared the old one.
#[tokio::test]
async fn closing_after_a_reopen_starts_a_fresh_retention_window() {
    let surreal = mem_surreal().await;
    let id = open_matter(&surreal, "round-trip").await;

    let first = transition_project(&surreal, id, Transition::Close)
        .await
        .expect("close");
    let first_stamp = first.closed_at.clone().expect("stamped");

    let reopened = transition_project(&surreal, id, Transition::Reopen)
        .await
        .expect("reopen");
    // The load-bearing step: the reopen actually cleared the stamp, so
    // the next close cannot inherit it. Asserted directly rather than
    // inferred from two clock reads differing.
    assert!(
        reopened.closed_at.is_none(),
        "the reopen must clear the date, or the next close preserves the old window"
    );

    let second = transition_project(&surreal, id, Transition::Close)
        .await
        .expect("close again");
    let second_stamp = second.closed_at.clone().expect("stamped");

    assert!(
        second_stamp >= first_stamp,
        "the fresh window cannot start before the original close: {second_stamp} < {first_stamp}"
    );
    assert_invariant(&second);
}

#[tokio::test]
async fn an_unknown_matter_is_not_found() {
    let surreal = mem_surreal().await;
    let err = transition_project(&surreal, Uuid::now_v7(), Transition::Close)
        .await
        .expect_err("unknown id");
    assert!(matches!(err, ProjectCommandError::NotFound), "{err:?}");
}

/// Every transition lands on the status it names — the mapping the API
/// verbs will key off.
#[test]
fn each_transition_names_its_target_status() {
    assert_eq!(Transition::Close.target_status(), "closed");
    assert_eq!(Transition::Reopen.target_status(), "open");
    assert_eq!(Transition::Archive.target_status(), "archived");
}
