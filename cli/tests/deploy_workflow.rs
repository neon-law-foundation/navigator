use std::path::Path;

fn deploy_workflow() -> String {
    repo_file(".github/workflows/deploy.yml")
}

fn repo_file(path: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(path),
    )
    .unwrap_or_else(|error| panic!("read {path}: {error}"))
}

/// The trigger block, parsed. `on` is the YAML 1.1 boolean `true`, so
/// `serde_yaml` keys it as a bool rather than the string "on" — reading it by
/// name silently finds nothing and every assertion below would pass vacuously.
fn deploy_triggers() -> serde_yaml::Mapping {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&deploy_workflow()).expect("deploy.yml parses as YAML");
    let triggers = workflow
        .get(serde_yaml::Value::Bool(true))
        .or_else(|| workflow.get("on"))
        .expect("deploy.yml must declare a trigger block");
    triggers
        .as_mapping()
        .expect("the trigger block must be a mapping")
        .clone()
}

fn has_trigger(name: &str) -> bool {
    deploy_triggers().contains_key(serde_yaml::Value::String(name.to_string()))
}

#[test]
fn deploy_workflow_has_no_pull_request_trigger() {
    let workflow = deploy_workflow();

    assert!(
        !workflow.contains("\n  pull_request:\n"),
        "deploy.yml must not trigger on pull_request — UI/browser proof runs on the \
         release train and locally, never on a PR"
    );
}

/// A PUSHED TAG IS THE ONLY WAY TO PUBLISH, and that is what makes an image's
/// version trustworthy.
///
/// The clock and the dispatch both derived a version from `date`, so the name an
/// image carried stood behind no Git ref: `Cargo.toml` sat at one version while
/// published images marched on under another, and a plain build of the source a
/// release was cut from misreported itself. Deriving is what allowed the drift —
/// a tag cannot drift from itself, because `release-version` refuses a tag that
/// does not equal `[workspace.package].version`.
///
/// Both retired triggers are asserted absent rather than merely unused. A
/// surviving `workflow_dispatch` would publish whatever `date` returned, under a
/// version no tag and no manifest agrees with, and would go green doing it.
#[test]
fn deploy_workflow_publishes_only_from_a_pushed_tag() {
    assert!(
        !has_trigger("schedule"),
        "deploy.yml must not publish on a clock: a cron carries no tag, so it can only derive a \
         version, which is exactly the drift the tag-equals-manifest check exists to stop"
    );
    assert!(
        !has_trigger("workflow_dispatch"),
        "deploy.yml must not publish on demand: a dispatch runs from a branch and would publish a \
         derived version no Git tag stands behind. Push the tag instead"
    );
    assert!(
        has_trigger("push"),
        "deploy.yml publishes from a pushed tag, so it must keep its `push` trigger"
    );
}

/// THE WORKFLOW DEPLOYS NOTHING. It ends at the registry: every rollout is
/// `navigator ops ship`, run by a person against their own short-lived
/// credentials.
///
/// This is a security boundary, not a preference. A pipeline that can roll a
/// cluster is a pipeline whose compromise rolls that cluster, and a ship job
/// added back here would restore that reach silently — the run would go green
/// and nobody would read the diff that did it. Two things are asserted, because
/// a job can reach a cloud provider without being called `ship-*`: no such job
/// exists, and no step federates an identity into one.
#[test]
fn deploy_workflow_ships_nothing_and_holds_no_cloud_credential() {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&deploy_workflow()).expect("deploy.yml parses as YAML");
    let jobs = workflow["jobs"]
        .as_mapping()
        .expect("deploy.yml must declare jobs");
    let names: Vec<String> = jobs
        .keys()
        .map(|key| key.as_str().unwrap_or("<non-string job key>").to_string())
        .collect();

    let shipping: Vec<&String> = names
        .iter()
        .filter(|name| name.starts_with("ship"))
        .collect();
    assert!(
        shipping.is_empty(),
        "deploy.yml must contain no ship job: {shipping:?}. Publishing is automated; deploying is \
         a human act run from `navigator ops ship`, so nothing here may roll a cluster"
    );

    // Comments are stripped first. The header explains at length that this
    // workflow federates into no cloud provider, and naming the thing it does
    // not do must not read as doing it.
    let source = deploy_workflow();
    let effective: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");

    for forbidden in ["google-github-actions/auth", "workload_identity_provider"] {
        assert!(
            !effective.contains(forbidden),
            "deploy.yml still references `{forbidden}` — this workflow reaches no cloud provider, \
             and a surviving credential exchange is reach it does not need"
        );
    }
}

