//! Pin how `.github/actions/application-publish` moves the bytes.
//!
//! The action uploads a Project's built portal into the shared, private
//! applications bucket. Two properties of *how* it uploads are load-bearing and
//! neither is visible from reading the step name:
//!
//! 1. **It must overwrite unconditionally.** The bucket carries an object-age
//!    Delete rule ([`APPLICATIONS_RETENTION_DAYS`] in
//!    `cli::devx::gcp::buckets`). An upload that skips unchanged objects leaves
//!    a live asset's age running while `index.html` keeps naming it, so the
//!    entry document outlives the assets it points at. `gcloud storage rsync`
//!    skips unchanged objects by definition, which is why it is forbidden here.
//! 2. **It must merge, not nest.** `gcloud storage cp --recursive <dir> <dst>`
//!    writes `<dst>/<dir>/...` — a trailing slash on the source does not change
//!    that — so uploading `portal/dist` would publish
//!    `<code>/portal/dist/assets/...`, a path Navigator does not serve. The
//!    action uploads the directory's *entries* instead.
//!
//! Unlike `project_gate.rs`, which asserts presence in source because executing
//! that gate would need a runner, the upload step here *is* executed: it is a
//! self-contained bash block whose only outside call is `gcloud`. Stubbing
//! `gcloud` and reading back its argv tests the real thing, and it is the only
//! way to catch the nesting trap — a source-text assertion cannot tell
//! `cp -r dist dst` from `cp -r dist/* dst`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace root (`CARGO_MANIFEST_DIR` points at `cli/`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("workspace root exists")
}

fn action_path() -> PathBuf {
    workspace_root().join(".github/actions/application-publish/action.yml")
}

fn action_source() -> String {
    let path = action_path();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The composite's steps, in declaration order.
fn steps() -> Vec<serde_yaml::Mapping> {
    let action: serde_yaml::Value =
        serde_yaml::from_str(&action_source()).expect("action.yml parses as YAML");
    action
        .get("runs")
        .and_then(|r| r.get("steps"))
        .and_then(|s| s.as_sequence())
        .expect("the composite declares steps")
        .iter()
        .map(|s| s.as_mapping().expect("each step is a mapping").clone())
        .collect()
}

/// The index of the one step whose `name` starts with `prefix`.
fn step_index(prefix: &str) -> usize {
    let steps = steps();
    let matches: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.get(serde_yaml::Value::from("name"))
                .and_then(|n| n.as_str())
                .is_some_and(|n| n.starts_with(prefix))
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one step named `{prefix}...`, found {}",
        matches.len()
    );
    matches[0]
}

/// The `run:` script of the step named `prefix...`.
fn step_script(prefix: &str) -> String {
    steps()[step_index(prefix)]
        .get(serde_yaml::Value::from("run"))
        .and_then(|r| r.as_str())
        .expect("the step runs a script")
        .to_string()
}

/// Run one of the action's bash steps with `gcloud` stubbed, and return the
/// argv the step passed to it.
///
/// `dist` names the files to create under `<workdir>/portal/dist`. The stub
/// writes its argv one-per-line, so a caller can assert the exact command
/// without depending on the real `gcloud` being installed or authenticated.
fn run_step_with_gcloud_stub(script: &str, dist: &[&str]) -> Result<Vec<String>, String> {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();

    for relative in dist {
        let file = root.join("portal/dist").join(relative);
        fs::create_dir_all(file.parent().expect("file has a parent")).expect("create dist dirs");
        fs::write(&file, b"x").expect("write dist file");
    }

    let bin = root.join("bin");
    fs::create_dir_all(&bin).expect("create stub bin");
    let argv_log = root.join("argv.txt");
    let stub = bin.join("gcloud");
    fs::write(
        &stub,
        format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" > {}\n",
            argv_log.display()
        ),
    )
    .expect("write gcloud stub");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("chmod stub");

    let script_path = root.join("step.sh");
    fs::write(&script_path, script).expect("write step script");

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new("bash")
        .arg(&script_path)
        .current_dir(root)
        .env("PATH", path)
        .env("BUCKET", "a-deployment-applications")
        .env("PREFIX", "acme/portal")
        .env("DIST_DIR", "portal/dist")
        .output()
        .expect("run the step under bash");

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr));
    }

    Ok(read_argv(&argv_log))
}

fn read_argv(log: &Path) -> Vec<String> {
    fs::read_to_string(log)
        .expect("the step invoked gcloud")
        .lines()
        .map(str::to_string)
        .collect()
}

/// The publish never uses `gcloud storage rsync`.
///
/// This is the whole of ENG-273 in one assertion, and it guards two independent
/// failures at once. `rsync` skips unchanged objects, which lets a live asset
/// age past the bucket's Delete rule while `index.html` still names it; and
/// `rsync` compares against the destination, so it needs
/// `storage.objects.list`, a permission evaluated against the *bucket* that no
/// prefix condition can scope. A comment is allowed to name `rsync` — the
/// action explains at length why it is not used — so only executable lines are
/// searched.
#[test]
fn the_publish_never_rsyncs() {
    for (number, line) in action_source().lines().enumerate() {
        let code = line.trim_start();
        if code.starts_with('#') {
            continue;
        }
        assert!(
            !code.contains("storage rsync"),
            "line {} publishes with `gcloud storage rsync`; it skips unchanged \
             objects, so a live asset ages out under the bucket's Delete rule: {code}",
            number + 1,
        );
    }
}

