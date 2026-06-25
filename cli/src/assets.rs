//! `cli assets build` — transcode curated source photos into the
//! responsive web variants that [`views::assets::responsive_picture`]
//! points at.
//!
//! The manifest ([`views::assets::GALLERY`]) and the width set
//! ([`views::assets::WIDTHS`]) are the single source of truth, shared
//! with the view layer — so adding a photo is a manifest edit, never a
//! change here. For each photo we decode the source JPEG once, then
//! emit every width as AVIF, lossy WebP, and JPEG (the three formats
//! the `<picture>` element negotiates, smallest first). Output lands
//! under `<out>/img/<slug>/<slug>-<width>w.<ext>`, which is exactly
//! what the `/public` mount (and, in production, the CDN bucket) serves.

use std::path::Path;
use std::process::ExitCode;

use anyhow::Context;
use cloud::{GcsStorage, GcsStorageConfig, StorageService};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder};
use rgb::FromSlice;
use views::assets::{GALLERY, WIDTHS};

/// `Cache-Control` stamped on every uploaded variant: cacheable by any
/// shared cache for ~1 week. Crucially **not** `immutable` — the
/// variant URLs carry no cache-bust token (`views::assets` dropped
/// `?v=`), so `immutable` would turn "stale for a week" into "stale
/// forever." A bounded max-age means a re-`build` + re-`upload` is
/// picked up once the week elapses.
const ASSET_CACHE_CONTROL: &str = "public, max-age=604800";

/// JPEG quality (0–100). 82 is a good photographic sweet spot —
/// visually lossless at typical viewing sizes without bloating bytes.
const JPEG_QUALITY: u8 = 82;

/// WebP quality (0–100). WebP at 80 typically lands ~30% under the
/// equivalent JPEG with no visible difference.
const WEBP_QUALITY: f32 = 80.0;

/// AVIF quality (0–100). 70 is a sound web default — AVIF at this
/// quality typically lands ~20–30% under the equivalent WebP.
const AVIF_QUALITY: f32 = 70.0;

/// AVIF encoder speed (0 slowest/smallest – 10 fastest/largest). 6
/// keeps the whole gallery encode under a minute while staying near
/// the small-file end of the curve.
const AVIF_SPEED: u8 = 6;