/// The `inputs` context cannot exist without a declared input, so any reference
/// to one is dead. This is ENG-182 as a guard.
#[test]
fn deploy_workflow_references_no_workflow_inputs() {
    let workflow = deploy_workflow();

    assert!(
        !workflow.contains("inputs."),
        "deploy.yml declares no inputs, so `inputs.<name>` always evaluates empty — a reference \
         to one is dead and reads as a knob that exists (ENG-182)"
    );
}

/// The tag filter and the `release-tags` ruleset must admit the same shape.
///
/// `cli/src/devx/github_setup.rs` protects `refs/tags/[0-9]*.[0-9]*.[0-9]*`
/// against deletion and update. A filter here that admitted more than that —
/// `v*`, or a bare `*` — would let an unprotected, movable tag start a publish,
/// and a moved tag makes every artifact already carrying that version a lie.
#[test]
fn deploy_workflow_publishes_from_a_dated_tag() {
    let triggers = deploy_triggers();
    let push = triggers
        .get(serde_yaml::Value::String("push".into()))
        .expect("deploy.yml must keep its push trigger");

    let tags: Vec<String> = serde_yaml::from_value(push["tags"].clone())
        .expect("the push trigger must carry a `tags` filter — a tag is the publish path");
    assert!(
        tags.iter().any(|glob| glob == "[0-9]*.[0-9]*.[0-9]*"),
        "deploy.yml must publish only from a dated tag matching the `release-tags` ruleset glob \
         `[0-9]*.[0-9]*.[0-9]*`, so every publishing ref is one GitHub refuses to move. Got: \
         {tags:?}"
    );

    // The pre-publish iteration seam: the only way to prove a change to this
    // workflow without spending a day's tag to find out.
    let branches: Vec<String> = serde_yaml::from_value(push["branches"].clone())
        .expect("the push trigger must keep its `kind-ci/**` branch filter");
    assert!(
        branches.iter().any(|glob| glob == "kind-ci/**"),
        "deploy.yml must keep the `kind-ci/**` integration-only trigger: it is the one way to \
         prove a workflow change without spending a tag. Got: {branches:?}"
    );
}

/// The tag is CHECKED against the UTC clock, not derived from it.
///
/// The zone is a decision, and the wrong one rejects a whole day's releases for
/// part of the year. UTC is the zone `YY.M.D` has always been derived in, it has
/// no DST discontinuity, and `cli/src/release_version.rs` writes the manifest
/// version in that same zone — so a local-zone check here would reject the tag
/// the CLI just told the operator to push.
#[test]
fn the_release_tag_must_be_todays_utc_date() {
    let workflow = deploy_workflow();

    assert!(
        workflow.contains("TZ=UTC date +'%y %m %d'"),
        "deploy.yml must compare the pushed tag against the UTC clock"
    );
    assert!(
        !workflow.contains("TZ=America"),
        "the release date is UTC: it is the zone `YY.M.D` has always been derived in, it has no \
         DST discontinuity, and `ops release-version` writes the manifest in it"
    );
}

