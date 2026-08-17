//! The native dependency tier — every local dependency as a host
//! process instead of a pod.
//!
//! This is the default local lane; `--runtime kind` selects the cluster
//! lane in [`super::orchestrate`]. Both write the same `.devx/env` and
//! `.devx/worktree.json`, so everything downstream — `dev grant-lawyer`,
//! `dev browser-e2e`, `cargo run -p neon` — is identical under either.
//! The env file, not the topology, is the contract.
//!
//! Isolation is logical rather than physical. Most dependencies are one
//! long-lived shared process serving every worktree through a tenant
//! key, which is what makes a second `worktree-env up` fast: it connects
//! to processes that are already running instead of building a cluster.
//!
//! | Dependency | Process | Per-worktree tenant |
//! |---|---|---|
//! | `SurrealDB` | shared | database inside the `navigator` namespace |
//! | Rauthy | shared | none needed — one client, wildcard localhost redirect |
//! | Garage | shared | bucket pair |
//! | Restate | **per worktree** | — |
//! | `workflows-service` | **per worktree** | — |
//!
//! Restate is the exception, and not by oversight. Its OSS server has no
//! namespace or environment concept, and a service name resolves to one
//! active deployment endpoint — so a second worktree registering
//! `workflows-service` would silently take over the first worktree's
//! invocations and execute them against its own store handle. The SDK
//! offers no runtime rename either: `restate_sdk::service::ServiceDefinition`
//! keeps its `discovery` field crate-private and exposes only `options`.
//! One `restate-server` per worktree is the fix, and it is cheap next to
//! the cluster it replaces.
//!
//! # What is implemented here, and what is not
//!
//! The sharing above is the destination, not this module's current
//! behavior. Today every process is started by, recorded in, and
//! reclaimed with **one worktree** — the registry that lets a second
//! checkout adopt a running `SurrealDB` instead of starting its own is
//! ENG-129, and inverting `sweep` so it can never reclaim a shared
//! process belongs with it. Per-worktree processes are correct in the
//! meantime because each worktree already owns an exclusive port slot;
//! sharing is the optimization, not the correctness.
//!
//! Two members of the tier have no host process yet. `restate-server`
//! and `workflows-service` are ENG-130; `OpenObserve` and `ClamAV` are
//! ENG-131. Both gaps are declared in [`DEFERRED`] rather than dropped
//! from the readiness gate — see [`super::worktree_env`]'s gate table
//! and the test that forces every gated port to be either supervised
//! here or attributed to the issue that will supervise it.

mod garage;
mod preflight;
mod rauthy;
mod supervisor;
mod surreal;

use std::path::Path;

use anyhow::{Context, Result};

use super::KindConfig;

/// Ledger keys, log-file stems, and `.devx/native/<label>/` directory
/// names. Stable: a rename orphans a running process's record, which is
/// how `down` comes to leave a listener behind.
const RAUTHY_LABEL: &str = "rauthy";
const GARAGE_LABEL: &str = "garage";
const SURREAL_LABEL: &str = "surreal";

/// Ports the tier binds that nothing outside it connects to.
///
/// The slot table in [`super::worktree_env`] reserves a port for every
/// address the *workspace* reaches — those end up in `.devx/env`. These
/// four do not: Garage's RPC and admin listeners and Rauthy's two
/// embedded Hiqlite listeners are internal to their own processes. They
/// still have to be per-worktree, because at their defaults the second
/// checkout to start would fail to bind, so they are derived from the
/// same slot here rather than widening a config the rest of the
/// workspace reads. The bases sit above `21_299`, past the last range
/// the slot table claims.
const GARAGE_RPC_PORT_BASE: u16 = 21_300;
const GARAGE_ADMIN_PORT_BASE: u16 = 21_400;
const RAUTHY_RAFT_PORT_BASE: u16 = 21_500;
const RAUTHY_API_PORT_BASE: u16 = 21_600;

/// Readiness-gate members this lane starts and health-gates.
///
/// Spelled exactly as [`super::worktree_env`]'s gate table spells them —
/// the two lists are compared by a test, so a typo here is a failing
/// build rather than a port that silently answers for nothing.
pub(super) const SUPERVISED: &[&str] = &["Rauthy", "Garage", "SurrealDB"];

/// Readiness-gate members this lane has no host process for yet, and the
/// issue that gives each one.
///
/// Declared rather than dropped. A gate that quietly shrinks to the
/// ports a lane happens to serve stops being a gate: it reports ready
/// while half the tier is missing, and the next person to add a
/// dependency has no signal that one lane never got it. Every entry here
/// is a port `worktree-env up --runtime native` will *not* satisfy, said
/// out loud at the end of the run.
pub(super) const DEFERRED: &[(&str, &str)] = &[
    ("KIND ingress HTTP", "ENG-132"),
    ("KIND ingress HTTPS", "ENG-132"),
    ("Restate ingress", "ENG-130"),
    ("Restate admin", "ENG-130"),
    ("OpenObserve", "ENG-131"),
    ("OpenObserve OTLP", "ENG-131"),
    ("ClamAV", "ENG-131"),
];

/// Converge this host's toolchain on the pinned native dependencies.
///
/// The entry point behind `navigator dev install`. The platform string
/// is injected into [`preflight::ensure`] rather than read inside it, so
/// the non-macOS refusal stays testable without a non-macOS machine.
///
/// Rauthy comes last because it is the only step that can take minutes:
/// a Homebrew failure should surface before a developer waits on a
/// source build.
pub(super) fn install() -> Result<()> {
    preflight::ensure(std::env::consts::OS)?;
    rauthy::resolve()?;
    Ok(())
}

