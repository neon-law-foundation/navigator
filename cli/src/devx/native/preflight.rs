//! Host toolchain preflight for the native dependency tier.
//!
//! The native lane runs every dependency as a host process instead of a
//! pod, so the binaries have to exist on the machine before anything can
//! start. This module is that gate: it refuses to run anywhere but
//! macOS, locates Homebrew, and installs the pinned formulas.
//!
//! Two properties are worth naming because they are what make the lane
//! trustworthy rather than merely fast:
//!
//! - **Version parity with the KIND lane is asserted, not hoped for.**
//!   Each [`Formula`] carries the `manifest` whose container image it
//!   replaces, and a test reads that manifest and compares tags. A
//!   version bump that touches only one lane fails the suite instead of
//!   producing two silently different local environments.
//! - **Presence is not the check; version is.** A formula that is
//!   installed but older than the image pin would run a different engine
//!   than the cluster does, which is exactly the class of difference
//!   that costs an afternoon. [`stale`] treats it as missing.
//!
//! Rauthy is deliberately absent from [`REQUIRED`]: it has no Homebrew
//! formula, so it is acquired by pinned download-and-cache the way
//! `super::super::chrome` acquires Chrome for Testing. `OpenObserve` is
//! absent for a different reason — telemetry export is opt-in (an unset
//! `OTEL_EXPORTER_OTLP_ENDPOINT` leaves `telemetry` in local-only mode),
//! so the native tier does not run it unless asked.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// A Homebrew formula the native tier needs, and the KIND manifest whose
/// image it stands in for.
///
/// `pin` is the version the cluster runs. It is a floor rather than an
/// equality at runtime: Homebrew moves faster than the vendored
/// manifests, and refusing to start because brew is one patch ahead
/// would make the lane unusable. The equality that *is* enforced lives
/// in the tests, against `manifest`.
pub(super) struct Formula {
    /// Tap to add before installing, when the formula is not in core.
    pub(super) tap: Option<&'static str>,
    /// Formula name as `brew install` spells it.
    pub(super) name: &'static str,
    /// Version the KIND lane pins, and the floor for the host install.
    pub(super) pin: &'static str,
    /// Manifest carrying the container image this formula replaces.
    /// `None` for tooling with no in-cluster counterpart.
    ///
    /// Read only by the parity test — that is the field's whole job.
    /// Declaring it here rather than in a test-side table is deliberate:
    /// a new formula cannot be added without stating what it replaces,
    /// so the two lanes cannot drift apart unnoticed.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) manifest: Option<&'static str>,
}

impl Formula {
    /// What `brew install` is called with — tap-qualified when the
    /// formula lives outside core, so the install is unambiguous even if
    /// a same-named core formula appears later.
    pub(super) fn install_target(&self) -> String {
        match self.tap {
            Some(tap) => format!("{tap}/{}", self.name),
            None => self.name.to_string(),
        }
    }
}

/// Every formula the native tier installs. All of them are Rust, which
/// is the whole point of the lane.
pub(super) const REQUIRED: &[Formula] = &[
    Formula {
        tap: Some("surrealdb/tap"),
        name: "surreal",
        pin: "3.2.3",
        manifest: Some("k8s/overlays/kind/surreal/surreal.yaml"),
    },
    Formula {
        tap: Some("restatedev/tap"),
        name: "restate-server",
        pin: "1.7.2",
        manifest: Some("k8s/staging/restate.yaml"),
    },
    // The CLI is how `dev` registers the worker's endpoint with the
    // server; it has no image because nothing in-cluster runs it.
    Formula {
        tap: Some("restatedev/tap"),
        name: "restate",
        pin: "1.7.2",
        manifest: None,
    },
    Formula {
        tap: None,
        name: "garage",
        pin: "2.3.0",
        manifest: Some("k8s/overlays/kind/garage/garage.yaml"),
    },
];

/// Refuse to run anywhere but macOS.
///
/// The native tier is macOS-only by decision, not by accident: it leans
/// on Homebrew for acquisition and on a single well-known process model
/// for supervision. Failing loudly here beats a partial start on a host
/// this lane has never been exercised on — the KIND lane stays available
/// everywhere, so the fallback is real.
pub(super) fn require_macos(os: &str) -> Result<()> {
    if os == "macos" {
        return Ok(());
    }
    bail!(
        "the native dependency tier is macOS-only (this host reports `{os}`).\n\
         Use the cluster lane instead: \
         `navigator dev worktree-env up --path \"$PWD\" --runtime kind`"
    )
}

