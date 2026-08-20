//! `navigator ops release-version` — write the release version the operator
//! names into `[workspace.package].version`, so the commit a release tags
//! carries the version its Git tag names.
//!
//! IT DERIVES NOTHING. `--tag` is required and its value is written verbatim
//! once the shape checks out. Naming a release is an operator decision — whether
//! a cut is ordinary or a hotfix, and which `N` a hotfix carries — and a command
//! that guessed made the name a side effect of when it happened to run.
//! `deploy.yml`'s `release-version` job is the authority on which names are
//! admissible; this command refuses the ones that job would refuse, here, while
//! nothing has been published and the name is still free.
//!
//! The release is a deliberate tag push (`docs/gitops.md` → "One workflow owns
//! the release"). Nothing derives the version from the tag at build time unless
//! `NAVIGATOR_RELEASE_TAG` is set, so a plain build of the tagged source reports
//! the workspace crate version — which used to stay `0.1.0` forever, a binary
//! that misreports the release it was cut from. Every crate inherits this value
//! through `version.workspace = true` and `cli/build.rs` bakes it into
//! `navigator --version`. Run it, land the bump on `main` through a PR, then tag
//! that merged commit; `deploy.yml` fails a tag whose source version disagrees,
//! so the tag and the source can never drift.
//!
//! It writes `Cargo.lock` alongside the manifest, because the release builds
//! with `--locked` and that flag refuses a lock whose versions the manifest has
//! moved past. A bump carrying only `Cargo.toml` fails after the tag is pushed,
//! and a tag cannot be moved.
//!
//! It never pushes to `main` itself: `main` is squash-merge-only and no ref may
//! be moved by automation (`docs/gitops.md` → "`main` is sacred"). The bump goes
//! through the ordinary PR flow like any other change.

use std::path::Path;
use std::process::ExitCode;

/// Accept exactly the release versions `deploy.yml`'s `release-version` job
/// accepts, and reject everything else before a byte is written.
///
/// This check REPLACES a derivation rather than adding a new rule. While this
/// command computed the version itself, a well-formed name was a property of the
/// code; now that the operator supplies every name it is an input, and an
/// unchecked input would write a version Cargo cannot parse into the manifest —
/// to be discovered by the release, on a tag that cannot be moved.
///
/// The grammar is `deploy.yml`'s, transcribed: a two-digit year, an unpadded
/// month and day, and an optional `-hotfix.N` prerelease whose `N` is unpadded.
/// Padding is not cosmetic — semver forbids a leading zero in a numeric
/// prerelease identifier, so `hotfix.08` is not a version at all.
///
/// IT DOES NOT READ THE CLOCK. Whether a base is today's or tomorrow's UTC date
/// is a question about *when* this runs, and answering it here would restore, in
/// a different shape, exactly the guessing this command stopped doing. The two
/// places that already own that check keep it: `deploy.yml`'s date guard, and
/// `cut-release`'s `validate-release-tag.sh` before anything is written.
fn validate_release_version(version: &str) -> Result<(), String> {
    let (base, prerelease) = match version.split_once('-') {
        Some((base, rest)) => (base, Some(rest)),
        None => (version, None),
    };

    let mut components = base.split('.');
    let (Some(year), Some(month), Some(day), None) = (
        components.next(),
        components.next(),
        components.next(),
        components.next(),
    ) else {
        return Err(format!(
            "`{version}` is not a `YY.M.D` release version: its base needs exactly three              dot-separated components"
        ));
    };

    if year.len() != 2 || !year.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!(
            "`{version}` must open with a two-digit year, not `{year}`"
        ));
    }

    for (label, component) in [("month", month), ("day", day)] {
        if !is_unpadded_number(component, Some(2)) {
            return Err(format!(
                "`{version}` has an invalid {label} `{component}`: it must be an unpadded number                  (August is `8`, never `08`)"
            ));
        }
    }

    if let Some(prerelease) = prerelease {
        let Some(number) = prerelease.strip_prefix("hotfix.") else {
            return Err(format!(
                "`{version}`: the only prerelease a release may carry is `-hotfix.N`, not                  `-{prerelease}`"
            ));
        };
        if !is_unpadded_number(number, None) {
            return Err(format!(
                "`{version}` has an invalid hotfix number `{number}`: semver forbids a leading                  zero in a numeric prerelease identifier, so a padded one is not a version at all"
            ));
        }
    }

    Ok(())
}