/// Entry point for `cli assets build`.
pub fn run_build(src: &Path, out: &Path) -> ExitCode {
    match build(src, out) {
        Ok(variants) => {
            println!(
                "navigator: built {variants} variant(s) for {} photo(s) under {}",
                GALLERY.len(),
                out.join("img").display(),
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("navigator: assets build: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn build(src: &Path, out: &Path) -> anyhow::Result<usize> {
    let img_root = out.join("img");
    let mut variants = 0usize;
    for g in GALLERY {
        let src_path = src.join(g.source);
        let decoded = image::open(&src_path)
            .with_context(|| format!("open source `{}`", src_path.display()))?;
        let (ow, oh) = (decoded.width(), decoded.height());
        anyhow::ensure!(
            ow > 0 && oh > 0,
            "source `{}` has zero dimension",
            src_path.display()
        );

        let dir = img_root.join(g.slug);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("create output dir `{}`", dir.display()))?;

        for &w in &WIDTHS {
            // Preserve the photo's native aspect; CSS `object-fit:cover`
            // crops to the display ratio box, so the stored variant is
            // never letterboxed or distorted here.
            let h = u32::try_from(u64::from(w) * u64::from(oh) / u64::from(ow))
                .unwrap_or(u32::MAX)
                .max(1);
            let rgb = decoded.resize_exact(w, h, FilterType::Lanczos3).to_rgb8();

            // ── JPEG (universal fallback) ──
            let jpg = dir.join(format!("{}-{w}w.jpg", g.slug));
            let file = std::fs::File::create(&jpg)
                .with_context(|| format!("create `{}`", jpg.display()))?;
            JpegEncoder::new_with_quality(std::io::BufWriter::new(file), JPEG_QUALITY)
                .write_image(rgb.as_raw(), w, h, ExtendedColorType::Rgb8)
                .with_context(|| format!("encode `{}`", jpg.display()))?;

            // ── WebP (smaller, modern browsers) ──
            let webp_path = dir.join(format!("{}-{w}w.webp", g.slug));
            let encoded = webp::Encoder::from_rgb(rgb.as_raw(), w, h).encode(WEBP_QUALITY);
            std::fs::write(&webp_path, &*encoded)
                .with_context(|| format!("write `{}`", webp_path.display()))?;

            // ── AVIF (smallest; the negotiated first choice) ──
            let avif_path = dir.join(format!("{}-{w}w.avif", g.slug));
            let avif = ravif::Encoder::new()
                .with_quality(AVIF_QUALITY)
                .with_speed(AVIF_SPEED)
                .encode_rgb(ravif::Img::new(
                    rgb.as_raw().as_rgb(),
                    w as usize,
                    h as usize,
                ))
                .with_context(|| format!("encode `{}`", avif_path.display()))?;
            std::fs::write(&avif_path, &avif.avif_file)
                .with_context(|| format!("write `{}`", avif_path.display()))?;

            variants += 3;
        }
        println!(
            "  {:<24} {ow}x{oh} → {} widths × (avif, webp, jpg)",
            g.slug,
            WIDTHS.len()
        );
    }
    Ok(variants)
}

/// Entry point for `cli assets upload`. `bucket` defaults to the
/// `NAVIGATOR_ASSETS_BUCKET` env var (the public `<project>-assets`
/// bucket, distinct from the app's documents bucket
/// `NAVIGATOR_DOCUMENTS_BUCKET`) so an upload can never accidentally
/// write photos into the documents bucket.
pub fn run_upload(dir: &Path, bucket: Option<String>) -> ExitCode {
    let bucket = match bucket.or_else(|| std::env::var("NAVIGATOR_ASSETS_BUCKET").ok()) {
        Some(b) if !b.trim().is_empty() => b,
        _ => {
            eprintln!(
                "navigator: assets upload: no bucket — pass --bucket or set NAVIGATOR_ASSETS_BUCKET"
            );
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("navigator: assets upload: tokio runtime: {e}");
            return ExitCode::from(2);
        }
    };
    runtime.block_on(async move {
        // Reuse the documents backend's endpoint override (emulator
        // support) but point at the assets bucket; ADC auth otherwise.
        // An empty value resolves to `None` (real GCS) so a dev shell
        // that exports `NAVIGATOR_STORAGE_ENDPOINT=` to shadow the
        // `.devx/env` fake-gcs overlay uploads to the real bucket.
        let cfg = GcsStorageConfig {
            bucket: bucket.clone(),
            endpoint: std::env::var("NAVIGATOR_STORAGE_ENDPOINT")
                .ok()
                .filter(|s| !s.trim().is_empty()),
        };
        let storage = match GcsStorage::new_from_config(cfg).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("navigator: assets upload: open bucket `{bucket}`: {e}");
                return ExitCode::from(2);
            }
        };
        match upload(&storage, dir).await {
            Ok(n) => {
                println!("navigator: uploaded {n} variant(s) to gs://{bucket}/img");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("navigator: assets upload: {e:#}");
                ExitCode::from(2)
            }
        }
    })
}

/// Entry point for `cli assets pull` — the inverse of `upload`, for
/// local development. `web/public/img/` is gitignored (photos live only
/// in the public assets bucket, never in git, never baked into the
/// image), so a fresh clone serves empty photo slots. This downloads
/// every built variant under the bucket's `img/` prefix into `out`
/// (default `web/public/img`) so the `/public` mount has the photos
/// again — no source JPEGs or a re-`build` required. Read-only against
/// the bucket; `bucket` defaults to `NAVIGATOR_ASSETS_BUCKET`.
pub fn run_pull(out: &Path, bucket: Option<String>) -> ExitCode {
    let bucket = match bucket.or_else(|| std::env::var("NAVIGATOR_ASSETS_BUCKET").ok()) {
        Some(b) if !b.trim().is_empty() => b,
        _ => {
            eprintln!(
                "navigator: assets pull: no bucket — pass --bucket or set NAVIGATOR_ASSETS_BUCKET"
            );
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("navigator: assets pull: tokio runtime: {e}");
            return ExitCode::from(2);
        }
    };
    runtime.block_on(async move {
        // Same endpoint override as `upload` (emulator support), pointed
        // at the assets bucket; ADC auth otherwise.
        let cfg = GcsStorageConfig {
            bucket: bucket.clone(),
            endpoint: std::env::var("NAVIGATOR_STORAGE_ENDPOINT")
                .ok()
                .filter(|s| !s.trim().is_empty()),
        };
        let storage = match GcsStorage::new_from_config(cfg).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("navigator: assets pull: open bucket `{bucket}`: {e}");
                return ExitCode::from(2);
            }
        };
        match download(&storage, out).await {
            Ok(n) => {
                println!(
                    "navigator: pulled {n} variant(s) from gs://{bucket}/img into {}",
                    out.display()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("navigator: assets pull: {e:#}");
                ExitCode::from(2)
            }
        }
    })
}

