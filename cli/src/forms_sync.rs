//! `navigator forms sync` / `navigator forms fields` — vendor, pin,
//! and inspect the blank government forms in the public assets bucket.
//!
//! The bucket is the only home of the blank bytes; the repo keeps the
//! diffable text, including a `.sha256` pin per form. `sync` closes the
//! loop in both directions:
//!
//! - a **local working copy** at `templates/<object_path>` (untracked —
//!   the human downloads or re-authors it there) is uploaded and its
//!   repo pin rewritten to match;
//! - **without** a working copy, the bucket object is pulled and
//!   verified against the pin — a missing object or a mismatch is a
//!   loud non-zero exit, because the fill path would refuse the same
//!   bytes.
//!
//! `fields` prints a blank's `AcroForm` `/T` names (pulled + pin-verified
//! first), the ground truth for authoring its `.fields.toml` or
//! re-authoring the field layer.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cloud::{GcsStorage, GcsStorageConfig, StorageService};

/// Long-lived cache: a pinned form object is immutable in practice (a
/// re-vendor rewrites the pin in the same PR), so downstream caches may
/// hold it.
const FORM_CACHE_CONTROL: &str = "public, max-age=604800";

/// One registry form resolved onto the local checkout: where its
/// working copy would live and where its pin file lives.
struct SyncItem {
    code: String,
    object_path: String,
    /// `templates/<object_path>` — the untracked working copy.
    local_blank: PathBuf,
    /// The tracked sibling `.sha256` pin.
    pin_file: PathBuf,
    /// The pin as compiled into the registry — the fallback when the
    /// pin file cannot be read; the file wins so a pin rewritten by a
    /// previous `sync` in this checkout verifies without a rebuild.
    pinned: String,
}

/// What `sync` did for one form.
#[derive(Debug, PartialEq, Eq)]
enum SyncOutcome {
    /// Local working copy uploaded where needed; pin file (re)written
    /// when it didn't match the working copy.
    Vendored { pin_rewritten: bool },
    /// No working copy; the bucket object matches the pin.
    Verified,
}

fn items_from_registry(workspace_root: &Path) -> anyhow::Result<Vec<SyncItem>> {
    Ok(forms::registry()?
        .into_iter()
        .map(|form| {
            let local_blank = workspace_root.join("templates").join(form.object_path);
            let pin_file = local_blank.with_extension("sha256");
            SyncItem {
                code: form.code.to_string(),
                object_path: form.object_path.to_string(),
                local_blank,
                pin_file,
                pinned: form.pinned_sha256().to_string(),
            }
        })
        .collect())
}

/// The workspace root: `sync` runs from the checkout (it rewrites pin
/// files), so walk up from the current directory to the first ancestor
/// carrying `templates/forms`.
fn workspace_root() -> anyhow::Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    cwd.ancestors()
        .find(|p| p.join("templates/forms").is_dir())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no `templates/forms` directory above {} — run from the workspace checkout",
                cwd.display()
            )
        })
}

/// Entry point for `cli forms sync`. `bucket` defaults to the
/// `NAVIGATOR_ASSETS_BUCKET` env var — the public `<project>-assets`
/// bucket, deliberately distinct from the documents bucket so blanks
/// never land in the confidential lane (and vice versa).
pub fn run_sync(bucket: Option<&str>) -> ExitCode {
    with_assets_storage("forms sync", bucket, |storage| async move {
        let items = items_from_registry(&workspace_root()?)?;
        let mut vendored = 0usize;
        let mut verified = 0usize;
        for item in &items {
            match sync_one(storage.as_ref(), item).await? {
                SyncOutcome::Vendored { pin_rewritten } => {
                    vendored += 1;
                    if pin_rewritten {
                        println!(
                            "  {}: uploaded working copy, pin rewritten at {} \
                             (rebuild to bake the new pin into the binaries)",
                            item.code,
                            item.pin_file.display()
                        );
                    } else {
                        println!("  {}: working copy in sync, pin unchanged", item.code);
                    }
                }
                SyncOutcome::Verified => {
                    verified += 1;
                    println!("  {}: bucket object matches its pin", item.code);
                }
            }
        }
        println!("navigator: forms sync: {vendored} vendored, {verified} verified");
        Ok(())
    })
}

