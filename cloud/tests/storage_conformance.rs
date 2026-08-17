//! Provider-neutral `StorageService` conformance suite.

use std::time::Duration;

use cloud::{StorageError, StorageService};

async fn contract(storage: &dyn StorageService) {
    let prefix = format!("conformance/{}/", std::process::id());
    let first = format!("{prefix}a.txt");
    let second = format!("{prefix}b.bin");
    let missing = format!("{prefix}missing");

    assert!(!storage.exists(&missing).await.expect("HEAD missing object"));
    assert!(matches!(
        storage.get(&missing).await,
        Err(StorageError::NotFound(_))
    ));

    storage
        .put_cached(
            &first,
            b"garage-conformance",
            "text/plain",
            "private, max-age=60",
        )
        .await
        .expect("put cached object");
    storage
        .put(&second, b"\0\x01\x02", "application/octet-stream")
        .await
        .expect("put binary object");
    assert!(storage.exists(&first).await.expect("HEAD existing object"));
    let object = storage.get(&first).await.expect("get object");
    assert_eq!(object.bytes, b"garage-conformance");
    assert_eq!(object.content_type, "text/plain");

    let listed = storage.list(&prefix).await.expect("list prefix");
    assert_eq!(
        listed
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        [&first, &second]
    );

    let url = storage
        .signed_url(&first, Duration::from_mins(1))
        .await
        .expect("presigned GET");
    let response = reqwest::get(url).await.expect("fetch presigned GET");
    assert!(response.status().is_success());
    assert_eq!(response.headers()["content-type"], "text/plain");
    assert_eq!(response.headers()["cache-control"], "private, max-age=60");
    assert_eq!(
        response.bytes().await.unwrap(),
        b"garage-conformance".as_slice()
    );

    storage.delete(&first).await.expect("delete first object");
    storage.delete(&second).await.expect("delete second object");
    storage
        .delete(&second)
        .await
        .expect("delete remains idempotent");
    assert!(!storage.exists(&first).await.expect("HEAD deleted object"));
    assert!(storage
        .list(&prefix)
        .await
        .expect("clean prefix")
        .is_empty());
}

#[tokio::test]
async fn filesystem_contract() {
    let root = tempfile::TempDir::new().unwrap();
    let storage = cloud::FsStorage::new(root.path()).await.unwrap();
    // Filesystem cannot issue signed URLs; its remaining contract is covered
    // by the backend unit tests while the live test below exercises signing.
    storage.put("contract/a", b"a", "text/plain").await.unwrap();
    assert_eq!(storage.get("contract/a").await.unwrap().bytes, b"a");
    storage.delete("contract/a").await.unwrap();
}

#[tokio::test]
async fn live_s3_contract_when_requested() {
    if std::env::var("NAVIGATOR_LIVE_S3").as_deref() != Ok("1") {
        return;
    }
    let storage = cloud::from_env().await.expect("open live S3 storage");
    contract(storage.as_ref()).await;
}

#[tokio::test]
async fn live_s3_lane_isolation_when_requested() {
    if std::env::var("NAVIGATOR_LIVE_S3").as_deref() != Ok("1") {
        return;
    }
    let config = cloud::S3StorageConfig {
        bucket: std::env::var("NAVIGATOR_STORAGE_BUCKET").unwrap(),
        endpoint: std::env::var("NAVIGATOR_STORAGE_ENDPOINT").unwrap(),
        region: std::env::var("NAVIGATOR_STORAGE_REGION").unwrap(),
        access_key: std::env::var("NAVIGATOR_LIVE_S3_DENIED_ACCESS_KEY").unwrap(),
        secret_key: std::env::var("NAVIGATOR_LIVE_S3_DENIED_SECRET_KEY").unwrap(),
        session_token: None,
    };
    let storage = cloud::S3Storage::new(config).unwrap();
    let result = storage.exists("conformance/lane-isolation-probe").await;
    assert!(
        matches!(result, Err(StorageError::S3 { .. })),
        "cross-lane access must be denied: {result:?}"
    );
}