/// The content type for an asset under `web/public/img/`, keyed off its
/// extension. The three formats `cli assets build` emits (AVIF/WebP/JPEG)
/// plus `png` for hand-authored blog/illustration heroes are carried;
/// anything else under `dir` (a stray `.DS_Store`, an editor temp file)
/// is skipped rather than pushed with a wrong type.
fn content_type_for(ext: &str) -> Option<&'static str> {
    match ext {
        "avif" => Some("image/avif"),
        "webp" => Some("image/webp"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        // Not a `build` re-encode variant — `png` carries hand-authored
        // blog/illustration assets dropped straight under
        // `web/public/img/<slug>/` (e.g. a painted hero), where JPEG's
        // ringing on sharp edges would show. `upload`/`pull` carry the
        // bytes through untouched.
        "png" => Some("image/png"),
        _ => None,
    }
}

/// Walk `dir` and `put_cached` every recognized image variant under the
/// key `img/<path-relative-to-dir>` (e.g. `img/lake-tahoe/lake-tahoe-400w.avif`).
/// Decoupled from backend construction so tests drive it against the
/// `Fs` backend. Returns the count of objects uploaded.
async fn upload(storage: &dyn StorageService, dir: &Path) -> anyhow::Result<usize> {
    anyhow::ensure!(
        dir.is_dir(),
        "asset directory `{}` does not exist — run `cli assets build` first",
        dir.display()
    );
    let mut uploaded = 0usize;
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = entry.with_context(|| format!("walk `{}`", dir.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let Some(content_type) = content_type_for(&ext) else {
            continue;
        };
        let rel = path
            .strip_prefix(dir)
            .with_context(|| format!("`{}` not under `{}`", path.display(), dir.display()))?;
        // Keys always use `/`; build from components so a Windows host
        // doesn't emit backslash-separated keys.
        let rel_key = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let key = format!("img/{rel_key}");
        let bytes = std::fs::read(path).with_context(|| format!("read `{}`", path.display()))?;
        storage
            .put_cached(&key, &bytes, content_type, ASSET_CACHE_CONTROL)
            .await
            .with_context(|| format!("upload `{key}`"))?;
        println!("  → {key} ({content_type}, {} bytes)", bytes.len());
        uploaded += 1;
    }
    Ok(uploaded)
}

/// List the bucket's `img/` prefix and write each built variant to
/// `out/<key-without-"img/">` — the inverse of [`upload`]'s keying, so a
/// pulled tree is byte-identical to what `build` would produce. Skips
/// any object that isn't one of the three built formats (defensive: the
/// bucket's `img/` lane only ever holds variants). Decoupled from
/// backend construction so tests drive it against the `Fs` backend.
/// Returns the count of variants written.
async fn download(storage: &dyn StorageService, out: &Path) -> anyhow::Result<usize> {
    let listings = storage
        .list("img/")
        .await
        .context("list the bucket's `img/` prefix")?;
    let mut listed_under_img = 0usize;
    let mut pulled = 0usize;
    for listing in listings {
        let key = listing.key;
        let Some(rel) = key.strip_prefix("img/").filter(|r| !r.is_empty()) else {
            continue;
        };
        listed_under_img += 1;
        let ext = Path::new(rel)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if content_type_for(&ext).is_none() {
            continue;
        }
        // Rebuild the destination from `/`-separated key segments,
        // refusing empty/`.`/`..` so a malformed key can't escape `out`.
        let mut dest = out.to_path_buf();
        for seg in rel.split('/') {
            anyhow::ensure!(
                !seg.is_empty() && seg != "." && seg != "..",
                "refusing unsafe object key `{key}`"
            );
            dest.push(seg);
        }
        let obj = storage
            .get(&key)
            .await
            .with_context(|| format!("download `{key}`"))?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create `{}`", parent.display()))?;
        }
        std::fs::write(&dest, &obj.bytes).with_context(|| format!("write `{}`", dest.display()))?;
        println!("  ← {key} → {} ({} bytes)", dest.display(), obj.bytes.len());
        pulled += 1;
    }
    anyhow::ensure!(
        pulled > 0,
        "{}",
        if listed_under_img == 0 {
            "no objects under `img/` in the bucket — populate it first with \
             `cli assets build` + `cli assets upload`"
        } else {
            "objects exist under `img/`, but none are supported image variants \
             (.avif, .webp, .jpg, .jpeg, .png)"
        }
    );
    Ok(pulled)
}

