//! Governed-expunge orchestration: admin gate + history rewrite +
//! object-storage deletion + audit record, end to end.
//!
//! Multi-thread runtime: the rewrite shells `git` via `spawn_blocking`.

use std::process::Command;
use std::sync::Arc;

use cloud::StorageService;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use store::entity::person::Role;
use store::entity::{expunge_record, person, project};
use store::test_support::pg;
use store::Db;

async fn a_person(db: &Db, name: &str, email: &str, role: Role) -> uuid::Uuid {
    person::ActiveModel {
        name: ActiveValue::Set(name.into()),
        email: ActiveValue::Set(email.into()),
        role: ActiveValue::Set(role),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn governed_expunge_rewrites_deletes_and_records() {
    let repo_root = tempfile::tempdir().unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", repo_root.path());

    let db = pg().await;
    let storage: Arc<dyn StorageService> = Arc::new(
        cloud::FsStorage::new(
            std::env::temp_dir().join(format!("nav-expunge-{}", uuid::Uuid::now_v7())),
        )
        .await
        .unwrap(),
    );

    let admin = a_person(&db, "Nick", "nick@neonlaw.com", Role::Admin).await;
    let client = a_person(&db, "Aries", "aries@example.com", Role::Client).await;
    let __dri = store::test_support::dri_person(&db).await;
    let proj = project::ActiveModel {
        name: ActiveValue::Set("Matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;

    // Commit a privileged doc + a kept doc into the repo, and stash the
    // privileged bytes in object storage at a blobs/<sha> key.
    let repo_store = repos::RepoStore::new(repo_root.path());
    repo_store
        .commit_as(
            proj,
            repos::Author {
                name: "Aries",
                email: "aries@example.com",
            },
            "file docs",
            &[
                ("privileged.pdf", b"privileged material"),
                ("keep.pdf", b"ordinary doc"),
            ],
        )
        .unwrap();
    let object_key = "blobs/deadbeefdeadbeef";
    storage
        .put(object_key, b"privileged material", "application/pdf")
        .await
        .unwrap();

    // A non-admin may NOT expunge — and nothing is touched.
    let denied = web::expunge::expunge(
        &db,
        &storage,
        web::expunge::ExpungeRequest {
            project_id: proj,
            path: "privileged.pdf",
            category: expunge_record::CATEGORY_PRIVILEGE,
            authorized_by: client,
            storage_key: Some(object_key),
            note: None,
        },
    )
    .await;
    assert!(matches!(denied, Err(web::expunge::ExpungeError::NotAdmin)));
    let repo = repo_store.path_for(proj);
    assert_eq!(
        git_show(&repo, "show main:privileged.pdf"),
        "privileged material"
    );
    assert!(
        storage.get(object_key).await.is_ok(),
        "no deletion on a denied expunge"
    );

    // The admin expunges: history rewritten, bytes deleted, audit row
    // written.
    let record_id = web::expunge::expunge(
        &db,
        &storage,
        web::expunge::ExpungeRequest {
            project_id: proj,
            path: "privileged.pdf",
            category: expunge_record::CATEGORY_SEALING,
            authorized_by: admin,
            storage_key: Some(object_key),
            note: Some("sealed per docket 24-CV-1"),
        },
    )
    .await
    .expect("admin expunge succeeds");

    // (2) gone from history, kept doc survives.
    let history = git_show(&repo, "log --all --oneline -- privileged.pdf");
    assert!(
        history.is_empty(),
        "privileged doc still in history: {history}"
    );
    assert_eq!(git_show(&repo, "show main:keep.pdf"), "ordinary doc");

    // (3) bytes gone from object storage.
    assert!(matches!(
        storage.get(object_key).await,
        Err(cloud::StorageError::NotFound(_))
    ));

    // (4) audit row records who/when/category, not content.
    let row = expunge_record::Entity::find_by_id(record_id)
        .one(&db)
        .await
        .unwrap()
        .expect("expunge record");
    assert_eq!(row.category, expunge_record::CATEGORY_SEALING);
    assert_eq!(row.authorized_by_person_id, admin);
    assert_eq!(row.project_id, proj);
    assert!(row.head_after.is_some());

    std::env::remove_var("NAVIGATOR_GIT_REPO_ROOT");
}

fn git_show(repo: &std::path::Path, args: &str) -> String {
    let split: Vec<&str> = args.split(' ').collect();
    let out = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .arg("-C")
        .arg(repo)
        .args(&split)
        .output()
        .expect("git");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