/// A SAME-DAY HOTFIX HAS A SPELLING, and it hangs off TOMORROW's date.
///
/// `YY.M.D` admits one ordinary release per UTC day and the `release-tags`
/// ruleset will not let anyone move the tag, so the day's release name is spent
/// the moment it is pushed. A semver prerelease is the escape hatch — Cargo
/// parses `26.8.18-hotfix.17` where it rejects a fourth component outright.
///
/// THE BASE IS THE NEXT DAY BECAUSE SEMVER RANKS A PRERELEASE BELOW ITS OWN
/// BASE (spec §11.3). `26.8.17-hotfix.17` would sort as OLDER than the `26.8.17`
/// it exists to fix, so Cargo, Homebrew, and every image sort would read the fix
/// as the earlier release. This test pins the tomorrow-base rule shut.
#[test]
fn a_hotfix_tag_is_a_prerelease_on_tomorrows_utc_date() {
    let workflow = deploy_workflow();

    assert!(
        workflow.contains("(-hotfix\\.(0|[1-9][0-9]*))?$"),
        "the shape check must admit an optional `-hotfix.N` suffix with any unpadded numeric N — \
         a missing, empty, nonnumeric, or padded number is invalid"
    );
    assert!(
        workflow.contains("date -d 'tomorrow'"),
        "the hotfix base is TOMORROW's UTC date, so the step must derive it"
    );
    assert!(
        workflow.contains("expected=\"${tomorrow}\""),
        "a prerelease tag must be validated against tomorrow's base, not today's"
    );
    assert!(
        workflow.contains("expected=\"${today}\""),
        "an ordinary release must still be validated against today's base"
    );
}

/// The release source must already have passed through `main`. A tag can be
/// pushed from any commit, and Git commits retain no branch name, so the first
/// job has to fetch `origin/main`, peel the tag to its commit, and prove that
/// commit is an ancestor before any image, archive, or GitHub Release publishes.
#[test]
fn publication_waits_for_the_main_reachability_guard() {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&deploy_workflow()).expect("deploy.yml parses as YAML");
    let release = &workflow["jobs"]["release-version"];
    let steps = release["steps"]
        .as_sequence()
        .expect("release-version must declare steps");

    let guard = steps
        .iter()
        .find(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("ops release-provenance"))
        })
        .expect("release-version must invoke the Rust release-provenance guard");
    let run = guard["run"].as_str().expect("the guard must be a run step");
    assert!(run.contains("--tag \"${REF_NAME}\""), "{run}");

    let checkout = steps
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .unwrap_or_default()
                .starts_with("actions/checkout")
        })
        .expect("release-version must check out the tagged tree");
    assert_eq!(
        checkout["with"]["fetch-depth"].as_u64(),
        Some(0),
        "the ancestry test needs the complete tag and main history"
    );

    for publisher in [
        "publish-service",
        "publish-triggers",
        "release-windows-cli-build",
        "release-cli-build-linux",
        "release-cli-build-macos",
        "release-windows-cli-publish",
    ] {
        let needs = match &workflow["jobs"][publisher]["needs"] {
            serde_yaml::Value::String(need) => vec![need.clone()],
            serde_yaml::Value::Sequence(needs) => needs
                .iter()
                .map(|need| {
                    need.as_str()
                        .unwrap_or_else(|| panic!("{publisher} has a non-string need"))
                        .to_string()
                })
                .collect(),
            other => panic!("{publisher} needs must be a string or list, got {other:?}"),
        };
        assert!(
            needs.iter().any(|need| need == "release-version"),
            "{publisher} must wait for the main-reachability guard"
        );
    }
}

/// A hotfix must not masquerade as the latest release, in either place that
/// decides what a user gets by default.
///
/// The GitHub Release is flagged so it stops being reported as "Latest", and the
/// Homebrew tap is not told about it at all — the formula holds exactly one
/// version, so bumping it to a prerelease would hand an rc to every `brew
/// upgrade` while ranking below the ordinary release it precedes.
#[test]
fn a_hotfix_publishes_as_a_prerelease_and_never_reaches_the_tap() {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&deploy_workflow()).expect("deploy.yml parses as YAML");

    let outputs = &workflow["jobs"]["release-version"]["outputs"];
    assert!(
        outputs["prerelease"].as_str().is_some(),
        "`release-version` must publish a `prerelease` output for downstream jobs to gate on"
    );

    let gate = workflow["jobs"]["release-homebrew-tap"]["if"]
        .as_str()
        .expect("the tap job must declare an `if:` gate");
    assert!(
        gate.contains("prerelease != 'true'"),
        "the Homebrew tap must be skipped for a hotfix, got: {gate:?}"
    );

    assert!(
        deploy_workflow().contains("flags+=(--prerelease)"),
        "the GitHub Release for a hotfix must be created with --prerelease so it is not reported \
         as the latest release"
    );
}

