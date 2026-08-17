//! Guard the three CLI release artifacts — the Linux archive CI installs, and
//! the Windows and macOS archives humans download — and the install commands
//! advertised in the successful-release Slack message.

use std::fs;
use std::path::PathBuf;

fn deploy_workflow() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github")
        .join("workflows")
        .join("deploy.yml");
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn deploy_job(name: &str) -> serde_yaml::Value {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&deploy_workflow()).expect("deploy.yml parses as YAML");
    workflow
        .get("jobs")
        .and_then(|jobs| jobs.get(name))
        .cloned()
        .unwrap_or_else(|| panic!("deploy.yml must define the `{name}` job"))
}

fn job_needs(name: &str) -> Vec<String> {
    let job = deploy_job(name);
    serde_yaml::from_value(job["needs"].clone())
        .unwrap_or_else(|error| panic!("`{name}` must declare a `needs` list: {error}"))
}

/// The composite gate every Project repository runs. It downloads the archive
/// `deploy.yml` publishes, and the two files never reference each other — so
/// the asset name is a contract only a test can hold.
fn validate_action() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github")
        .join("actions")
        .join("validate")
        .join("action.yml");
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

#[test]
fn releases_build_and_attach_a_windows_cli_archive() {
    let workflow = deploy_workflow();

    for required in [
        "release-windows-cli-build:",
        "runs-on: windows-latest",
        "NAVIGATOR_RELEASE_TAG: ${{ needs.release-version.outputs.tag }}",
        "Copy-Item \"target/release/navigator.exe\"",
        "Copy-Item \"LICENSE.md\"",
        "Copy-Item \"LICENSE-MIT\"",
        "Copy-Item \"LICENSE-APACHE\"",
        "Compress-Archive -Path \"dist/navigator-windows/*\"",
        "release-windows-cli-publish:",
        "gh release create \"${TAG}\"",
        "gh release upload \"${TAG}\" dist/navigator-*-windows.zip",
    ] {
        assert!(
            workflow.contains(required),
            "deploy.yml must retain the Windows CLI release contract `{required}`"
        );
    }
}

/// Every release that publishes images also builds the Windows CLI. The job
/// carries no `if:` of its own: `needs` alone decides when it runs, so a
/// dispatch release can no longer publish images while quietly shipping no
/// CLI for the people who run Navigator on Windows.
#[test]
fn every_published_release_builds_the_windows_cli() {
    let build = deploy_job("release-windows-cli-build");

    assert!(
        build.get("if").is_none(),
        "release-windows-cli-build must carry no `if:` gate — `needs` decides when it runs"
    );

    let needs = job_needs("release-windows-cli-build");
    for required in ["publish-service", "publish-triggers", "release-version"] {
        assert!(
            needs.iter().any(|entry| entry == required),
            "release-windows-cli-build must start with ship-staging, so it needs `{required}`"
        );
    }
}

/// Every run that reaches this job is a tag release, so the archive is built
/// from the commit its tag will name.
///
/// There is no tag to check out: `release-windows-cli-publish` CUTS the tag,
/// at this same SHA, after both archives are built. So the archive and the tag
/// naming it describe one commit rather than two that happened to be close —
/// which is the property the old `ref: <tag>` was protecting, held from the
/// other end.
#[test]
fn the_windows_build_checks_out_the_commit_it_claims() {
    assert_builds_from_the_sha("release-windows-cli-build");
}

/// The ref must be the run's SHA and nothing else. Shared by both CLI builds
/// because the requirement is identical: the archive's name carries the version,
/// so its bytes must come from the commit that version is cut at.
fn assert_builds_from_the_sha(job: &str) {
    let build = deploy_job(job);
    let steps = build["steps"]
        .as_sequence()
        .unwrap_or_else(|| panic!("{job} must declare steps"));
    let checkout = steps
        .iter()
        .find(|step| {
            step.get("uses")
                .and_then(serde_yaml::Value::as_str)
                .is_some_and(|uses| uses.starts_with("actions/checkout@"))
        })
        .unwrap_or_else(|| panic!("{job} must check the tree out"));
    let git_ref = checkout["with"]["ref"]
        .as_str()
        .expect("the checkout must pin a ref");

    assert!(
        !git_ref.contains('\n'),
        "the ref expression must be ONE line, got: {git_ref:?}"
    );
    assert_eq!(
        git_ref.trim(),
        "${{ github.sha }}",
        "{job} must build the run's own SHA — on a tag push that IS the tagged commit, and the \
         Release these archives attach to hangs off that same tag, so checking out anything else \
         would let an archive named for a version be compiled from a different commit"
    );
}