/// The action tells the next reader why, naming the constant that decides it.
///
/// `rsync` is the faster call, so "optimizing" back to it is the obvious change
/// to make. Someone who does should first hit the sentence explaining that
/// unconditional overwrite is a correctness requirement of the bucket's
/// retention policy, and be able to follow the name to the rule itself.
#[test]
fn the_action_explains_the_retention_coupling() {
    let source = action_source();
    assert!(
        source.contains("APPLICATIONS_RETENTION_DAYS"),
        "the action must name the retention constant its upload is coupled to",
    );
    assert!(
        source.contains("cli/src/devx/gcp/buckets.rs"),
        "the action must point at the file defining that rule",
    );
}

/// Pass 1 uploads the dist directory's *entries*, never the directory.
///
/// The nesting trap: `cp --recursive portal/dist gs://b/acme/portal/` writes
/// `acme/portal/dist/assets/...`, which Navigator does not serve, and a trailing
/// slash on the source does not change it. Uploading each entry instead makes
/// `assets/` land at `acme/portal/assets/`. Asserted on the argv the step
/// actually builds, because the two spellings differ by three characters and
/// read identically.
#[test]
fn pass_one_uploads_entries_so_objects_are_not_nested_under_dist() {
    let argv = run_step_with_gcloud_stub(
        &step_script("publish assets"),
        &[
            "index.html",
            "assets/index-ABC.js",
            "assets/index-DEF.css",
            "documents/engagement.pdf",
            "pdf.worker.mjs",
        ],
    )
    .expect("the publish step succeeds");

    assert_eq!(
        argv[..3].to_vec(),
        vec!["storage", "cp", "--recursive"],
        "pass 1 must upload with `gcloud storage cp --recursive`, got {argv:?}",
    );

    let (sources, destination) = argv[3..].split_at(argv.len() - 4);
    assert_eq!(
        destination,
        ["gs://a-deployment-applications/acme/portal/"],
        "pass 1 must upload into the Project's own `<code>/portal/` prefix",
    );

    let mut sources: Vec<&str> = sources.iter().map(String::as_str).collect();
    sources.sort_unstable();
    assert_eq!(
        sources,
        [
            "portal/dist/assets",
            "portal/dist/documents",
            "portal/dist/pdf.worker.mjs",
        ],
        "pass 1 must pass the dist directory's entries, not the directory \
         itself — `cp --recursive portal/dist` nests every object under \
         `acme/portal/dist/`",
    );
}

/// Pass 1 holds `index.html` back.
///
/// `gcloud storage cp` has no `--exclude`, so the exclusion `rsync` expressed as
/// a flag is now a filter over the entry list. If it regressed, `index.html`
/// would publish in the same pass as the assets it names rather than after
/// them, and a reader arriving mid-publish could load an entry document naming
/// a hashed asset that does not exist yet.
#[test]
fn pass_one_holds_index_html_back() {
    let argv = run_step_with_gcloud_stub(
        &step_script("publish assets"),
        &["index.html", "assets/index-ABC.js"],
    )
    .expect("the publish step succeeds");

    assert!(
        !argv.iter().any(|a| a.ends_with("index.html")),
        "pass 1 uploaded index.html; it belongs in pass 2, after its assets: {argv:?}",
    );
}

/// `index.html` is published last, in its own pass, and overwritten with `cp`.
///
/// The ordering is what makes a mid-publish read safe: every asset the entry
/// document names is already readable before the document that names it is.
#[test]
fn index_html_publishes_after_the_assets_it_names() {
    assert!(
        step_index("publish assets") < step_index("publish index.html"),
        "index.html must publish after the assets pass",
    );
    let script = step_script("publish index.html");
    assert!(
        script.contains("gcloud storage cp"),
        "index.html must be overwritten with `cp` on every publish, so its age \
         restarts along with the assets': {script}",
    );
}

/// A build that produced no assets is refused rather than published.
///
/// With `rsync`'s `--exclude` gone, an empty entry list would otherwise reach
/// `gcloud` as a `cp` with no sources. That fails obscurely at best, and the
/// real defect is upstream: a `dist/` holding only `index.html` means the build
/// emitted nothing, and publishing it would replace a working portal's entry
/// document with one naming assets that were never uploaded.
#[test]
fn a_dist_holding_only_index_html_is_refused() {
    let error = run_step_with_gcloud_stub(&step_script("publish assets"), &["index.html"])
        .expect_err("a dist with no assets must fail the publish");
    assert!(
        error.contains("holds nothing but index.html"),
        "the refusal must say why the build is unpublishable, got: {error}",
    );
}