#[cfg(test)]
mod tests {
    use super::{content_type_for, download, upload, ASSET_CACHE_CONTROL};
    use cloud::{FsStorage, ObjectListing, StorageError, StorageService, StoredObject};
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    struct ListingOnlyStorage {
        keys: Vec<String>,
    }

    #[async_trait::async_trait]
    impl StorageService for ListingOnlyStorage {
        async fn put(
            &self,
            _key: &str,
            _bytes: &[u8],
            _content_type: &str,
        ) -> Result<(), StorageError> {
            Err(StorageError::Unsupported("ListingOnlyStorage put"))
        }

        async fn get(&self, key: &str) -> Result<StoredObject, StorageError> {
            Ok(StoredObject {
                key: key.to_string(),
                bytes: b"bytes".to_vec(),
                content_type: "image/avif".to_string(),
            })
        }

        async fn delete(&self, _key: &str) -> Result<(), StorageError> {
            Err(StorageError::Unsupported("ListingOnlyStorage delete"))
        }

        async fn list(&self, prefix: &str) -> Result<Vec<ObjectListing>, StorageError> {
            Ok(self
                .keys
                .iter()
                .filter(|key| key.starts_with(prefix))
                .map(|key| ObjectListing {
                    key: key.clone(),
                    size_bytes: 1,
                })
                .collect())
        }

        async fn signed_url(
            &self,
            _key: &str,
            _expires_in: Duration,
        ) -> Result<String, StorageError> {
            Err(StorageError::Unsupported(
                "ListingOnlyStorage has no signed URL",
            ))
        }
    }

    #[test]
    fn content_type_covers_the_built_formats_plus_png_and_skips_others() {
        assert_eq!(content_type_for("avif"), Some("image/avif"));
        assert_eq!(content_type_for("webp"), Some("image/webp"));
        assert_eq!(content_type_for("jpg"), Some("image/jpeg"));
        assert_eq!(content_type_for("jpeg"), Some("image/jpeg"));
        // PNG rides the same lane for hand-authored blog/illustration heroes.
        assert_eq!(content_type_for("png"), Some("image/png"));
        assert_eq!(content_type_for("txt"), None);
        assert_eq!(content_type_for("DS_Store"), None);
    }