/// A GitHub Release hangs off an immutable Git tag, and `publishable` is true
/// only for a validated tag ref, so that one output is the whole gate. The
/// trigger-shaped clauses this carried existed to stop a clock- or
/// dispatch-driven run claiming a Release no tag stood behind; neither trigger
/// exists now, and naming a retired one here would be a gate on a condition that
/// can never be true.
#[test]
fn only_tagged_releases_attach_the_archive_to_a_github_release() {
    let gate = deploy_job("release-windows-cli-publish")["if"]
        .as_str()
        .expect("release-windows-cli-publish must declare an `if:` gate")
        .to_string();

    assert!(
        gate.contains("needs.release-version.outputs.publishable == 'true'"),
        "attaching to a GitHub Release must stay gated on a publishable run, got: {gate:?}"
    );
    for retired in ["github.event_name == 'schedule'", "workflow_dispatch"] {
        assert!(
            !gate.contains(retired),
            "the gate must not name the retired trigger `{retired}` — a release is a tag push"
        );
    }
}

/// #navigator's install message offers a download per platform, and keeps the
/// build-from-source path for the one Mac no archive covers.
///
/// The source build used to be the *only* macOS instruction, because there was
/// no macOS archive to point at. Now it is the Intel fallback — `macos-latest`
/// is arm64, so that is what ships. Both halves are asserted: dropping the
/// download would send every Mac operator back to a 20-minute compile, and
/// dropping the fallback would leave an Intel Mac with an instruction that
/// produces a binary it cannot execute.
#[test]
fn the_slack_message_offers_a_download_for_every_published_archive() {
    let workflow = deploy_workflow();

    for required in [
        "navigator-${TAG}-windows.zip",
        "navigator-${TAG}-macos.tar.gz",
        "navigator-${TAG}-linux.tar.gz",
    ] {
        assert!(
            workflow.contains(required),
            "#navigator's install instructions must name the published archive `{required}`"
        );
    }

    assert!(
        workflow.contains("On an Intel Mac there is no prebuilt archive"),
        "the message must say which Mac the download does not cover"
    );
    for required in [
        "git clone --depth 1 --branch",
        "NAVIGATOR_RELEASE_TAG=",
        "cargo install --locked --path",
        "/tmp/navigator.XXXXXX",
    ] {
        assert!(
            workflow.contains(required),
            "the Intel-Mac fallback must build the immutable source tag: `{required}`"
        );
    }
}

/// The macOS archive, and the 404 it closes.
///
/// `.github/actions/validate` has always mapped a macOS runner to
/// `platform=macos` and downloaded `navigator-<tag>-macos.tar.gz`. Nothing
/// built one, so the notation gate failed on any Project repository that ran
/// it on a macOS runner — the same breakage the Linux job's comment describes,
/// one platform over, and invisible from this repository because the failure
/// lands in the consumer's CI.
#[test]
fn releases_build_and_attach_a_macos_cli_archive() {
    let workflow = deploy_workflow();

    for required in [
        "release-cli-build-macos:",
        "runs-on: macos-latest",
        "install -m 0755 target/release/navigator dist/navigator-macos/navigator",
        "install -m 0644 LICENSE.md dist/navigator-macos/LICENSE.md",
        "install -m 0644 LICENSE-MIT dist/navigator-macos/LICENSE-MIT",
        "install -m 0644 LICENSE-APACHE dist/navigator-macos/LICENSE-APACHE",
        "-C dist/navigator-macos navigator LICENSE.md LICENSE-MIT LICENSE-APACHE",
        "name: navigator-macos-cli",
        "gh release upload \"${TAG}\" dist/navigator-*-macos.tar.gz",
    ] {
        assert!(
            workflow.contains(required),
            "deploy.yml must retain the macOS CLI release contract `{required}`"
        );
    }
}