/// Entry point for `cli forms fields <code>`: pull the blank, verify
/// its pin, and print its `AcroForm` `/T` names one per line.
pub fn run_fields(code: &str, bucket: Option<&str>) -> ExitCode {
    let code = code.to_string();
    with_assets_storage("forms fields", bucket, |storage| async move {
        let form = forms::get(&code)?
            .ok_or_else(|| anyhow::anyhow!("`{code}` is not in the vendored forms registry"))?;
        let blank = storage.get(form.object_path).await.map_err(|e| {
            anyhow::anyhow!(
                "pull `{}`: {e} — vendor the blank with `navigator forms sync`",
                form.object_path
            )
        })?;
        form.verify(&blank.bytes)?;
        for name in pdf::field_names(&blank.bytes)? {
            println!("{name}");
        }
        Ok(())
    })
}

/// Entry point for `cli forms re-author <code>` (#256 item 1): pull the
/// blank, verify its pin, and transform its field layer so the `AcroForm`
/// `/T` names *are* questionnaire state paths — the recorded judgment in
/// the form's `.fields.toml` drives every rename, radio merge, and
/// pre-printed literal, and every unmapped field lands in the
/// `unmapped__` namespace. Writes the re-authored working copy to
/// `templates/<object_path>` plus its diffable `.fields` manifest, then
/// prints the human steps that remain: visual QA of the filled blank,
/// `navigator forms sync` to vendor + re-pin, and deleting the
/// `.fields.toml` the transform just consumed.
pub fn run_reauthor(code: &str, bucket: Option<&str>) -> ExitCode {
    let code = code.to_string();
    with_assets_storage("forms re-author", bucket, |storage| async move {
        let root = workspace_root()?;
        let form = forms::get(&code)?
            .ok_or_else(|| anyhow::anyhow!("`{code}` is not in the vendored forms registry"))?;
        let map = forms::field_map(&code)?.ok_or_else(|| {
            anyhow::anyhow!(
                "`{code}` has no `.fields.toml` — its judgment layer is the transform's \
                 input, so a form without one is already re-authored (or never mapped)"
            )
        })?;

        let blank = storage.get(form.object_path).await.map_err(|e| {
            anyhow::anyhow!(
                "pull `{}`: {e} — vendor the blank with `navigator forms sync`",
                form.object_path
            )
        })?;
        // The pin file wins over the compiled-in pin, exactly like
        // `sync`, so a re-vendor earlier in this checkout verifies
        // without a rebuild.
        let local_blank = root.join("templates").join(form.object_path);
        let pin_file = local_blank.with_extension("sha256");
        let pinned = std::fs::read_to_string(&pin_file).map_or_else(
            |_| form.pinned_sha256().to_string(),
            |s| s.trim().to_string(),
        );
        forms::verify_sha256(&pinned, &blank.bytes)?;

        let states = questionnaire_states(&root, form.object_path)?;
        let names = pdf::field_names(&blank.bytes)?;
        let plan = forms::reauthor::plan(&map, &names, &states)?;
        let spec = pdf::ReauthorSpec {
            renames: plan.renames,
            radios: plan
                .radios
                .into_iter()
                .map(|(name, members)| pdf::RadioMergeSpec { name, members })
                .collect(),
            literals: plan.literals,
        };
        let reauthored = pdf::reauthor(&blank.bytes, &spec)?;

        if let Some(parent) = local_blank.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&local_blank, &reauthored)?;
        let mut manifest = pdf::field_names(&reauthored)?;
        manifest.sort();
        let manifest_path = local_blank.with_extension("fields");
        std::fs::write(&manifest_path, manifest.join("\n") + "\n")?;

        println!(
            "navigator: forms re-author: `{code}` re-authored ({} fields)",
            manifest.len()
        );
        println!("  working copy: {}", local_blank.display());
        println!("  manifest:     {}", manifest_path.display());
        println!("  next: fill the working copy with sample answers and visually QA it,");
        println!("        then `navigator forms sync` to vendor + re-pin, and delete the");
        println!("        `.fields.toml` this transform consumed.");
        Ok(())
    })
}

/// The sibling notation's declared questionnaire states — the resolution
/// target for every `.fields.toml` question reference (the same read the
/// `question_code_contract` guard performs).
fn questionnaire_states(root: &Path, object_path: &str) -> anyhow::Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct Notation {
        questionnaire: std::collections::BTreeMap<String, serde_yaml::Value>,
    }
    let md = root
        .join("templates")
        .join(object_path.replace(".pdf", ".md"));
    let contents = std::fs::read_to_string(&md)
        .map_err(|e| anyhow::anyhow!("read notation {}: {e}", md.display()))?;
    let fm = contents
        .strip_prefix("---\n")
        .and_then(|rest| rest.find("\n---").map(|end| &rest[..end]))
        .ok_or_else(|| anyhow::anyhow!("{}: no `---` frontmatter block", md.display()))?;
    let notation: Notation = serde_yaml::from_str(fm)
        .map_err(|e| anyhow::anyhow!("{}: parse frontmatter: {e}", md.display()))?;
    Ok(notation
        .questionnaire
        .into_keys()
        .filter(|s| s != "BEGIN" && s != "END")
        .collect())
}

