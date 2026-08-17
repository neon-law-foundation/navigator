//! Which local dependency tier a worktree runs on.
//!
//! Two lanes serve the same contract. The native lane runs each
//! dependency as a host process, sharing most of them across worktrees
//! through a tenant key; the cluster lane gives each worktree its own
//! KIND cluster. They differ in topology and in nothing else that a
//! caller can observe: both write the same `.devx/env` and the same
//! `.devx/worktree.json`, so `dev grant-lawyer`, `dev browser-e2e`, and
//! `cargo run -p neon` behave identically under either.
//!
//! The env file — not the topology — is the contract. Keeping that true
//! is what makes the second lane cheap to carry rather than a fork of
//! the whole loop.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The dependency tier backing one worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum Runtime {
    /// Host processes, most of them shared across worktrees. macOS only.
    #[default]
    Native,
    /// A KIND cluster per worktree. Slower and heavier, but it is the
    /// topology the deployments run, so it stays a supported choice
    /// rather than a deprecation — `--demo` needs its ingress, and it is
    /// the only lane on a non-macOS host.
    Kind,
}

impl Runtime {
    /// The value an existing descriptor is read as when it predates this
    /// field.
    ///
    /// Every `.devx/worktree.json` written before the native lane
    /// existed describes a KIND cluster, so this is what those files
    /// actually mean — not a compatibility shim but the correct reading
    /// of on-disk state. Without it, `down` would consult the wrong tier
    /// and leave a live cluster bound to its ports.
    pub(super) fn of_existing_descriptor() -> Self {
        Self::Kind
    }

    /// How this lane names itself in `status` output and errors.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Kind => "kind",
        }
    }
}

impl fmt::Display for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::Runtime;

    /// Native is the default lane. The cluster is opt-in.
    #[test]
    fn the_default_lane_is_native() {
        assert_eq!(Runtime::default(), Runtime::Native);
    }

    /// A descriptor written before the field existed describes a
    /// cluster. Reading it as native would send `down` to the wrong
    /// tier and leak the cluster it was supposed to reclaim.
    #[test]
    fn a_descriptor_without_the_field_reads_as_the_cluster_lane() {
        assert_eq!(Runtime::of_existing_descriptor(), Runtime::Kind);
    }

    /// The descriptor is JSON on disk that both lanes read, so the
    /// serialized spelling is a contract, not an implementation detail.
    #[test]
    fn the_serialized_spelling_is_stable_and_lowercase() {
        assert_eq!(
            serde_json::to_string(&Runtime::Native).expect("serialize"),
            "\"native\""
        );
        assert_eq!(
            serde_json::to_string(&Runtime::Kind).expect("serialize"),
            "\"kind\""
        );
        assert_eq!(
            serde_json::from_str::<Runtime>("\"kind\"").expect("deserialize"),
            Runtime::Kind
        );
    }

    #[test]
    fn each_lane_labels_itself_for_status_and_errors() {
        assert_eq!(Runtime::Native.to_string(), "native");
        assert_eq!(Runtime::Kind.to_string(), "kind");
    }
}