/// The same two-file contract the Linux archive is held to, for the platform
/// whose absence was the reason to write this test.
#[test]
fn the_macos_archive_name_matches_what_the_validate_action_downloads() {
    assert!(
        validate_action().contains("macOS)  platform=macos"),
        "the validate action must still map a macOS runner to the `macos` platform"
    );
    assert!(
        deploy_workflow().contains("dist/navigator-${TAG}-macos.tar.gz"),
        "deploy.yml must build the exact asset name the validate action downloads"
    );
}

/// Same rule as the other two builds: no `if:` of its own.
#[test]
fn every_published_release_builds_the_macos_cli() {
    let build = deploy_job("release-cli-build-macos");

    assert!(
        build.get("if").is_none(),
        "release-cli-build-macos must carry no `if:` gate \u{2014} `needs` decides when it runs"
    );

    let needs = job_needs("release-cli-build-macos");
    for required in ["publish-service", "publish-triggers", "release-version"] {
        assert!(
            needs.iter().any(|entry| entry == required),
            "release-cli-build-macos must run beside the publish jobs, so it needs `{required}`"
        );
    }
}

#[test]
fn the_macos_build_checks_out_the_commit_it_claims() {
    assert_builds_from_the_sha("release-cli-build-macos");
}

/// A build that fails must page, and only the jobs `notify-failure` lists can.
///
/// The list is hand-maintained and the CLI builds are the easiest rows to
/// forget: they are peers of the publishes rather than successors, so a green
/// publish reads like a green release right up until the Release carries two
/// archives instead of three.
#[test]
fn a_failed_cli_build_pages_engineering() {
    let needs = job_needs("notify-failure");
    for required in [
        "release-windows-cli-build",
        "release-cli-build-linux",
        "release-cli-build-macos",
    ] {
        assert!(
            needs.iter().any(|entry| entry == required),
            "notify-failure cannot report a failure in `{required}` unless it needs it"
        );
    }
}

/// The Linux archive is the one CI actually consumes, and it went missing for
/// long enough that no repository had a working notation gate. `deploy.yml`
/// built and attached Windows only, while `.github/actions/validate` asks for
/// `navigator-<tag>-linux.tar.gz` on every runner.
#[test]
fn releases_build_and_attach_a_linux_cli_archive() {
    let workflow = deploy_workflow();

    for required in [
        "release-cli-build-linux:",
        "runs-on: ubuntu-latest",
        "install -m 0755 target/release/navigator dist/navigator-linux/navigator",
        "install -m 0644 LICENSE.md dist/navigator-linux/LICENSE.md",
        "install -m 0644 LICENSE-MIT dist/navigator-linux/LICENSE-MIT",
        "install -m 0644 LICENSE-APACHE dist/navigator-linux/LICENSE-APACHE",
        "-C dist/navigator-linux navigator LICENSE.md LICENSE-MIT LICENSE-APACHE",
        "name: navigator-linux-cli",
        "gh release upload \"${TAG}\" dist/navigator-*-linux.tar.gz",
    ] {
        assert!(
            workflow.contains(required),
            "deploy.yml must retain the Linux CLI release contract `{required}`"
        );
    }
}

/// The archive name is a contract between two files that never reference each
/// other. `.github/actions/validate` composes
/// `navigator-${VERSION}-${platform}.tar.gz` with `platform=linux`; `deploy.yml`
/// has to produce exactly that. Nothing else in the tree ties them together,
/// and the drift cost every consuming repository its `ci` job.
#[test]
fn the_linux_archive_name_matches_what_the_validate_action_downloads() {
    let action = validate_action();

    assert!(
        action.contains("navigator-${VERSION}-${platform}.tar.gz"),
        "the validate action must still compose its asset name from VERSION and platform"
    );
    assert!(
        action.contains("Linux)  platform=linux"),
        "the validate action must still map a Linux runner to the `linux` platform"
    );
    assert!(
        deploy_workflow().contains("dist/navigator-${TAG}-linux.tar.gz"),
        "deploy.yml must build the exact asset name the validate action downloads"
    );
}