/// THE TAG MUST CARRY ITS OWN VERSION. This is the check that makes a published
/// image's self-reported version true.
///
/// Without it `Cargo.toml` sat at `0.1.0` while tags marched on, so `navigator
/// --version` and a plain build of the tagged source both named a release the
/// source had never heard of. `cli/build.rs` bakes
/// `[workspace.package].version` into the binary, and `RELEASE_TAG` stamps the
/// image — this comparison is the only thing forcing those two to agree.
///
/// It must fail the run at the FIRST job. A mismatch caught after forty minutes
/// of image builds has already spent the day's tag, which the `release-tags`
/// ruleset will not let anyone move.
#[test]
fn the_release_tag_must_equal_the_workspace_version() {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&deploy_workflow()).expect("deploy.yml parses as YAML");
    let steps = workflow["jobs"]["release-version"]["steps"]
        .as_sequence()
        .expect("release-version must declare steps");

    let script: String = steps
        .iter()
        .filter_map(|step| step["run"].as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        script.contains("workspace.package"),
        "release-version must read `[workspace.package].version` out of Cargo.toml and refuse a \
         tag that does not equal it — otherwise a published container reports a version its own \
         source never carried"
    );

    // Reading the manifest and building the Rust provenance guard require the
    // source on disk. A sparse metadata checkout would make one or both checks
    // impossible.
    let checkout = steps
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .unwrap_or_default()
                .starts_with("actions/checkout")
        })
        .expect("release-version must check out the tree it validates");
    assert!(
        checkout["with"]["sparse-checkout"].is_null(),
        "release-version must check out the full source for Cargo.toml and the Rust provenance guard"
    );
}

/// A CONTAINER MUST REPORT THE VERSION ITS IMAGE IS TAGGED WITH.
///
/// Two independent things hold this, and both are needed. The
/// tag-equals-`Cargo.toml` check above makes the *source* carry the release
/// name, so a plain `cargo build` of the tagged tree self-reports correctly.
/// This one makes the *image* carry it: the tag is passed as the `RELEASE_TAG`
/// build-arg, each Containerfile turns it into a runtime
/// `ENV NAVIGATOR_RELEASE_TAG`, and `main.rs` reads that override.
///
/// Drop the build-arg and nothing fails: images still publish, and every one of
/// them silently reports whatever the manifest happened to say. That silence is
/// why this is asserted.
#[test]
fn every_image_is_stamped_with_the_release_tag() {
    let workflow = deploy_workflow();

    assert!(
        workflow.contains("printf 'RELEASE_TAG=%s"),
        "deploy.yml must pass the derived version to every image build as the `RELEASE_TAG` \
         build-arg — without it a published container reports the wrong version and nothing fails"
    );

    let containerfile = repo_file("images/Containerfile.neon");
    assert!(
        containerfile.contains("ARG RELEASE_TAG"),
        "Containerfile.neon must accept the RELEASE_TAG build-arg deploy.yml passes it"
    );
    assert!(
        containerfile.contains("ENV NAVIGATOR_RELEASE_TAG=$RELEASE_TAG"),
        "Containerfile.neon must expose RELEASE_TAG as the runtime NAVIGATOR_RELEASE_TAG override \
         `main.rs` reads — a build-arg that never becomes an ENV stamps nothing"
    );
}

/// Nothing in the release pipeline may move a Git ref. `release-version` held
/// `contents: write` to cut the nightly tag; a human pushes the tag now, so the
/// permission is gone and the whole workflow is read-only against the
/// repository.
#[test]
fn no_job_can_write_repository_contents() {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&deploy_workflow()).expect("deploy.yml parses as YAML");

    assert_eq!(
        workflow["permissions"]["contents"].as_str(),
        Some("read"),
        "the workflow-level contents permission must stay `read`"
    );

    let jobs = workflow["jobs"]
        .as_mapping()
        .expect("deploy.yml must declare jobs");
    let mut writers = Vec::new();
    for (name, job) in jobs {
        if job["permissions"]["contents"].as_str() == Some("write") {
            writers.push(name.as_str().unwrap_or("<non-string job key>").to_string());
        }
    }

    // `release-windows-cli-publish` is the ONE exception: it creates the
    // GitHub Release that hangs off the already-pushed tag and uploads the CLI
    // archives to it. It creates no ref.
    assert_eq!(
        writers,
        ["release-windows-cli-publish"],
        "only the Release-attach job may hold `contents: write`. `release-version` validates the \
         ref it was handed and must never create one again"
    );
}

