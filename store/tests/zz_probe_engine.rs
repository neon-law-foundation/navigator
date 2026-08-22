//! SCRATCH PROBE — delete before commit. ENG-312.
//!
//! Does the double-commit of an identical record-key `CREATE` happen on
//! *server* mode too, or only on the embedded `mem://` engine?
//!
//! One harness, both engines, so a negative on one is comparable to a
//! positive on the other. Each round uses a fresh anchor *key* rather
//! than a fresh database, so the claim slot is new without paying a
//! schema apply per round.
//!
//! `NAVIGATOR_SURREAL_ENDPOINT` set -> server mode. Unset -> embedded.
//! `ANCHOR_ROUNDS` sets the round count.

use std::sync::Arc;
use store::entities::{self, EntityError, NewEntity};
use store::surreal::SurrealDb;

const RACERS: usize = 8;

fn anchor_input(key: &str) -> NewEntity {
    NewEntity {
        name: format!("Shook Law PLLC {key}"),
        entity_type_id: store::test_support::SEED_ENTITY_TYPE_ID,
        jurisdiction_id: store::test_support::SEED_ENTITY_JURISDICTION_ID,
        phone: None,
        url: None,
        firm_anchor_key: Some(key.to_string()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn probe_double_commit_by_engine() {
    let rounds: usize = std::env::var("ANCHOR_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);

    let (db, mode): (Arc<SurrealDb>, &str) =
        if std::env::var_os("NAVIGATOR_SURREAL_ENDPOINT").is_some() {
            let server = store::test_support::server_surreal("eng312_probe")
                .await
                .expect("NAVIGATOR_SURREAL_ENDPOINT is set, so the server lane must connect");
            (Arc::new(server.db), "server")
        } else {
            (
                Arc::new(store::test_support::mem_surreal().await),
                "embedded",
            )
        };

    let mut forks = 0;
    let mut refusals_seen = 0;
    for round in 0..rounds {
        // A fresh key per round is a fresh claim record id.
        let key = format!("shook law pllc {round}");
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..RACERS {
            let db = Arc::clone(&db);
            let key = key.clone();
            tasks.spawn(async move { entities::create(&db, &anchor_input(&key)).await });
        }

        let (mut created, mut refused) = (0, 0);
        while let Some(outcome) = tasks.join_next().await {
            match outcome.expect("a racer must not panic") {
                Ok(_) => created += 1,
                Err(EntityError::FirmAnchorTaken) => refused += 1,
                Err(other) => panic!("round {round}: unrecognised failure: {other}"),
            }
        }
        refusals_seen += refused;
        if created != 1 {
            forks += 1;
            println!("{mode}: round {round} FORKED — created={created} refused={refused}");
        }
    }

    println!(
        "{mode}: {forks} forks in {rounds} rounds ({} races), {refusals_seen} refusals observed",
        rounds * RACERS,
    );
    // Deliberately not an assertion: this probe reports a rate for both
    // engines rather than gating on one.
}
