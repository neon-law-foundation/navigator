//! End-to-end import tests: walk the `notation_templates/` fixture tree, lint
//! the markdown notation templates, and persist into a per-test
//! Postgres schema spun up via `store::test_support::pg`. These
//! tests prove that
//!
//! 1. Every shipped template lints clean against the full Neon Law Navigator
//!    default rule set (so the fixtures stay honest).
//! 2. The import path actually writes templates and questions to the
//!    database — re-running it is idempotent and doesn't duplicate.
//! 3. Question codes referenced by `questionnaire:` and `workflow:`
//!    end up as `questions` rows the application can later resolve.

use std::path::PathBuf;
use std::process::Command;

use sea_orm::EntityTrait;
use store::entity::{question, template};
use store::test_support::schema;

fn fixtures_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR points at cli; the templates live at the
    // repository root under `notation_templates/<category>/<name>.md`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../notation_templates")
        .canonicalize()
        .expect("templates dir exists")
}

/// Count the notation templates under the fixture tree — every `.md`
/// carrying YAML frontmatter (so `notation_templates/README.md` and other prose
/// are excluded). The import writes one `templates` row per such file,
/// so this is the expected `templates_created` count and tolerates new
/// templates landing without a hard-coded number going stale.
fn fixture_template_count() -> usize {
    fn walk(dir: &std::path::Path, n: &mut usize) {
        for entry in std::fs::read_dir(dir)
            .expect("read templates dir")
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, n);
            } else if path.extension().and_then(|s| s.to_str()) == Some("md")
                && std::fs::read_to_string(&path).is_ok_and(|s| s.starts_with("---\n"))
            {
                *n += 1;
            }
        }
    }
    let mut n = 0;
    walk(&fixtures_dir(), &mut n);
    n
}

async fn fs_storage() -> std::sync::Arc<dyn cloud::StorageService> {
    std::sync::Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-cli-import-test"))
            .await
            .expect("temp FsStorage"),
    )
}

#[tokio::test]
async fn fixture_directory_validates_clean() {
    let bin = assert_cmd::cargo::cargo_bin("navigator");
    // Run from a scratch dir so the `cli` binary's startup `dotenvy`
    // load can't pick up a developer's `.devx/env` (which points
    // `DATABASE_URL` at a KIND port-forward). This test asserts the
    // fixtures lint clean against the *default* rule set — a no-DB,
    // structural check — so it must stay hermetic and not flake on
    // whether a local port-forward happens to be up.
    let out = Command::new(&bin)
        .current_dir(std::env::temp_dir())
        .arg("validate")
        .arg(fixtures_dir())
        .output()
        .expect("run navigator validate");
    assert!(
        out.status.success(),
        "navigator validate must succeed on fixtures; stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[tokio::test]
async fn import_writes_each_fixture_template_and_question_to_postgres() {
    let s = schema().await;
    let report = cli_import::import_directory(&s.db, &fs_storage().await, &fixtures_dir())
        .await
        .expect("import succeeds");

    assert_eq!(report.files_skipped_due_to_violations, 0);
    assert_eq!(
        report.templates_created,
        fixture_template_count(),
        "expected one templates row per fixture"
    );
    assert!(
        report.questions_created >= 9,
        "each fixture references at least 3 question codes — total ≥ 9 got {}",
        report.questions_created
    );

    let templates = template::Entity::find().all(&s.db).await.unwrap();
    let codes: Vec<&str> = templates.iter().map(|t| t.code.as_str()).collect();
    assert!(codes.contains(&"trusts__nevada"));
    assert!(codes.contains(&"ca__llc_operating_agreement"));
    assert!(codes.contains(&"will__simple"));
    assert!(codes.contains(&"onboarding__retainer"));
    assert!(codes.contains(&"nv__dissolution"));
    assert!(codes.contains(&"nv__annual_report"));
    assert!(codes.contains(&"nv__modified_business_tax"));
    assert!(codes.contains(&"nv__nonprofit_501c3_formation"));
    assert!(codes.contains(&"us__form_990"));
    assert!(codes.contains(&"nv__charitable_solicitation_registration"));
    assert!(codes.contains(&"closing__letter"));

    let questions = question::Entity::find().all(&s.db).await.unwrap();
    let q_codes: Vec<&str> = questions.iter().map(|q| q.code.as_str()).collect();
    // Spot-check codes that come from the trust fixture.
    assert!(q_codes.contains(&"trustee_name"));
    assert!(q_codes.contains(&"trust_property"));
    // And from the LLC fixture.
    assert!(q_codes.contains(&"entity"));
    assert!(q_codes.contains(&"address"));
    assert!(q_codes.contains(&"people"));
}

#[tokio::test]
async fn db_backed_validate_loads_codes_and_swaps_f104() {
    let s = schema().await;
    cli_import::import_directory(&s.db, &fs_storage().await, &fixtures_dir())
        .await
        .expect("import succeeds");

    let codes = cli_import::load_question_codes(&s.db)
        .await
        .expect("load codes");
    assert!(
        codes.iter().any(|c| c == "trustee_name"),
        "loaded registry must contain canonical question codes; got {codes:?}",
    );

    let ruleset = rules::navigator_default_rules_with_codes(&codes);
    let n104 = ruleset
        .iter()
        .find(|r| r.code() == "N104")
        .expect("rule set must contain a swapped N104");
    assert_eq!(n104.code(), "N104");
}

#[tokio::test]
async fn re_running_import_is_idempotent() {
    let s = schema().await;
    let first = cli_import::import_directory(&s.db, &fs_storage().await, &fixtures_dir())
        .await
        .expect("first import");
    let second = cli_import::import_directory(&s.db, &fs_storage().await, &fixtures_dir())
        .await
        .expect("second import");

    assert_eq!(first.templates_created, fixture_template_count());
    assert_eq!(
        second.templates_created, 0,
        "second pass must not duplicate templates"
    );
    assert_eq!(
        second.questions_created, 0,
        "second pass must not duplicate questions"
    );

    let templates = template::Entity::find().all(&s.db).await.unwrap();
    assert_eq!(templates.len(), fixture_template_count());
}

/// Module shim so the integration test can call into the binary
/// crate's `import` function. The cleaner alternative would be a
/// dedicated library crate; for now expose the import API via a
/// path-based module include.
#[path = "../src/import.rs"]
mod cli_import;