/// The browser gate builds every image it then audits.
///
/// It used to clone the deployed pod on a second port and run a second brand
/// binary beside it, because the firm and the Foundation were separate images.
/// One binary serves both faces now, so that whole apparatus is gone — and what
/// survives is the part that always mattered: the images the gate exercises are
/// the ones a deployment rolls, not a route substitution.
#[test]
fn browser_accessibility_uses_the_shipped_images() {
    let workflow = deploy_workflow();

    for required in [
        "          - image: neon-server\n            dockerfile: images/Containerfile.neon",
        "for img in navigator-web neon-server navigator-workflows-service navigator-gateway; do",
    ] {
        assert!(
            workflow.contains(required),
            "deploy.yml must keep browser accessibility proof `{required}`"
        );
    }

    // The retired second-host apparatus must not come back: it cloned the web
    // Deployment onto port 3002 to run a second brand image, and there is no
    // second brand image to run.
    for retired in [
        "neon-browser-a11y",
        "NAV_BASE_URL: http://localhost:3002",
        ".image = \"neon-server:dev\"",
    ] {
        assert!(
            !workflow.contains(retired),
            "deploy.yml still carries the retired second-host clone `{retired}`; \
             one binary serves both faces, so the gate audits one host"
        );
    }
}

/// The two public-host images compile the full server and Dioxus web bundle.
/// A stock `ubuntu-latest` runner timed out building `neon-server` at the
/// release job's 90-minute wedge detector (run 32185875546), while the
/// repository's established eight-vCPU Blacksmith lane already carries the
/// workspace build. Keep only these two heavy matrix legs on that runner; the
/// smaller service images do not earn a metered machine.
#[test]
fn public_host_images_build_on_the_blacksmith_eight_vcpu_runner() {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&deploy_workflow()).expect("deploy.yml parses as YAML");
    let matrix = workflow["jobs"]["build"]["strategy"]["matrix"]["include"]
        .as_sequence()
        .expect("the build job must declare an include matrix");

    for image in ["navigator-web", "neon-server"] {
        let leg = matrix
            .iter()
            .find(|leg| leg["image"].as_str() == Some(image))
            .unwrap_or_else(|| panic!("the build matrix must include {image}"));
        assert_eq!(
            leg["runner"].as_str(),
            Some("blacksmith-8vcpu-ubuntu-2404"),
            "{image} compiles the full Rust and Dioxus application and must use the established \
             eight-vCPU Blacksmith runner"
        );
    }
}

/// Slack is an optional reporting surface, not a publication gate. Progress
/// posts already notice-and-skip when the webhook is absent; the terminal
/// success report and failure alert must follow the same contract. Otherwise a
/// fully published release ends red solely because this public repository has
/// no Slack secret configured (run 32148764921).
#[test]
fn missing_slack_webhook_does_not_fail_the_release_workflow() {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&deploy_workflow()).expect("deploy.yml parses as YAML");

    for job in ["notify", "notify-failure"] {
        let steps = workflow["jobs"][job]["steps"]
            .as_sequence()
            .unwrap_or_else(|| panic!("{job} must declare steps"));
        for script in steps.iter().filter_map(|step| step["run"].as_str()) {
            if script.contains("SLACK_WEBHOOK_URL is unset") {
                assert!(
                    script.contains("::notice::SLACK_WEBHOOK_URL is unset"),
                    "{job} must report an absent optional webhook as a notice"
                );
                assert!(
                    script.contains("exit 0"),
                    "{job} must skip successfully when the optional webhook is absent"
                );
                assert!(
                    !script.contains("::error::SLACK_WEBHOOK_URL is unset"),
                    "{job} must not turn an absent optional webhook into a release failure"
                );
            }
        }
    }
}