/// Every CLI archive carries the licence and both grants.
///
/// They answer different questions and none substitutes for another:
/// `LICENSE.md` states the dual grant, the content licence, and the trademark
/// reservation, while `LICENSE-MIT` and `LICENSE-APACHE` are the two texts the
/// recipient chooses between. A recipient holds the archive and not the
/// repository — that is the whole point of shipping a binary — so MIT's
/// condition that the notice travel with every copy, and Apache-2.0 § 4(a)'s
/// obligation to hand recipients the License, are met by the archive or not at
/// all. An archive naming a choice whose texts it omits offers no choice.
///
/// Asserted per platform rather than once over the file, because the packaging
/// steps are written in different shells against different paths and a fix to
/// one has already missed another.
#[test]
fn every_cli_archive_carries_the_licence_and_both_grants() {
    let workflow = deploy_workflow();

    for (platform, licence, second_grant) in [
        (
            "Windows",
            "Copy-Item \"LICENSE.md\"",
            "Copy-Item \"LICENSE-APACHE\"",
        ),
        (
            "Linux",
            "install -m 0644 LICENSE.md dist/navigator-linux/LICENSE.md",
            "install -m 0644 LICENSE-APACHE dist/navigator-linux/LICENSE-APACHE",
        ),
        (
            "macOS",
            "install -m 0644 LICENSE.md dist/navigator-macos/LICENSE.md",
            "install -m 0644 LICENSE-APACHE dist/navigator-macos/LICENSE-APACHE",
        ),
    ] {
        assert!(
            workflow.contains(licence),
            "the {platform} archive must stage LICENSE.md"
        );
        assert!(
            workflow.contains(second_grant),
            "the {platform} archive must stage LICENSE-APACHE — a dual licence \
             whose second half is missing is a single licence"
        );
    }
}

/// Same rule as the Windows build: no `if:` of its own, so a release cannot
/// publish images while quietly shipping no CLI for CI to install.
#[test]
fn every_published_release_builds_the_linux_cli() {
    let build = deploy_job("release-cli-build-linux");

    assert!(
        build.get("if").is_none(),
        "release-cli-build-linux must carry no `if:` gate \u{2014} `needs` decides when it runs"
    );

    let needs = job_needs("release-cli-build-linux");
    for required in ["publish-service", "publish-triggers", "release-version"] {
        assert!(
            needs.iter().any(|entry| entry == required),
            "release-cli-build-linux must run beside the publish jobs, so it needs `{required}`"
        );
    }
}

/// One publish job attaches all three archives. Several would each run
/// `gh release create` behind a check-then-act `if ! gh release view` guard and
/// race on the same tag.
#[test]
fn one_publish_job_attaches_every_cli_archive() {
    let needs = job_needs("release-windows-cli-publish");
    for required in [
        "release-windows-cli-build",
        "release-cli-build-linux",
        "release-cli-build-macos",
    ] {
        assert!(
            needs.iter().any(|entry| entry == required),
            "the publish job attaches every archive, so it needs `{required}`"
        );
    }

    // Count real invocations, not prose: this file's own comments discuss the
    // command, and an earlier version of this assertion counted them.
    let invocations = deploy_workflow()
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#') && trimmed.starts_with("gh release create")
        })
        .count();
    assert_eq!(
        invocations, 1,
        "exactly one job may create the Release, or two runs race on the same tag"
    );
}

/// The Linux archive is the one CI installs, so it is held to the same rule as
/// the Windows one: built from the tag it is named for.
#[test]
fn the_linux_build_checks_out_the_commit_it_claims() {
    assert_builds_from_the_sha("release-cli-build-linux");
}