/// Install every dependency the native tier needs, converging on the
/// pinned versions.
///
/// Idempotent, so it is safe to run before every `up`: a host that is
/// already converged does one `brew list` and stops. That cheap
/// re-check is what lets the second `worktree-env up` connect to running
/// processes instead of provisioning anything.
pub(super) fn ensure(os: &str) -> Result<()> {
    require_macos(os)?;
    let prefix = require_homebrew()?;
    eprintln!("==> Homebrew at {prefix}");

    let listing = brew(&["list", "--formula", "--versions"])?;
    let installed = parse_installed(&listing);
    let work = stale(&installed, REQUIRED);

    if work.is_empty() {
        eprintln!("==> every native dependency is installed at or above its pin");
        return Ok(());
    }

    for tap in taps_for(&work) {
        eprintln!("==> tapping {tap}");
        brew(&["tap", tap])?;
    }
    for formula in work {
        let target = formula.install_target();
        eprintln!("==> installing {target} (pin {})", formula.pin);
        brew(&["install", &target])?;
    }
    Ok(())
}

/// Absolute path to an executable inside an installed formula.
///
/// Resolved through `brew --prefix` rather than `PATH`: a keg-only
/// formula is never linked into the prefix, so a `PATH` lookup finds
/// either nothing or some unrelated system binary, and a `PATH` that
/// changes between `install` and `up` would otherwise swap the binary
/// under a tier that is already running.
pub(super) fn binary(formula: &str, name: &str) -> Result<PathBuf> {
    let prefix = brew(&["--prefix", formula])?;
    let path = binary_path(prefix.trim(), name);
    if !path.is_file() {
        bail!(
            "{} is not installed at {} — run `navigator dev install`",
            formula,
            path.display()
        );
    }
    Ok(path)
}

/// Pure half of [`binary`]: where Homebrew puts a formula's executables.
fn binary_path(prefix: &str, name: &str) -> PathBuf {
    Path::new(prefix).join("bin").join(name)
}

