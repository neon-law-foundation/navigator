//! Start, health-gate, record, and stop the native tier's host processes.
//!
//! The cluster lane hands supervision to Kubernetes: a Deployment restarts
//! its pod, and `kind delete cluster` reclaims the whole tier at once.
//! Nothing on a host does that, so this module is the substitute — and it
//! is deliberately the smallest one that is still safe.
//!
//! Three properties carry that safety:
//!
//! - **A recorded PID is re-identified before it is signalled.** PIDs are
//!   reused. A ledger entry that outlives its process names a stranger,
//!   and a naive `down` would kill it. [`owns_pid`] re-reads the running
//!   process's command line and only signals a match.
//! - **A process is not reported started until its port answers.**
//!   [`super::super::wait_for_tcp`] is the same gate the cluster lane
//!   applies to its port-forwards, so both lanes mean the same thing by
//!   "ready".
//! - **A start that never binds surfaces its own log.** A dependency that
//!   dies on a bad flag would otherwise present as a bare connection
//!   timeout thirty seconds later, with the reason sitting unread in a
//!   file.
//!
//! Ownership *across* worktrees — one engine serving every checkout —
//! is ENG-129. Everything here is scoped to a single worktree's `.devx/`,
//! which is what makes `down` unambiguous at this stage: every process in
//! the ledger was started by this checkout and is reclaimed with it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// How long a `SIGTERM`ed dependency gets to exit before `SIGKILL`.
const TERM_GRACE: Duration = Duration::from_secs(5);

/// Log lines quoted when a process fails to bind its port.
const LOG_TAIL_LINES: usize = 20;

/// One host process the native tier supervises.
pub(super) struct Service {
    /// Stable name — the ledger key, the log file stem, and how `status`
    /// and errors spell this dependency.
    pub(super) label: &'static str,
    /// Absolute path to the executable. Resolved by the caller rather
    /// than looked up here, so a `PATH` change between `install` and `up`
    /// cannot silently swap the engine under a running tier.
    pub(super) program: PathBuf,
    pub(super) args: Vec<String>,
    /// Extra environment for the child, on top of the inherited process
    /// environment.
    pub(super) env: Vec<(String, String)>,
    /// Working directory. Several of these dependencies resolve their
    /// data directory relative to it.
    pub(super) cwd: PathBuf,
    /// The port that must accept a TCP connection before this service
    /// counts as started.
    pub(super) port: u16,
}

/// A started process, as recorded in the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Started {
    pub(super) label: String,
    pub(super) pid: u32,
    pub(super) port: u16,
    /// The executable's file name, re-checked against the live process
    /// before `down` signals the PID. See [`owns_pid`].
    pub(super) program: String,
}

/// Where this worktree records the processes it started.
pub(super) fn ledger_path(root: &Path) -> PathBuf {
    root.join(".devx").join("native-processes.json")
}

/// The directory holding one service's data and log.
pub(super) fn service_dir(root: &Path, label: &str) -> PathBuf {
    root.join(".devx").join("native").join(label)
}

fn log_path(root: &Path, label: &str) -> PathBuf {
    service_dir(root, label).join("process.log")
}