/// Resolve the assets-lane config: `--bucket` wins; otherwise the same
/// lane resolution `web` uses — `NAVIGATOR_ASSETS_BUCKET`, falling back
/// to `NAVIGATOR_STORAGE_BUCKET` in the single-bucket KIND/dev topology.
fn assets_config<G: Fn(&str) -> Option<String>>(
    bucket: Option<&str>,
    get: G,
) -> Result<GcsStorageConfig, cloud::StorageError> {
    GcsStorageConfig::assets_from_lookup(|key| {
        if key == "NAVIGATOR_ASSETS_BUCKET" {
            if let Some(b) = bucket.map(str::trim).filter(|b| !b.is_empty()) {
                return Some(b.to_string());
            }
        }
        get(key)
    })
}

/// Shared bucket + runtime scaffolding for the `forms` subcommands.
fn with_assets_storage<F, Fut>(what: &str, bucket: Option<&str>, run: F) -> ExitCode
where
    F: FnOnce(std::sync::Arc<GcsStorage>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let Ok(cfg) = assets_config(bucket, |key| std::env::var(key).ok()) else {
        eprintln!(
            "navigator: {what}: no bucket — pass --bucket or set NAVIGATOR_ASSETS_BUCKET \
             (or NAVIGATOR_STORAGE_BUCKET)"
        );
        return ExitCode::from(2);
    };
    let bucket = cfg.bucket.clone();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("navigator: {what}: tokio runtime: {e}");
            return ExitCode::from(2);
        }
    };
    runtime.block_on(async move {
        let storage = match GcsStorage::new_from_config(cfg).await {
            Ok(s) => std::sync::Arc::new(s),
            Err(e) => {
                eprintln!("navigator: {what}: open bucket `{bucket}`: {e}");
                return ExitCode::from(2);
            }
        };
        match run(storage).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("navigator: {what}: {e:#}");
                ExitCode::from(2)
            }
        }
    })
}

/// Sync one form. With a local working copy: upload when the bucket
/// bytes differ (or are absent) and rewrite the pin file when it does
/// not match the working copy. Without one: pull + verify against the
/// pin, erroring loudly on a missing object or a mismatch.
async fn sync_one(storage: &dyn StorageService, item: &SyncItem) -> anyhow::Result<SyncOutcome> {
    if item.local_blank.is_file() {
        let bytes = std::fs::read(&item.local_blank)?;
        let digest = forms::sha256_hex(&bytes);
        let bucket_matches = match storage.get(&item.object_path).await {
            Ok(existing) => forms::sha256_hex(&existing.bytes) == digest,
            Err(cloud::StorageError::NotFound(_)) => false,
            Err(e) => return Err(e.into()),
        };
        if !bucket_matches {
            storage
                .put_cached(
                    &item.object_path,
                    &bytes,
                    "application/pdf",
                    FORM_CACHE_CONTROL,
                )
                .await?;
        }
        let pin_on_disk = std::fs::read_to_string(&item.pin_file)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let pin_rewritten = pin_on_disk != digest;
        if pin_rewritten {
            std::fs::write(&item.pin_file, format!("{digest}\n"))?;
        }
        return Ok(SyncOutcome::Vendored { pin_rewritten });
    }

    // No working copy: verify the bucket against the pin.
    let pinned = std::fs::read_to_string(&item.pin_file)
        .map_or_else(|_| item.pinned.clone(), |s| s.trim().to_string());
    let blank = storage.get(&item.object_path).await.map_err(|e| {
        anyhow::anyhow!(
            "{}: no working copy at {} and the bucket pull failed: {e} — \
             download the blank from the form's origin_url and re-run",
            item.code,
            item.local_blank.display()
        )
    })?;
    forms::verify_sha256(&pinned, &blank.bytes).map_err(|e| {
        anyhow::anyhow!(
            "{}: bucket object `{}` fails its pin: {e} — the blank was \
             re-vendored without updating {}; the fill path will refuse it",
            item.code,
            item.object_path,
            item.pin_file.display()
        )
    })?;
    Ok(SyncOutcome::Verified)
}

