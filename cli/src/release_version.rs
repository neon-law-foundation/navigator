//! `navigator ops release-version` — bump the workspace version to today's
//! `YY.M.D` release date so the commit a release tags carries the version its
//! Git tag names.
//!
//! The release is a deliberate tag push (`docs/gitops.md` → "One workflow owns
//! the release"). Nothing derives the version from the tag at build time unless
//! `NAVIGATOR_RELEASE_TAG` is set, so a plain build of the tagged source reports
//! the workspace crate version — which used to stay `0.1.0` forever, a binary
//! that misreports the release it was cut from. This command writes the release
//! date into `[workspace.package].version`, which every crate inherits through
//! `version.workspace = true` and `cli/build.rs` bakes into `navigator
//! --version`. Run it, land the bump on `main` through a PR, then tag that
//! merged commit; `deploy.yml`'s `release-version` job fails a tag whose source
//! version does not match, so the tag and the source can never drift.
//!
//! It never pushes to `main` itself: `main` is squash-merge-only and no ref may
//! be moved by automation (`docs/gitops.md` → "`main` is sacred"). The bump goes
//! through the ordinary PR flow like any other change.

use std::path::Path;
use std::process::ExitCode;

use chrono::{Datelike, Days, NaiveDate, Timelike, Utc};

/// Today's release version in the `YY.M.D` shape the tag glob and the
/// `deploy.yml` date guard require: the two-digit year and the UNPADDED month
/// and day, in UTC.
///
/// UTC, not local: it is the zone `YY.M.D` has always been derived in, it has no
/// DST discontinuity, and it is exactly what `deploy.yml` compares a pushed tag
/// against (`TZ=UTC date`). Deriving it in any other zone would let this command
/// write a version the release then rejects for being a day off.
fn todays_version() -> String {
    version_for(Utc::now().date_naive())
}

/// The `YY.M.D` string for one date, matching `deploy.yml`'s
/// `"$((10#$y)).$((10#$m)).$((10#$d))"` exactly: every component is base-10 with
/// no leading zero, so `2026-08-05` is `26.8.5`, not `26.08.05`.
fn version_for(date: NaiveDate) -> String {
    format!("{}.{}.{}", date.year() % 100, date.month(), date.day())
}

/// Today's hotfix version: a `-hotfix.N` prerelease hung off TOMORROW's
/// `YY.M.D`. The current UTC hour is a convenient default numeric `N`; the
/// grammar is not hour-bounded, and `--tag` accepts another discriminator.
///
/// This is the spelling for cutting a release when today's ordinary release
/// already happened — `YY.M.D` admits exactly one of those per UTC day, and the
/// tag is immutable, so the day's release name is spent the moment it is pushed.
fn todays_hotfix_version() -> String {
    let now = Utc::now();
    hotfix_version_for(now.date_naive(), now.hour())
}

/// The hotfix version for one UTC date and numeric discriminator.
///
/// THE BASE IS THE DAY AFTER `date`, and that is a correctness requirement
/// rather than a naming choice. Semver ranks a prerelease BELOW its own base
/// version (spec §11.3), so `26.8.17-hotfix.17` would sort as OLDER than the
/// `26.8.17` it exists to fix — Cargo, Homebrew, and every image sort would read
/// the fix as the earlier release. Hanging it off the next day makes the order
/// monotonic and true:
///
/// ```text
/// 26.8.17 < 26.8.18-hotfix.17 < 26.8.18-hotfix.21 < 26.8.18
/// ```
///
/// Read plainly, a hotfix IS the next day's release cut early: it carries fixes
/// that would otherwise wait for the next UTC day.
///
/// `number` is written unpadded because semver forbids a leading zero in a numeric
/// prerelease identifier — `hotfix.08` is not a valid version at all, which is
/// the same unpadded rule the date components already follow.
fn hotfix_version_for(date: NaiveDate, number: u32) -> String {
    // A date one day past the maximum representable date cannot arise from
    // `Utc::now()`; fall back to the same date rather than panicking, so this
    // helper has no failure mode a caller must handle.
    let base = date.checked_add_days(Days::new(1)).unwrap_or(date);
    format!("{}-hotfix.{number}", version_for(base))
}