/// THE 502 RACE, kept as a guard against its return. A one-shot
/// `curl --fail .../readyz` under `set -e`, fired while a load balancer is
/// still swapping backends, went red on `neon-production` in run 154026811
/// AFTER the roll had succeeded.
///
/// This workflow no longer probes a deployed host at all — it publishes images
/// and stops — so the assertion is now the absence of the shape rather than the
/// presence of the fix. If a probe is ever added back here, it must poll to a
/// deadline with the curl inside an `if`, never bare under `set -e`.
#[test]
fn no_readyz_probe_is_one_shot_curled() {
    let workflow = deploy_workflow();

    let bare: Vec<&str> = workflow
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("curl ") && line.contains("/readyz"))
        .collect();
    assert!(
        bare.is_empty(),
        "a /readyz probe must never be a bare command under `set -e` — a transient 502 during \
         the load-balancer swap then fails a step that already succeeded. Put the curl in an \
         `if` condition and poll to a deadline. Found: {bare:#?}"
    );
}

/// NO CLUSTER MANIFEST IS FETCHED AT RUN TIME. Every `kubectl apply` in the
/// KIND job reads a file this repository vendors.
///
/// `raw.githubusercontent.com` rate-limits by runner IP, and `kubectl` turns
/// its 429 into a hard error rather than a retry: run 32040810491 lost a
/// release four minutes into the integration job, after an hour of image
/// builds, because the ingress manifest happened to be unreachable in that
/// minute. Vendoring is also what makes the version pin real — a URL pinned
/// to a tag still trusts whatever bytes that tag serves today, while
/// `cli::devx::ingress_manifest_tests` holds the vendored copy to a recorded
/// digest.
///
/// The assertion is on the shape, not on the two known URLs, because the next
/// manifest added here would reintroduce the outage silently: the run would go
/// green on every attempt where the third party happened to answer.
#[test]
fn every_kubectl_apply_reads_a_vendored_manifest() {
    let workflow = deploy_workflow();

    // Backslash continuations are folded first: the Restate CRD apply carries
    // its `-f` argument on the following line, so a plain line scan sees a
    // `kubectl apply` with no URL and a URL with no `kubectl apply`, and passes
    // while the fetch is still there.
    let folded = workflow.replace("\\\n", " ");
    let remote: Vec<String> = folded
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.starts_with('#'))
        .filter(|line| line.contains("kubectl apply") && line.contains("://"))
        .collect();
    assert!(
        remote.is_empty(),
        "a release must not depend on a third party serving a manifest in the minute it runs — \
         vendor it under `k8s/vendor/` and apply the file, as `cli::devx::orchestrate` does. \
         Found: {remote:#?}"
    );

    // Both vendored roots must be named here, and every artifact must be
    // present in the tree — otherwise the apply trades a 429 for a missing
    // file and nothing is gained. The Restate CRDs are applied through a shell
    // loop, so the directory is what appears literally.
    for named in [
        "k8s/vendor/ingress-nginx-controller-v1.11.2.yaml",
        "k8s/vendor/restate-operator-v2.8.1/",
    ] {
        assert!(
            workflow.contains(named),
            "deploy.yml must apply the vendored `{named}`, keeping the KIND job on the same \
             manifests `dev up` applies locally"
        );
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for vendored in [
        "k8s/vendor/ingress-nginx-controller-v1.11.2.yaml",
        "k8s/vendor/restate-operator-v2.8.1/restateclusters.yaml",
        "k8s/vendor/restate-operator-v2.8.1/restatedeployments.yaml",
        "k8s/vendor/restate-operator-v2.8.1/restatecloudenvironments.yaml",
    ] {
        assert!(
            root.join(vendored).exists(),
            "`{vendored}` is named by deploy.yml but is missing from the tree"
        );
    }
}

#[test]
fn standalone_wasm_workflow_stays_retired() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    assert!(
        !root.join(".github/workflows/webapp-wasm.yml").exists(),
        "the deploy image build is the one Dioxus wasm proof path"
    );
}
