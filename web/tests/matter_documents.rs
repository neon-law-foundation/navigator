//! The matter-document write seam: persist → commit → lake-capture.
//!
//! Proves that filing a document through `web::matter_documents` does
//! all three jobs in one call: the durable blob+row, the attributed
//! repo commit (with the commit oid stamped on the row), and the
//! git-commit Parquet event written to the data lake.
//!
//! Multi-thread runtime: `commit_as` shells `git` via `spawn_blocking`.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use cloud::StorageService;
use repos::Author;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use store::documents::IngestArgs;
use store::entity::project;
use store::test_support::pg;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn record_document_persists_commits_and_captures_to_lake() {
    let repo_root = tempfile::tempdir().unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", repo_root.path());

    let db = pg().await;
    let storage: Arc<dyn StorageService> = Arc::new(
        cloud::FsStorage::new(
            std::env::temp_dir().join(format!("nav-matter-docs-{}", uuid::Uuid::now_v7())),
        )
        .await
        .unwrap(),
    );

    let proj = project::ActiveModel {
        name: ActiveValue::Set("Estate of Aries".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;

    let bytes = b"%PDF-1.7 collection notice";
    let ingested = web::matter_documents::record_document(
        &db,
        &storage,
        Author {
            name: "Aries",
            email: "aries@example.com",
        },
        &IngestArgs {
            project_id: proj,
            source: store::documents::source::EMAIL,
            filename: "notice.pdf",
            kind: "unclassified",
            content_type: "application/pdf",
            description: Some("received via support@ email"),
            source_revision_id: None,
        },
        bytes,
    )
    .await
    .expect("record_document persists");

    // (1) The document row exists and now carries its filing commit oid.
    let doc = store::entity::document::Entity::find_by_id(ingested.document_id)
        .one(&db)
        .await
        .unwrap()
        .expect("document row");
    let oid = doc.git_commit_oid.expect("git_commit_oid stamped");
    assert_eq!(oid.len(), 40, "a full sha-1 commit oid");

    // (2) The repo holds the bytes on main, authored as the sender.
    let repo = repos::RepoStore::new(repo_root.path()).path_for(proj);
    assert_eq!(
        git_show(&repo, "main:notice.pdf"),
        "%PDF-1.7 collection notice"
    );
    let author_line = git_log_format(&repo, "%an <%ae>");
    assert_eq!(author_line, "Aries <aries@example.com>");

    // (3) The git-commit event landed in the data lake as Parquet.
    let date = chrono::Utc::now().format("%Y-%m-%d");
    let key = format!("git-commits/data/dt={date}/{oid}.parquet");
    let obj = storage.get(&key).await.expect("commit event in the lake");
    assert!(!obj.bytes.is_empty(), "parquet event has bytes");

    // (4) commit_files — the e-sign path: two already-stored files land
    // in ONE commit authored as the signer, with no documents row.
    let signed = b"%PDF executed retainer";
    let cert = b"%PDF certificate";
    let oid2 = web::matter_documents::commit_files(
        &db,
        &storage,
        proj,
        Author {
            name: "Libra",
            email: "libra@example.com",
        },
        "esignature",
        "executed",
        "esignature: executed retainer + certificate of completion",
        &[
            ("signed-retainer.pdf", signed),
            ("certificate-of-completion.pdf", cert),
        ],
    )
    .await
    .expect("commit_files returns the oid");
    assert_eq!(
        git_show(&repo, "main:signed-retainer.pdf"),
        "%PDF executed retainer"
    );
    assert_eq!(
        git_show(&repo, "main:certificate-of-completion.pdf"),
        "%PDF certificate"
    );
    // Both files in one commit, authored as the signer.
    assert_eq!(git_log_format(&repo, "%an"), "Libra");
    let key2 = format!("git-commits/data/dt={date}/{oid2}.parquet");
    assert!(
        storage.get(&key2).await.is_ok(),
        "commit_files event in the lake"
    );

    std::env::remove_var("NAVIGATOR_GIT_REPO_ROOT");
}

fn git_show(repo: &Path, spec: &str) -> String {
    let out = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(["-C", repo.to_str().unwrap(), "show", spec])
        .output()
        .expect("git show");
    assert!(
        out.status.success(),
        "git show {spec}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git_log_format(repo: &Path, fmt: &str) -> String {
    let out = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args([
            "-C",
            repo.to_str().unwrap(),
            "log",
            "-1",
            &format!("--format={fmt}"),
        ])
        .output()
        .expect("git log");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