/// Replace the `version` value inside the `[workspace.package]` table only,
/// leaving every dependency's own `version =` untouched. Returns the rewritten
/// manifest, or an error naming why the one line could not be found — a manifest
/// whose shape moved should fail loudly, not silently write nothing.
///
/// Scoped to the one table on purpose: `[workspace.dependencies]` holds dozens
/// of `version =` lines, and a blind find-and-replace would rewrite the first
/// dependency pin instead of the workspace version.
fn set_workspace_version(manifest: &str, version: &str) -> Result<String, String> {
    let mut out = String::with_capacity(manifest.len() + version.len());
    let mut in_package = false;
    let mut replaced = false;

    for line in manifest.lines() {
        let trimmed = line.trim_start();
        // A table header re-scopes every following key until the next header.
        if trimmed.starts_with('[') {
            in_package = trimmed.starts_with("[workspace.package]");
        }

        // Match the `version` KEY, not `rust-version` and not a comment: split
        // on the first `=` and compare the trimmed left side exactly.
        let is_version_key = in_package
            && !replaced
            && line
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "version");

        if is_version_key {
            out.push_str("version = \"");
            out.push_str(version);
            out.push_str("\"\n");
            replaced = true;
            continue;
        }

        out.push_str(line);
        out.push('\n');
    }

    if !replaced {
        return Err(
            "Cargo.toml has no `version` key under `[workspace.package]` — the manifest shape moved"
                .to_string(),
        );
    }
    Ok(out)
}

/// Entry point for `ops release-version`.
///
/// Writes the version and, unless `no_commit`, commits `Cargo.toml` on the
/// current branch so the operator can push it as a PR. It refuses to commit on
/// `main`: that branch takes no direct commits, so the bump must reach it the
/// same way every change does.
pub fn run(
    manifest_path: &Path,
    version: Option<String>,
    hotfix: bool,
    no_commit: bool,
) -> ExitCode {
    let version = match version {
        Some(explicit) if explicit.trim().is_empty() => {
            eprintln!("navigator: release-version: --tag must not be empty");
            return ExitCode::from(2);
        }
        Some(explicit) => explicit.trim().to_string(),
        None if hotfix => todays_hotfix_version(),
        None => todays_version(),
    };

    let manifest = match std::fs::read_to_string(manifest_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!(
                "navigator: release-version: read {}: {error}",
                manifest_path.display()
            );
            return ExitCode::from(2);
        }
    };

    let rewritten = match set_workspace_version(&manifest, &version) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("navigator: release-version: {error}");
            return ExitCode::from(2);
        }
    };

    if rewritten == manifest {
        println!(
            "navigator: {} already at version {version}",
            manifest_path.display()
        );
    } else if let Err(error) = std::fs::write(manifest_path, &rewritten) {
        eprintln!(
            "navigator: release-version: write {}: {error}",
            manifest_path.display()
        );
        return ExitCode::from(2);
    } else {
        println!(
            "navigator: set [workspace.package] version = {version} in {}",
            manifest_path.display()
        );
    }

    if no_commit {
        println!("navigator: --no-commit: staged nothing; commit and tag it yourself");
        return ExitCode::SUCCESS;
    }

    commit_bump(&version)
}

/// Commit the bump on the current branch, refusing `main`. The commit forms a
/// PR; the operator merges it and tags the merged commit.
fn commit_bump(version: &str) -> ExitCode {
    let branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output();
    if let Ok(output) = &branch {
        if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "main" {
            eprintln!(
                "navigator: release-version: refusing to commit on `main` — it takes no direct \
                 commits. The version is written; open a branch, commit Cargo.toml, and PR it, \
                 then tag the merged commit {version}."
            );
            return ExitCode::from(2);
        }
    }

    let staged = std::process::Command::new("git")
        .args(["add", "Cargo.toml"])
        .status();
    let committed = staged.is_ok_and(|status| status.success())
        && std::process::Command::new("git")
            .args(["commit", "-m", &format!("chore(release): {version}")])
            .status()
            .is_ok_and(|status| status.success());

    if committed {
        println!(
            "navigator: committed chore(release): {version}. Push it, open a PR, and after it \
             lands on main tag that commit:\n    git tag {version} && git push origin {version}"
        );
        ExitCode::SUCCESS
    } else {
        // Not fatal — the file is written. The operator can commit by hand.
        eprintln!(
            "navigator: release-version: could not create the commit (no git repo, or nothing \
             to commit). Cargo.toml is written; commit it yourself, then tag the merged commit \
             {version}."
        );
        ExitCode::from(2)
    }
}

#[cfg(test)]
mod tests {
    use super::{hotfix_version_for, set_workspace_version, version_for};
    use chrono::NaiveDate;