    #[tokio::test]
    async fn upload_keys_each_variant_under_img_and_skips_non_images() {
        // Lay out a slug directory the way `cli assets build` does,
        // plus a stray non-image file that must not be uploaded.
        let dir = TempDir::new().unwrap();
        let slug = dir.path().join("lake-tahoe");
        fs::create_dir_all(&slug).unwrap();
        fs::write(slug.join("lake-tahoe-400w.avif"), b"avif").unwrap();
        fs::write(slug.join("lake-tahoe-400w.webp"), b"webp").unwrap();
        fs::write(slug.join("lake-tahoe-400w.jpg"), b"jpg").unwrap();
        fs::write(slug.join(".DS_Store"), b"junk").unwrap();

        let store_dir = TempDir::new().unwrap();
        let storage = FsStorage::new(store_dir.path().to_path_buf())
            .await
            .unwrap();
        let n = upload(&storage, dir.path()).await.unwrap();
        assert_eq!(
            n, 3,
            "the three image variants upload, the stray file does not"
        );

        // Keys are `img/<slug>/<file>`; the default `put_cached` on the
        // Fs backend falls back to `put`, so the bytes round-trip.
        let got = storage
            .get("img/lake-tahoe/lake-tahoe-400w.avif")
            .await
            .unwrap();
        assert_eq!(got.bytes, b"avif");
        assert_eq!(got.content_type, "image/avif");
        // The non-image stray was never stored under any key.
        assert!(storage.get("img/lake-tahoe/.DS_Store").await.is_err());
    }

    #[tokio::test]
    async fn upload_errors_when_dir_is_missing() {
        let store_dir = TempDir::new().unwrap();
        let storage = FsStorage::new(store_dir.path().to_path_buf())
            .await
            .unwrap();
        let missing = store_dir.path().join("no-such-img-tree");
        let err = upload(&storage, &missing).await.unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn cache_control_is_bounded_not_immutable() {
        // The variant URLs carry no `?v=` token, so the TTL must be
        // bounded — `immutable` would pin a stale photo forever.
        assert!(ASSET_CACHE_CONTROL.contains("max-age=604800"));
        assert!(!ASSET_CACHE_CONTROL.contains("immutable"));
    }

    #[tokio::test]
    async fn download_restores_each_variant_under_out_stripping_the_img_prefix() {
        // Seed the bucket the way `upload` keys it: `img/<slug>/<file>`,
        // plus a stray non-image key that `download` must skip.
        let store_dir = TempDir::new().unwrap();
        let storage = FsStorage::new(store_dir.path().to_path_buf())
            .await
            .unwrap();
        storage
            .put("img/lake-tahoe/lake-tahoe-400w.avif", b"avif", "image/avif")
            .await
            .unwrap();
        storage
            .put("img/lake-tahoe/lake-tahoe-400w.webp", b"webp", "image/webp")
            .await
            .unwrap();
        storage
            .put("img/lake-tahoe/lake-tahoe-400w.jpg", b"jpg", "image/jpeg")
            .await
            .unwrap();
        storage
            .put("img/lake-tahoe/notes.txt", b"junk", "text/plain")
            .await
            .unwrap();

        let out = TempDir::new().unwrap();
        let n = download(&storage, out.path()).await.unwrap();
        assert_eq!(
            n, 3,
            "the three image variants land, the stray file does not"
        );

        // The `img/` prefix is stripped; bytes round-trip under `out`.
        assert_eq!(
            fs::read(out.path().join("lake-tahoe/lake-tahoe-400w.avif")).unwrap(),
            b"avif"
        );
        assert_eq!(
            fs::read(out.path().join("lake-tahoe/lake-tahoe-400w.webp")).unwrap(),
            b"webp"
        );
        assert_eq!(
            fs::read(out.path().join("lake-tahoe/lake-tahoe-400w.jpg")).unwrap(),
            b"jpg"
        );
        // The non-image was never written.
        assert!(!out.path().join("lake-tahoe/notes.txt").exists());
    }

    #[tokio::test]
    async fn download_errors_when_the_bucket_has_no_variants() {
        let store_dir = TempDir::new().unwrap();
        let storage = FsStorage::new(store_dir.path().to_path_buf())
            .await
            .unwrap();
        let out = TempDir::new().unwrap();
        let err = download(&storage, out.path()).await.unwrap_err();
        assert!(
            err.to_string().contains("no objects under `img/`"),
            "empty bucket should guide the user, got: {err}"
        );
    }

    #[tokio::test]
    async fn download_rejects_unsafe_object_keys() {
        let storage = ListingOnlyStorage {
            keys: vec!["img/../../../etc/passwd.avif".to_string()],
        };
        let out = TempDir::new().unwrap();
        let err = download(&storage, out.path()).await.unwrap_err();
        assert!(
            err.to_string().contains("refusing unsafe object key"),
            "unsafe object key should fail before writing outside out, got: {err}"
        );
        assert!(
            !out.path().join("etc/passwd.avif").exists(),
            "unsafe key must not be written under the output directory"
        );
    }

    #[tokio::test]
    async fn download_distinguishes_non_image_objects_from_empty_bucket() {
        let store_dir = TempDir::new().unwrap();
        let storage = FsStorage::new(store_dir.path().to_path_buf())
            .await
            .unwrap();
        storage
            .put("img/lake-tahoe/notes.txt", b"junk", "text/plain")
            .await
            .unwrap();
        let out = TempDir::new().unwrap();
        let err = download(&storage, out.path()).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("none are supported image variants"),
            "non-image objects should get a precise diagnostic, got: {err}"
        );
    }