/// Start this worktree's native dependency tier and bootstrap it.
///
/// Idempotent, like the cluster lane's `up`: a process that is already
/// serving its port is left alone, and an existing Garage key is read
/// back rather than re-minted. That is what makes a repeated
/// `worktree-env up` — after a reboot, or just as a re-check — cheap.
///
/// The bootstrap that follows is the same work the cluster lane does,
/// reached differently: Garage's layout, keys, and buckets come from the
/// local binary instead of `kubectl exec`. The Surreal schema apply is
/// not repeated — it is already lane-neutral and stays with the caller.
pub(super) fn up(root: &Path, slot: u16, cfg: &KindConfig) -> Result<()> {
    preflight::ensure(std::env::consts::OS)?;
    let services = [
        surreal::service(root, cfg.surreal_port)?,
        garage::service(
            root,
            cfg.garage_s3_port,
            GARAGE_RPC_PORT_BASE + slot,
            GARAGE_ADMIN_PORT_BASE + slot,
        )?,
        rauthy::service(
            root,
            cfg.rauthy_port,
            RAUTHY_RAFT_PORT_BASE + slot,
            RAUTHY_API_PORT_BASE + slot,
        )?,
    ];
    supervisor::ensure_all(root, &services)?;
    super::garage::export(&garage::provision(root).context("provision native object storage")?);
    Ok(())
}

/// Stop every process this worktree started and remove their state.
///
/// Idempotent and scoped: only PIDs this worktree recorded, and only
/// after re-identifying each one, so a teardown can never reach another
/// checkout's tier or a stranger that inherited a PID.
pub(super) fn down(root: &Path) -> Result<()> {
    supervisor::stop_all(root);
    let state = root.join(".devx").join("native");
    if state.exists() {
        std::fs::remove_dir_all(&state).with_context(|| format!("remove {}", state.display()))?;
    }
    Ok(())
}

/// `status` lines for the processes this worktree started.
pub(super) fn status_lines(root: &Path) -> Vec<String> {
    supervisor::report(root)
        .into_iter()
        .map(|(label, pid, port, live)| {
            format!(
                "  {label} 127.0.0.1:{port} (pid {pid}): {}",
                if live { "yes" } else { "no" }
            )
        })
        .collect()
}

/// The lines naming what this lane does not serve yet.
///
/// Printed at the end of `up` so an operator reads the gap from the run
/// that produced it rather than from an issue tracker.
pub(super) fn deferred_lines() -> Vec<String> {
    let mut lines = vec![
        "==> not on the native lane yet — these dependency-tier ports stay unserved:".to_string(),
    ];
    lines.extend(
        DEFERRED
            .iter()
            .map(|(member, issue)| format!("    {member} ({issue})")),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::{
        deferred_lines, DEFERRED, GARAGE_ADMIN_PORT_BASE, GARAGE_RPC_PORT_BASE,
        RAUTHY_API_PORT_BASE, RAUTHY_RAFT_PORT_BASE, SUPERVISED,
    };
    use std::collections::BTreeSet;

    /// The slot table's last range starts at `21_200` and spans 100. An
    /// internal port base below `21_300` would hand a worktree a number
    /// another worktree's `SurrealDB` already holds — a collision the slot
    /// machinery cannot see, because these ports never reach it.
    #[test]
    fn every_internal_port_base_sits_past_the_slot_tables_last_range() {
        for base in [
            GARAGE_RPC_PORT_BASE,
            GARAGE_ADMIN_PORT_BASE,
            RAUTHY_RAFT_PORT_BASE,
            RAUTHY_API_PORT_BASE,
        ] {
            assert!(base >= 21_300, "{base} overlaps the slot table");
        }
    }

    /// Four bases, four ranges. Two sharing a base would put two
    /// listeners of the same worktree on one port, which presents as one
    /// of them failing to start for no visible reason.
    #[test]
    fn the_internal_port_ranges_do_not_overlap_each_other() {
        let bases = BTreeSet::from([
            GARAGE_RPC_PORT_BASE,
            GARAGE_ADMIN_PORT_BASE,
            RAUTHY_RAFT_PORT_BASE,
            RAUTHY_API_PORT_BASE,
        ]);

        assert_eq!(bases.len(), 4);
        let ordered: Vec<u16> = bases.into_iter().collect();
        for pair in ordered.windows(2) {
            assert!(
                pair[1] - pair[0] >= 100,
                "{pair:?} are closer together than one slot span"
            );
        }
    }

    /// A member cannot be both served and deferred: that reads as
    /// "supervised" to the gate and as "known gap" to the operator, and
    /// only one of them can be true.
    #[test]
    fn no_gate_member_is_both_supervised_and_deferred() {
        for (member, _) in DEFERRED {
            assert!(
                !SUPERVISED.contains(member),
                "{member} is claimed by both lists"
            );
        }
    }

    /// Every gap names the issue that closes it. An unattributed entry
    /// is indistinguishable from something nobody intends to do.
    #[test]
    fn every_deferred_member_names_the_issue_that_will_serve_it() {
        for (member, issue) in DEFERRED {
            assert!(
                issue.starts_with("ENG-"),
                "{member} defers to `{issue}`, which is not an issue identifier"
            );
        }
    }

    /// The gap is reported by the run that has it, not left for someone
    /// to look up. The issue identifiers are the actionable part, so
    /// they have to reach the output.
    #[test]
    fn the_reported_gap_names_each_member_and_its_issue() {
        let printed = deferred_lines().join("\n");

        for (member, issue) in DEFERRED {
            assert!(printed.contains(member), "{printed}");
            assert!(printed.contains(issue), "{printed}");
        }
    }
}