/// An unpadded nonnegative integer: `0` itself, or digits that do not open with
/// a zero. `max_digits` bounds the length where the grammar does — a month or a
/// day admits at most two digits, while a hotfix `N` is unbounded.
fn is_unpadded_number(text: &str, max_digits: Option<usize>) -> bool {
    !text.is_empty()
        && max_digits.is_none_or(|max| text.len() <= max)
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'))
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
/// Writes the version the operator named, refreshes `Cargo.lock` to match, and
/// unless `no_commit` commits both on the current branch so the operator can
/// push them as a PR. It refuses to commit on `main`: that branch takes no
/// direct commits, so the bump must reach it the same way every change does.
///
/// `version` is required and never defaulted. The shape is checked first, so a
/// name the release would refuse fails here instead of after a tag exists.
pub fn run(manifest_path: &Path, version: &str, no_commit: bool) -> ExitCode {
    let version = version.trim().to_string();
    if version.is_empty() {
        eprintln!("navigator: release-version: --tag must not be empty");
        return ExitCode::from(2);
    }
    if let Err(error) = validate_release_version(&version) {
        eprintln!("navigator: release-version: {error}");
        return ExitCode::from(2);
    }

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

    // `Cargo.lock` pins every workspace crate's version too, and `deploy.yml`
    // builds the release with `--locked` — in the provenance step and in all
    // three CLI archive jobs. `--locked` refuses a lock the manifest has moved
    // past, so writing one file without the other is latently fatal rather than
    // untidy: the failure lands AFTER the tag is pushed, the `release-tags`
    // ruleset admits no bypass actor, and the day's release name is spent. The
    // archive jobs are also what `.github/actions/validate` downloads, so the
    // breakage surfaces as a 404 in every Project repository's CI while nothing
    // here goes red. Refresh the lock in the same breath as the manifest.
    //
    // Unconditionally, not only when the manifest changed: a manifest already at
    // the target version beside a lock that never caught up is exactly the state
    // this repairs.
    let lockfile = manifest_path.with_file_name("Cargo.lock");
    let lock_present = lockfile.exists();
    if lock_present {
        if let Err(error) = refresh_lockfile(manifest_path) {
            eprintln!(
                "navigator: release-version: could not refresh {}: {error}",
                lockfile.display()
            );
            return ExitCode::from(2);
        }
        println!("navigator: refreshed {} to {version}", lockfile.display());
    }

    if no_commit {
        println!("navigator: --no-commit: staged nothing; commit and tag it yourself");
        return ExitCode::SUCCESS;
    }

    commit_bump(&version, lock_present)
}

/// Refresh `Cargo.lock` so every workspace crate's locked version equals the one
/// just written to `[workspace.package]`.
///
/// `cargo update --workspace` is the narrow spelling: it re-resolves the
/// workspace members only, so a release cut can never move a third-party pin as
/// a side effect of writing a date.
fn refresh_lockfile(manifest_path: &Path) -> Result<(), String> {
    // Offline is the honest first attempt: only the members' own version strings
    // moved, and that needs no registry data. A lock stale for some other reason
    // — a dependency added since it was written — does need the index, so fall
    // back to an online resolve rather than failing on the flag.
    match cargo_update(manifest_path, true) {
        Ok(()) => Ok(()),
        Err(offline) => cargo_update(manifest_path, false)
            .map_err(|online| format!("{online} (offline attempt: {offline})")),
    }
}

/// One `cargo update --workspace` invocation, surfacing cargo's own stderr as the
/// error so a failure explains itself.
fn cargo_update(manifest_path: &Path, offline: bool) -> Result<(), String> {
    // `CARGO` is set whenever cargo launched this process, which is how a release
    // runs it; using it pins the nested call to the same toolchain.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = std::process::Command::new(cargo);
    command.args(["update", "--workspace", "--quiet"]);
    if offline {
        command.arg("--offline");
    }
    command.arg("--manifest-path").arg(manifest_path);

    match command.output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        Err(error) => Err(format!("could not run `cargo update`: {error}")),
    }
}

