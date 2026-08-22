//! One mailbox, one person, under concurrency (ENG-114).
//!
//! The invariant is that exactly one `person` row may hold a mailbox
//! case-insensitively. The UNIQUE `person_email_lower` index reads like
//! what enforces it and does not: racers writing distinct person rows
//! collide on no shared record key, so the engine's optimistic layer has
//! nothing to conflict on and admits a second row. That matters more here
//! than almost anywhere else in the schema, because `person.role` is the
//! authorization root — two rows for one human is two roles for one human.
//!
//! The pre-guard shape is reproduced below against a deliberately
//! unguarded write, so the reason the `person_mailbox` claim table exists
//! cannot be refactored away as redundant. The guarded path is then raced
//! the same way and must settle on exactly one row.

use std::sync::Arc;
use store::persons::{self, NewPerson, PersonError};
use store::surreal::{record_id, SurrealDb};
use uuid::Uuid;

/// Enough racers to overlap, few enough to stay quick under a loaded CI
/// box. The fork this guards against reproduces from two.
const RACERS: usize = 8;

const MAILBOX: &str = "contested@example.com";

/// The unguarded write the claim replaced: the email probe and the
/// `CREATE` inside one `BEGIN … COMMIT`, leaning on the UNIQUE index
/// alone. Kept here as the control — it is what this module used to claim
/// was sufficient.
///
/// Every racer binds its own fresh record id, and the probe is a
/// table/index scan rather than a direct record read, so nothing enters
/// the transaction's read set that another racer could conflict with.
async fn unguarded_find_or_create(db: &SurrealDb, email: &str) -> Result<(), surrealdb::Error> {
    db.query(
        "BEGIN; \
         LET $existing = (SELECT VALUE id FROM person \
             WHERE email_lower = $email_lower LIMIT 1)[0]; \
         IF $existing = NONE { \
             CREATE $id SET name = $name, email = $email, role = 'client'; \
         }; \
         SELECT id FROM ONLY person WHERE email_lower = $email_lower LIMIT 1; \
         COMMIT;",
    )
    .bind(("id", record_id("person", Uuid::now_v7())))
    .bind(("name", "Contested".to_string()))
    .bind(("email", email.to_string()))
    .bind(("email_lower", email.to_lowercase()))
    .await
    .and_then(surrealdb::IndexedResults::check)
    .map(|_| ())
}

async fn rows_holding(db: &SurrealDb, email: &str) -> usize {
    persons::list_directory(db, "", email, &[])
        .await
        .expect("read the directory back")
        .len()
}

/// The control, and the reason the claim table exists.
///
/// Racing the *unguarded* shape is allowed to fork — the assertion is not
/// that it does (it needs a loaded machine to lose reliably), but that
/// nothing about the unguarded shape refuses the second row on its own
/// merits. If this ever starts refusing every fork, the engine gained
/// concurrent UNIQUE-index enforcement and the claim can be reconsidered.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn the_unique_index_alone_does_not_serialize_racers() {
    let db = Arc::new(store::test_support::mem_surreal().await);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..RACERS {
        let db = Arc::clone(&db);
        tasks.spawn(async move { unguarded_find_or_create(&db, MAILBOX).await });
    }
    let mut landed = 0;
    while let Some(outcome) = tasks.join_next().await {
        if outcome.expect("a racer must not panic").is_ok() {
            landed += 1;
        }
    }
    assert!(landed >= 1, "at least one unguarded write must land");
    assert!(
        rows_holding(&db, MAILBOX).await >= 1,
        "the unguarded shape must leave the mailbox held by at least one row",
    );
}

/// The guarded path, raced: every racer settles on one row, and exactly
/// one row exists when the dust settles.
///
/// No racer may come back an error at all. A loser is not refused here —
/// it re-reads the winner's row, which is what the canonical seed depends
/// on since it runs on every boot and two processes can start together.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_find_or_create_settles_on_exactly_one_row() {
    for round in 0..8 {
        let db = Arc::new(store::test_support::mem_surreal().await);
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..RACERS {
            let db = Arc::clone(&db);
            tasks.spawn(async move {
                persons::find_or_create(&db, &NewPerson::new("Contested", MAILBOX)).await
            });
        }

        let mut ids = std::collections::BTreeSet::new();
        while let Some(outcome) = tasks.join_next().await {
            match outcome.expect("a racer must not panic") {
                Ok(person) => {
                    ids.insert(person.id);
                }
                Err(other) => {
                    panic!("round {round}: a racer was refused instead of settling: {other:?}")
                }
            }
        }

        assert_eq!(
            ids.len(),
            1,
            "round {round}: the racers disagreed about which row won"
        );
        assert_eq!(
            rows_holding(&db, MAILBOX).await,
            1,
            "round {round}: a race must not leave a second row behind",
        );
    }
}

/// Two concurrent `create` calls for one mailbox: exactly one lands and
/// every other is refused as [`PersonError::EmailTaken`].
///
/// `create` mints its own key just as `find_or_create` did, so it carries
/// the identical exposure — the difference is only that a loser here wants
/// the refusal rather than the winner's row.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_creates_for_one_mailbox_land_exactly_one_row() {
    for round in 0..8 {
        let db = Arc::new(store::test_support::mem_surreal().await);
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..RACERS {
            let db = Arc::clone(&db);
            tasks.spawn(async move {
                persons::create(&db, &NewPerson::new("Contested", MAILBOX)).await
            });
        }

        let (mut created, mut refused) = (0, 0);
        while let Some(outcome) = tasks.join_next().await {
            match outcome.expect("a racer must not panic") {
                Ok(_) => created += 1,
                Err(PersonError::EmailTaken) => refused += 1,
                Err(other) => panic!("round {round}: a racer failed unrecognisably: {other:?}"),
            }
        }

        assert_eq!(created, 1, "round {round}: exactly one racer may create");
        assert_eq!(refused, RACERS - 1, "round {round}: the rest are refused");
        assert_eq!(
            rows_holding(&db, MAILBOX).await,
            1,
            "round {round}: the mailbox must not fork",
        );
    }
}