#[cfg(test)]
mod tests {
    use super::{assets_config, sync_one, SyncItem, SyncOutcome};
    use cloud::StorageService;

    #[test]
    fn assets_config_prefers_flag_then_assets_then_storage_bucket() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                vars.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).to_string())
            }
        };
        // --bucket wins over both env vars.
        let cfg = assets_config(
            Some("flag-bucket"),
            env(&[
                ("NAVIGATOR_ASSETS_BUCKET", "assets-bucket"),
                ("NAVIGATOR_STORAGE_BUCKET", "storage-bucket"),
            ]),
        )
        .unwrap();
        assert_eq!(cfg.bucket, "flag-bucket");
        // No flag: the assets bucket, then — single-bucket KIND/dev —
        // the same NAVIGATOR_STORAGE_BUCKET fallback `web` uses.
        let cfg =
            assets_config(None, env(&[("NAVIGATOR_STORAGE_BUCKET", "storage-bucket")])).unwrap();
        assert_eq!(cfg.bucket, "storage-bucket");
        // A blank flag is not a bucket.
        assert!(assets_config(Some("  "), env(&[])).is_err());
    }

    fn item(dir: &std::path::Path, with_blank: Option<&[u8]>, pinned: &str) -> SyncItem {
        let local_blank = dir.join("nv__test.pdf");
        if let Some(bytes) = with_blank {
            std::fs::write(&local_blank, bytes).unwrap();
        }
        SyncItem {
            code: "nv__test".into(),
            object_path: "forms/united_states/nevada/state/nv__test.pdf".into(),
            local_blank,
            pin_file: dir.join("nv__test.sha256"),
            pinned: pinned.into(),
        }
    }

    async fn fs_storage(tag: &str) -> cloud::FsStorage {
        cloud::FsStorage::new(std::env::temp_dir().join(format!(
            "navigator-forms-sync-{tag}-{}",
            uuid::Uuid::new_v4()
        )))
        .await
        .expect("temp FsStorage")
    }

    fn temp_repo(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "navigator-forms-sync-repo-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn a_working_copy_uploads_and_writes_the_pin_then_reverifies() {
        let storage = fs_storage("vendor").await;
        let dir = temp_repo("vendor");
        let blank = b"%PDF-1.5 working copy";
        let it = item(&dir, Some(blank), "");

        // First run vendors: uploads + writes the pin file.
        let outcome = sync_one(&storage, &it).await.unwrap();
        assert_eq!(
            outcome,
            SyncOutcome::Vendored {
                pin_rewritten: true
            }
        );
        let pin = std::fs::read_to_string(&it.pin_file).unwrap();
        assert_eq!(pin.trim(), forms::sha256_hex(blank));
        assert!(storage.exists(&it.object_path).await.unwrap());

        // Second run with the working copy still present: idempotent
        // (bucket bytes match, pin unchanged).
        let outcome = sync_one(&storage, &it).await.unwrap();
        assert_eq!(
            outcome,
            SyncOutcome::Vendored {
                pin_rewritten: false
            }
        );

        // Remove the working copy: the bucket verifies against the pin
        // file the first run wrote.
        std::fs::remove_file(&it.local_blank).unwrap();
        let outcome = sync_one(&storage, &it).await.unwrap();
        assert_eq!(outcome, SyncOutcome::Verified);
    }

    #[tokio::test]
    async fn a_missing_bucket_object_without_a_working_copy_fails_loudly() {
        let storage = fs_storage("missing").await;
        let dir = temp_repo("missing");
        let it = item(&dir, None, &forms::sha256_hex(b"whatever"));
        let err = sync_one(&storage, &it).await.unwrap_err();
        assert!(err.to_string().contains("no working copy"), "{err:#}");
    }

    #[tokio::test]
    async fn a_pin_mismatch_fails_loudly_instead_of_repinning() {
        let storage = fs_storage("mismatch").await;
        let dir = temp_repo("mismatch");
        let it = item(&dir, None, &forms::sha256_hex(b"the pinned blank"));
        storage
            .put(&it.object_path, b"silently re-vendored", "application/pdf")
            .await
            .unwrap();
        let err = sync_one(&storage, &it).await.unwrap_err();
        assert!(err.to_string().contains("fails its pin"), "{err:#}");
        assert!(
            !it.pin_file.exists(),
            "verify-only mode must never rewrite the pin"
        );
    }
}