    /// The hotfix shape must match `deploy.yml`'s regex byte for byte: the base
    /// is TOMORROW's unpadded `YY.M.D` and the number is unpadded.
    #[test]
    fn hotfix_version_hangs_off_tomorrow_at_the_given_number() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 17).expect("valid date");
        assert_eq!(hotfix_version_for(date, 17), "26.8.18-hotfix.17");
    }

    /// THE PROPERTY THE WHOLE CONVENTION EXISTS FOR. A hotfix must sort ABOVE
    /// the release it fixes and BELOW the next ordinary release, and larger
    /// numbers must sort above smaller ones. Hanging the prerelease off the SAME day
    /// would invert the first comparison — semver ranks a prerelease below its
    /// own base — which is the bug this ordering test pins shut.
    #[test]
    fn hotfix_sorts_between_todays_release_and_tomorrows() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 17).expect("valid date");
        let today: semver::Version = version_for(date).parse().expect("valid semver");
        let early: semver::Version = hotfix_version_for(date, 3).parse().expect("valid semver");
        let late: semver::Version = hotfix_version_for(date, 21).parse().expect("valid semver");
        let tomorrow: semver::Version = version_for(
            date.checked_add_days(chrono::Days::new(1))
                .expect("valid date"),
        )
        .parse()
        .expect("valid semver");

        assert!(
            today < early,
            "a hotfix must rank above the release it fixes"
        );
        assert!(early < late, "a larger N must rank above a smaller one");
        assert!(
            late < tomorrow,
            "a hotfix must rank below the next ordinary release"
        );
    }

    /// An unpadded number is not cosmetic: semver forbids a leading zero in a
    /// numeric prerelease identifier, so a padded `hotfix.08` would not parse at
    /// all and `deploy.yml`'s regex rejects it.
    #[test]
    fn hotfix_number_is_unpadded_and_parses_as_semver() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 17).expect("valid date");
        let version = hotfix_version_for(date, 8);
        assert_eq!(version, "26.8.18-hotfix.8");
        assert!(version.parse::<semver::Version>().is_ok());
        assert!(
            "26.8.18-hotfix.08".parse::<semver::Version>().is_err(),
            "a leading zero in a numeric prerelease identifier is invalid semver"
        );
    }

    /// A hotfix cut on the last day of a month rolls into the next month, and on
    /// New Year's Eve into the next year — the base is a real date, not string
    /// arithmetic on the day component.
    #[test]
    fn hotfix_base_rolls_over_month_and_year() {
        let month_end = NaiveDate::from_ymd_opt(2026, 8, 31).expect("valid date");
        assert_eq!(hotfix_version_for(month_end, 5), "26.9.1-hotfix.5");
        let year_end = NaiveDate::from_ymd_opt(2026, 12, 31).expect("valid date");
        assert_eq!(hotfix_version_for(year_end, 23), "27.1.1-hotfix.23");
    }

    /// The shape must match `deploy.yml` byte for byte: unpadded month and day,
    /// two-digit year. A padded `26.08.05` would be a tag the release rejects.
    #[test]
    fn version_is_unpadded_yy_m_d() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 5).expect("valid date");
        assert_eq!(version_for(date), "26.8.5");
    }

    /// Two-digit day and month stay two digits; only the leading zero is
    /// dropped, never a significant digit.
    #[test]
    fn version_keeps_two_digit_components() {
        let date = NaiveDate::from_ymd_opt(2026, 12, 25).expect("valid date");
        assert_eq!(version_for(date), "26.12.25");
    }

    /// The one line under `[workspace.package]` is rewritten and nothing else.
    #[test]
    fn set_version_rewrites_the_workspace_package_version() {
        let manifest = "[workspace.package]\nversion = \"0.1.0\"\nedition = \"2021\"\n";
        let out = set_workspace_version(manifest, "26.8.14").expect("version present");
        assert!(out.contains("version = \"26.8.14\""));
        assert!(!out.contains("0.1.0"));
        assert!(
            out.contains("edition = \"2021\""),
            "other keys are untouched"
        );
    }

    /// The critical safety property: a dependency's own `version =` is NEVER
    /// touched, even though it appears before `[workspace.package]` and matches
    /// the same key name. A blind replace would pin the wrong thing.
    #[test]
    fn set_version_leaves_dependency_versions_untouched() {
        let manifest = "\
[workspace.dependencies]
serde = { version = \"1\" }
anyhow = \"1\"

[workspace.package]
version = \"0.1.0\"
rust-version = \"1.95\"
";
        let out = set_workspace_version(manifest, "26.8.14").expect("version present");
        assert!(
            out.contains("serde = { version = \"1\" }"),
            "the dependency pin must be preserved verbatim"
        );
        assert!(
            out.contains("version = \"26.8.14\""),
            "the workspace version is bumped"
        );
        assert!(
            out.contains("rust-version = \"1.95\""),
            "rust-version is a different key and must not be mistaken for `version`"
        );
    }

    /// A manifest whose `[workspace.package]` has no `version` fails loudly
    /// rather than writing an unchanged file and reporting success.
    #[test]
    fn set_version_errors_when_the_key_is_absent() {
        let manifest = "[workspace.package]\nedition = \"2021\"\n";
        assert!(set_workspace_version(manifest, "26.8.14").is_err());
    }
}