/// Run a `brew` subcommand, surfacing its stderr on failure.
fn brew(args: &[&str]) -> Result<String> {
    let output = Command::new("brew")
        .args(args)
        .output()
        .with_context(|| format!("run `brew {}`", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "`brew {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Locate Homebrew, returning its prefix.
fn require_homebrew() -> Result<String> {
    let output = Command::new("brew").arg("--prefix").output().context(
        "Homebrew is required by the native dependency tier but `brew` is not on PATH.\n\
             Install it from https://brew.sh, then re-run this command.",
    )?;
    if !output.status.success() {
        bail!(
            "`brew --prefix` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Parse `brew list --formula --versions` into name → newest version.
///
/// Brew prints one formula per line as `name ver [ver...]`, newest last
/// when several are kegged. Lines without a version are ignored rather
/// than treated as version-zero, so a malformed line degrades to "not
/// installed" and gets reinstalled instead of failing the run.
pub(super) fn parse_installed(listing: &str) -> BTreeMap<String, String> {
    listing
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let version = parts.last()?;
            Some((name.to_string(), version.to_string()))
        })
        .collect()
}

/// Which formulas need installing or upgrading.
///
/// Absent and below-pin are the same outcome on purpose — see the module
/// docs. The caller runs `brew install` for both; brew upgrades in place
/// when the formula is already present.
pub(super) fn stale<'a>(
    installed: &BTreeMap<String, String>,
    required: &'a [Formula],
) -> Vec<&'a Formula> {
    required
        .iter()
        .filter(|formula| match installed.get(formula.name) {
            None => true,
            Some(version) => below(version, formula.pin),
        })
        .collect()
}

/// Whether `have` is older than `want`, comparing dotted numeric
/// segments.
///
/// Non-numeric segments compare as zero, and a shorter version is padded
/// with zeros, so `16` and `16.14` order correctly against a `16` pin.
fn below(have: &str, want: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split(['.', '_', '-'])
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (have, want) = (parse(have), parse(want));
    let width = have.len().max(want.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    for i in 0..width {
        match at(&have, i).cmp(&at(&want, i)) {
            std::cmp::Ordering::Less => return true,
            std::cmp::Ordering::Greater => return false,
            std::cmp::Ordering::Equal => {}
        }
    }
    false
}

/// Taps needed by a set of formulas, deduplicated and ordered.
pub(super) fn taps_for(formulas: &[&Formula]) -> Vec<&'static str> {
    let mut taps: Vec<&'static str> = formulas.iter().filter_map(|f| f.tap).collect();
    taps.sort_unstable();
    taps.dedup();
    taps
}

#[cfg(test)]
mod tests {
    use super::{
        below, binary_path, parse_installed, require_macos, stale, taps_for, Formula, REQUIRED,
    };
    use std::path::{Path, PathBuf};

    /// Resolution goes through the formula's own prefix rather than the
    /// linked Homebrew prefix, so a formula whose binary never links into
    /// `bin` is still found — and so is one whose executable is named
    /// differently from the formula.
    #[test]
    fn a_formula_executable_resolves_under_its_own_prefix() {
        assert_eq!(
            binary_path("/opt/homebrew/opt/garage", "garage"),
            PathBuf::from("/opt/homebrew/opt/garage/bin/garage")
        );
        assert_eq!(
            binary_path("/opt/homebrew/opt/restate-server", "restate-server"),
            PathBuf::from("/opt/homebrew/opt/restate-server/bin/restate-server")
        );
    }

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root is cli/'s parent")
    }

    /// The whole point of the two-lane design is that both lanes run the
    /// same engines. A formula pin that drifts from the image tag it
    /// replaces breaks that quietly — the cluster developer and the
    /// native developer would be debugging different software — so the
    /// coupling is asserted here rather than discovered.
    #[test]
    fn every_formula_pin_matches_the_image_tag_it_replaces() {
        for formula in REQUIRED {
            let Some(manifest) = formula.manifest else {
                continue;
            };
            let body = std::fs::read_to_string(repo_root().join(manifest))
                .unwrap_or_else(|e| panic!("read {manifest}: {e}"));
            let image = body
                .lines()
                .find(|line| line.trim_start().starts_with("image:"))
                .unwrap_or_else(|| panic!("{manifest} declares no image"));
            assert!(
                image.contains(formula.pin),
                "{} pins {} but {manifest} runs `{}`",
                formula.name,
                formula.pin,
                image.trim()
            );
        }
    }

    /// A tap-qualified target keeps the install unambiguous: `surreal`
    /// alone would resolve to core if a same-named formula ever landed
    /// there, silently installing different software.
    #[test]
    fn install_targets_are_tap_qualified_outside_core() {
        let tapped = Formula {
            tap: Some("surrealdb/tap"),
            name: "surreal",
            pin: "3.2.3",
            manifest: None,
        };
        let core = Formula {
            tap: None,
            name: "garage",
            pin: "2.3.0",
            manifest: None,
        };

        assert_eq!(tapped.install_target(), "surrealdb/tap/surreal");
        assert_eq!(core.install_target(), "garage");
    }

    #[test]
    fn a_non_macos_host_is_refused_and_pointed_at_the_cluster_lane() {
        let err = require_macos("linux").expect_err("linux must be refused");
        let message = err.to_string();

        assert!(message.contains("macOS-only"), "{message}");
        assert!(message.contains("linux"), "{message}");
        assert!(message.contains("--runtime kind"), "{message}");
    }

    #[test]
    fn macos_is_accepted() {
        require_macos("macos").expect("macOS is the supported host");
    }

    /// Brew lists every kegged version of a formula on one line, oldest
    /// first, so the last field is the one that would actually run.
    #[test]
    fn brew_listing_parses_to_the_newest_kegged_version() {
        let installed = parse_installed(
            "garage 2.2.9 2.3.0\n\
             restate-server 1.7.2\n\
             surreal 3.2.3\n",
        );

        assert_eq!(installed.get("garage").map(String::as_str), Some("2.3.0"));
        assert_eq!(
            installed.get("restate-server").map(String::as_str),
            Some("1.7.2")
        );
        assert_eq!(installed.len(), 3);
    }

    /// An installed-but-old formula is the dangerous case: it satisfies
    /// a presence probe while running a different engine than the
    /// cluster. It has to come back as work to do.
    #[test]
    fn an_installed_formula_below_the_pin_counts_as_stale() {
        let required = &[Formula {
            tap: None,
            name: "garage",
            pin: "2.3.0",
            manifest: None,
        }];

        let old = parse_installed("garage 2.2.9\n");
        assert_eq!(stale(&old, required).len(), 1);

        let exact = parse_installed("garage 2.3.0\n");
        assert!(stale(&exact, required).is_empty());

        // Brew running ahead of the vendored manifest must not block the
        // loop — the pin is a floor, and the tests above hold parity.
        let ahead = parse_installed("garage 2.4.0\n");
        assert!(stale(&ahead, required).is_empty());

        let absent = parse_installed("");
        assert_eq!(stale(&absent, required).len(), 1);
    }

    /// A formula that reports more version components than its pin —
    /// `3.2.3` against a `3.2` pin — must not read as a downgrade, or the
    /// loop reinstalls on every single `up`. Padding the shorter side with
    /// zeros is what keeps that straight.
    #[test]
    fn a_longer_version_satisfies_a_shorter_pin() {
        assert!(!below("3.2.3", "3.2"));
        assert!(below("3.1.9", "3.2"));
        assert!(!below("1.7.2", "1.7.2"));
        assert!(below("1.7.1", "1.7.2"));
    }

    #[test]
    fn taps_are_deduplicated_across_the_formulas_that_need_them() {
        let formulas: Vec<&Formula> = REQUIRED.iter().collect();

        assert_eq!(taps_for(&formulas), vec!["restatedev/tap", "surrealdb/tap"]);
    }

    /// Rauthy has no Homebrew formula, so it must not appear here — it
    /// is acquired by pinned download-and-cache instead. `OpenObserve` is
    /// excluded because telemetry export is opt-in. Both exclusions are
    /// load-bearing decisions, so they get a guard.
    #[test]
    fn the_formula_list_excludes_the_dependencies_brew_cannot_provide() {
        let names: Vec<&str> = REQUIRED.iter().map(|f| f.name).collect();

        assert!(!names.contains(&"rauthy"), "{names:?}");
        assert!(!names.contains(&"openobserve"), "{names:?}");
    }
}