/// Commit the bump on the current branch, refusing `main`. The commit carries the
/// manifest and the refreshed lock together; it forms a PR, and the operator
/// merges it and tags the merged commit.
fn commit_bump(version: &str, lock_present: bool) -> ExitCode {
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

    // Both files or neither: the release builds with `--locked`, so a commit
    // carrying the manifest alone names a version its own lock refuses to build.
    let mut paths = vec!["Cargo.toml"];
    if lock_present {
        paths.push("Cargo.lock");
    }
    let staged = std::process::Command::new("git")
        .arg("add")
        .args(&paths)
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
    use super::{set_workspace_version, validate_release_version};

    /// An ordinary release version is accepted as written.
    #[test]
    fn accepts_an_unpadded_yy_m_d_version() {
        assert!(validate_release_version("26.8.5").is_ok());
        assert!(validate_release_version("26.12.25").is_ok());
    }

    /// A hotfix prerelease is accepted, and `N` is not hour-bounded — it is a
    /// uniqueness-and-ordering discriminator the operator chooses, so a value
    /// past 23 is as valid as any other.
    #[test]
    fn accepts_a_hotfix_prerelease_with_any_unpadded_number() {
        assert!(validate_release_version("26.8.18-hotfix.3").is_ok());
        assert!(validate_release_version("26.8.18-hotfix.0").is_ok());
        assert!(validate_release_version("26.8.18-hotfix.99").is_ok());
    }

    /// THE PROPERTY THE WHOLE CONVENTION EXISTS FOR, asserted against a real
    /// semver implementation on the literal spellings this validator admits: a
    /// hotfix must sort ABOVE the release it fixes and BELOW the next ordinary
    /// release, and a larger `N` above a smaller one. Hanging the prerelease off
    /// the SAME day would invert the first comparison, because semver ranks a
    /// prerelease below its own base — which is why a hotfix names TOMORROW.
    #[test]
    fn a_hotfix_sorts_between_the_release_it_fixes_and_the_next_one() {
        let names = [
            "26.8.17",
            "26.8.18-hotfix.3",
            "26.8.18-hotfix.21",
            "26.8.18",
        ];
        for name in names {
            assert!(
                validate_release_version(name).is_ok(),
                "{name} must be an admissible release version"
            );
        }

        let parsed: Vec<semver::Version> = names
            .iter()
            .map(|name| name.parse().expect("valid semver"))
            .collect();
        for pair in parsed.windows(2) {
            assert!(pair[0] < pair[1], "{} must sort below {}", pair[0], pair[1]);
        }
    }

    /// A padded component is not cosmetic: `deploy.yml` anchors the unpadded
    /// shape, so `26.08.20` is a tag the release refuses.
    #[test]
    fn rejects_padded_month_and_day() {
        assert!(validate_release_version("26.08.20").is_err());
        assert!(validate_release_version("26.8.05").is_err());
    }

    /// A fourth component is impossible by construction — Cargo parses this
    /// value as strict semver and rejects it — so it must never reach the
    /// manifest.
    #[test]
    fn rejects_a_fourth_component() {
        assert!(validate_release_version("26.8.20.13").is_err());
        assert!(validate_release_version("26.8").is_err());
    }

    /// Semver forbids a leading zero in a numeric prerelease identifier, so a
    /// padded `hotfix.08` is not a version at all — and would not parse.
    #[test]
    fn rejects_a_padded_hotfix_number() {
        assert!(validate_release_version("26.8.18-hotfix.08").is_err());
        assert!(
            "26.8.18-hotfix.08".parse::<semver::Version>().is_err(),
            "the rejection is semver's rule, not a local preference"
        );
    }

    /// `-hotfix.N` is the only prerelease a release may carry: the publish path
    /// and the Homebrew tap both reason about that one spelling.
    #[test]
    fn rejects_any_other_prerelease() {
        assert!(validate_release_version("26.8.18-rc.1").is_err());
        assert!(validate_release_version("26.8.18-hotfix").is_err());
        assert!(validate_release_version("26.8.18-hotfix.x").is_err());
    }

    /// A four-digit year, a `v` prefix, and empty input are all refused rather
    /// than written into the manifest.
    #[test]
    fn rejects_a_malformed_year_or_prefix() {
        assert!(validate_release_version("2026.8.20").is_err());
        assert!(validate_release_version("v26.8.20").is_err());
        assert!(validate_release_version("").is_err());
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