pub(super) fn read_ledger(root: &Path) -> Vec<Started> {
    fs::read_to_string(ledger_path(root))
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

fn write_ledger(root: &Path, records: &[Started]) -> Result<()> {
    let path = ledger_path(root);
    let parent = path.parent().context("ledger path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let body = serde_json::to_string_pretty(records).context("serialize the process ledger")?;
    fs::write(&path, body).with_context(|| format!("write {}", path.display()))
}

/// The executable's file name — what `ps` prints as `argv[0]`'s basename
/// and what [`owns_pid`] matches on.
fn program_name(program: &Path) -> String {
    program
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether a live process's command line is the program we recorded.
///
/// This is the guard against signalling a recycled PID. `ps -o command=`
/// prints the full argv, so an exact-equality check would be brittle
/// against flag changes; the executable's own name is the stable part,
/// and matching it is enough to distinguish our process from whatever
/// unrelated process inherited the number.
///
/// Empty output means the PID is gone, which is not ownership.
pub(super) fn owns_pid(ps_command: &str, program: &str) -> bool {
    !program.is_empty()
        && ps_command
            .split_whitespace()
            .next()
            .is_some_and(|argv0| Path::new(argv0).file_name().is_some_and(|n| n == program))
}

/// Whether a ledger entry still describes the service we are about to
/// start. A slot change moves a port and a version bump can move a
/// binary, so a stale entry must be replaced rather than reused.
pub(super) fn describes(record: &Started, service: &Service) -> bool {
    record.label == service.label
        && record.port == service.port
        && record.program == program_name(&service.program)
}

/// The last few lines of a log, for an error message.
pub(super) fn tail(log: &str, lines: usize) -> String {
    let all: Vec<&str> = log.lines().filter(|line| !line.trim().is_empty()).collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// What `ps` reports for a PID, or `None` when it has exited.
fn ps_command(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-o", "command="])
        .arg("-p")
        .arg(pid.to_string())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!command.is_empty()).then_some(command)
}

/// Whether the recorded process is still alive and still ours.
fn still_ours(record: &Started) -> bool {
    ps_command(record.pid).is_some_and(|command| owns_pid(&command, &record.program))
}

fn port_listening(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}")
            .parse()
            .expect("a loopback socket address is well-formed"),
        Duration::from_millis(200),
    )
    .is_ok()
}

/// Start every service that is not already running, health-gate each on
/// its port, and record the result.
///
/// Idempotent, which is what makes a repeated `worktree-env up` cheap: a
/// service whose ledger entry is still alive, still ours, and still
/// listening is left exactly as it is.
pub(super) fn ensure_all(root: &Path, services: &[Service]) -> Result<Vec<Started>> {
    let existing = read_ledger(root);
    let mut records = Vec::with_capacity(services.len());
    for service in services {
        let reusable = existing
            .iter()
            .find(|record| describes(record, service))
            .filter(|record| still_ours(record) && port_listening(record.port));
        match reusable {
            Some(record) => {
                eprintln!(
                    "==> {} already serving 127.0.0.1:{} (pid {})",
                    service.label, record.port, record.pid
                );
                records.push(record.clone());
            }
            None => records.push(start(root, service)?),
        }
    }
    write_ledger(root, &records)?;
    Ok(records)
}

/// Spawn one service and wait for its port.
fn start(root: &Path, service: &Service) -> Result<Started> {
    let dir = service_dir(root, service.label);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    // Truncated, not appended. This log is only ever opened for a
    // process about to be started, so the previous run's contents are
    // finished business — and leaving them in place puts an old failure
    // directly above a new one in the tail below, which reads as one
    // event.
    let log = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path(root, service.label))
        .with_context(|| format!("open {}", log_path(root, service.label).display()))?;
    let log_err = log
        .try_clone()
        .context("duplicate the process log handle")?;

    eprintln!(
        "==> starting {} on 127.0.0.1:{}",
        service.label, service.port
    );
    let mut command = Command::new(&service.program);
    command
        .args(&service.args)
        .envs(service.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(&service.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    detach(&mut command);
    let child = command
        .spawn()
        .with_context(|| format!("spawn {}", service.program.display()))?;
    let pid = child.id();
    // Detached on purpose: the tier outlives this CLI invocation, and
    // `Child::drop` does not kill a process it never waited on.
    std::mem::forget(child);

    let record = Started {
        label: service.label.to_string(),
        pid,
        port: service.port,
        program: program_name(&service.program),
    };
    if let Err(err) = super::super::wait_for_tcp("127.0.0.1", service.port) {
        // The process either died or never bound. Reclaim it before
        // reporting, so a failed `up` does not leave an unrecorded
        // process holding a data directory open.
        signal(&record);
        let log = fs::read_to_string(log_path(root, service.label)).unwrap_or_default();
        bail!(
            "{} did not accept connections on 127.0.0.1:{}: {err}\n\
             last lines of {}:\n{}",
            service.label,
            service.port,
            log_path(root, service.label).display(),
            tail(&log, LOG_TAIL_LINES),
        );
    }
    Ok(record)
}

/// Stop every process this worktree recorded, then clear the ledger.
///
/// Idempotent and best-effort: a process that already exited, or whose
/// PID now belongs to something else, is skipped rather than signalled.
/// Infallible on purpose: a teardown that can fail is a teardown an
/// operator learns to re-run, and every step here is already a no-op
/// against a tier that is not there.
pub(super) fn stop_all(root: &Path) {
    for record in read_ledger(root) {
        if still_ours(&record) {
            eprintln!("==> stopping {} (pid {})", record.label, record.pid);
            signal(&record);
        }
    }
    let _ = fs::remove_file(ledger_path(root));
}

/// `SIGTERM`, then `SIGKILL` if the process is still there.
///
/// Garage flushes on `SIGTERM`, so the grace period is not politeness —
/// a `SIGKILL`ed process leaves a data directory that needs recovery on
/// the next start.
fn signal(record: &Started) {
    kill("-TERM", record.pid);
    let deadline = Instant::now() + TERM_GRACE;
    while Instant::now() < deadline {
        if !still_ours(record) {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    kill("-KILL", record.pid);
}

fn kill(signal: &str, pid: u32) {
    let _ = Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// One line per recorded process for `status`: label, PID, port, and
/// whether that port is answering right now.
pub(super) fn report(root: &Path) -> Vec<(String, u32, u16, bool)> {
    read_ledger(root)
        .into_iter()
        .map(|record| {
            let live = still_ours(&record) && port_listening(record.port);
            (record.label, record.pid, record.port, live)
        })
        .collect()
}

/// Put the child in its own process group so a Ctrl-C in the terminal
/// that ran `up` does not tear the tier down with it.
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn detach(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::{
        describes, ledger_path, owns_pid, program_name, service_dir, tail, Service, Started,
    };
    use std::path::{Path, PathBuf};

    fn service() -> Service {
        Service {
            label: "surreal",
            program: PathBuf::from("/opt/homebrew/bin/surreal"),
            args: vec!["start".into(), "memory".into()],
            env: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            port: 20_034,
        }
    }

    /// PIDs are reused. Signalling a recorded number without re-reading
    /// what now holds it is how a teardown kills an unrelated process, so
    /// the match is on the executable rather than on presence.
    #[test]
    fn a_recycled_pid_is_not_recognized_as_ours() {
        assert!(owns_pid(
            "/opt/homebrew/bin/surreal start memory --bind 0.0.0.0:20034",
            "surreal"
        ));
        assert!(!owns_pid("/usr/bin/vim notes.txt", "surreal"));
        assert!(!owns_pid("", "surreal"));
        assert!(!owns_pid("/opt/homebrew/bin/garage server", "surreal"));
    }

    /// An empty program name would match anything with an argv[0], which
    /// is the one input that must never be treated as ownership.
    #[test]
    fn an_empty_program_name_never_claims_a_pid() {
        assert!(!owns_pid("/usr/bin/anything", ""));
    }

    /// `ps` prints the absolute path the process was launched with. The
    /// ledger records only the file name, so the comparison has to be on
    /// the basename — matching the whole string would fail for every
    /// process we actually start.
    #[test]
    fn ownership_matches_the_basename_not_the_full_path() {
        assert!(owns_pid(
            "/opt/homebrew/bin/surreal start memory",
            "surreal"
        ));
        assert_eq!(
            program_name(Path::new("/opt/homebrew/bin/surreal")),
            "surreal"
        );
    }

    /// A slot change moves every port. Reusing a ledger entry whose port
    /// no longer matches would report a service ready at an address
    /// nothing is listening on.
    #[test]
    fn a_ledger_entry_on_a_different_port_does_not_describe_the_service() {
        let service = service();
        let matching = Started {
            label: "surreal".into(),
            pid: 4242,
            port: 20_034,
            program: "surreal".into(),
        };
        assert!(describes(&matching, &service));

        let moved = Started {
            port: 20_035,
            ..matching.clone()
        };
        assert!(!describes(&moved, &service));

        let renamed = Started {
            program: "garage".into(),
            ..matching.clone()
        };
        assert!(!describes(&renamed, &service));

        let other = Started {
            label: "garage".into(),
            ..matching
        };
        assert!(!describes(&other, &service));
    }

    /// The failure message is the whole diagnostic when a dependency
    /// refuses to bind, so it must survive a log shorter than the tail
    /// it asks for, and must drop the blank lines these servers pad with.
    #[test]
    fn the_log_tail_is_the_last_lines_without_blanks() {
        assert_eq!(tail("a\nb\nc\nd\n", 2), "c\nd");
        assert_eq!(tail("only\n", 5), "only");
        assert_eq!(tail("", 5), "");
        assert_eq!(tail("a\n\n\nb\n", 5), "a\nb");
    }

    /// Both paths live under the worktree's gitignored `.devx/`, which is
    /// what keeps a native tier's data out of the tree and reclaimable by
    /// `worktree-env down`.
    #[test]
    fn every_state_path_stays_inside_the_worktrees_devx_directory() {
        let root = Path::new("/checkout");

        assert_eq!(
            ledger_path(root),
            PathBuf::from("/checkout/.devx/native-processes.json")
        );
        assert_eq!(
            service_dir(root, "garage"),
            PathBuf::from("/checkout/.devx/native/garage")
        );
    }
}