    #[tokio::test]
    async fn upload_then_download_round_trips_the_tree_byte_for_byte() {
        // Build a slug dir, upload it to an Fs-backed bucket, then pull
        // it into a fresh dir — the result is identical to the source.
        let src = TempDir::new().unwrap();
        let slug = src.path().join("lantana");
        fs::create_dir_all(&slug).unwrap();
        fs::write(slug.join("lantana-800w.avif"), b"AVIF-bytes").unwrap();
        fs::write(slug.join("lantana-800w.jpg"), b"JPEG-bytes").unwrap();

        let store_dir = TempDir::new().unwrap();
        let storage = FsStorage::new(store_dir.path().to_path_buf())
            .await
            .unwrap();
        assert_eq!(upload(&storage, src.path()).await.unwrap(), 2);

        let out = TempDir::new().unwrap();
        assert_eq!(download(&storage, out.path()).await.unwrap(), 2);
        assert_eq!(
            fs::read(out.path().join("lantana/lantana-800w.avif")).unwrap(),
            b"AVIF-bytes"
        );
        assert_eq!(
            fs::read(out.path().join("lantana/lantana-800w.jpg")).unwrap(),
            b"JPEG-bytes"
        );
    }

    #[tokio::test]
    async fn png_blog_image_uploads_and_round_trips_with_its_content_type() {
        // A hand-authored blog hero is a raw `.png` dropped straight
        // under `web/public/img/<slug>/` — not a `build` variant. It must
        // upload (keyed under `img/`, content-type `image/png`) and pull
        // back byte-for-byte so a fresh clone serves it from `/public`.
        let src = TempDir::new().unwrap();
        let slug = src.path().join("going-all-in-on-rust");
        fs::create_dir_all(&slug).unwrap();
        fs::write(slug.join("ferris.png"), b"PNG-bytes").unwrap();

        let store_dir = TempDir::new().unwrap();
        let storage = FsStorage::new(store_dir.path().to_path_buf())
            .await
            .unwrap();
        assert_eq!(upload(&storage, src.path()).await.unwrap(), 1);

        let stored = storage
            .get("img/going-all-in-on-rust/ferris.png")
            .await
            .unwrap();
        assert_eq!(stored.bytes, b"PNG-bytes");
        assert_eq!(stored.content_type, "image/png");

        let out = TempDir::new().unwrap();
        assert_eq!(download(&storage, out.path()).await.unwrap(), 1);
        assert_eq!(
            fs::read(out.path().join("going-all-in-on-rust/ferris.png")).unwrap(),
            b"PNG-bytes"
        );
    }
}
