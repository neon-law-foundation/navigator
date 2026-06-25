# Photography assets

Navigator's marketing and workshop pages render responsive photos through `views::assets::responsive_picture`. Those
photos are **never** stored in git or baked into the Docker image — they live only in a public Google Cloud Storage
bucket. That keeps the repository small (a clone is code, not megabytes of binaries) and lets production serve the
images straight from object storage.

## The three commands

The `navigator assets` subcommands form a build → publish → restore loop. The manifest
([`views::assets::GALLERY`](../views/src/assets.rs)) and the width set (`WIDTHS = [400, 800, 1200]`) are the single
source of truth shared with the view layer, so adding a photo is a manifest edit plus a JPEG — never a code change.

| Command | Direction | What it does |
| --- | --- | --- |
| `assets build` | source JPEGs → `web/public/img/` | Re-encode each manifest photo to AVIF/WebP/JPEG at every width. |
| `assets upload` | `web/public/img/` → bucket | Push the built variants to `gs://<project>-assets/img/<slug>/…`. |
| `assets pull` | bucket → `web/public/img/` | Download the published variants back for local development. |

`build` and `upload` are the publish path, run by whoever curates the gallery; `pull` is the restore path every
developer runs.

## Why the images aren't in git

`web/public/img/` is in [`.gitignore`](../.gitignore). A fresh clone has **empty photo slots** — the rest of
`web/public` (Bootstrap, brand SVGs, vendored JS/CSS) stays tracked and still ships in the image, but photos do not.
Production does not bake them in either: the `web` binary's image URLs resolve through `views::assets::asset_url`, which
in production prefixes `NAVIGATOR_ASSET_BASE_URL` (the bucket's public origin), so browsers fetch photos straight from
the bucket. The single cross-origin allowance in the Content-Security-Policy (`img-src`) is exactly this.

## Local development: pull the photos down

Because the slots are empty on a fresh clone, the dev `/public` mount 404s every photo until you populate
`web/public/img/`. The fast path is to **pull** the already-published variants from the bucket — no source JPEGs, no
re-encode:

```bash
export NAVIGATOR_ASSETS_BUCKET="$(gcloud config get-value project)-assets"   # or set it in .env
cargo run -p cli -- assets pull
```

This downloads every variant under the bucket's `img/` prefix into `web/public/img/<slug>/…`, byte-identical to what
`build` would have produced. Run it once after cloning, and again whenever the gallery changes; the KIND dev loop then
serves the photos from `/public` with no further setup. Auth is ADC (`gcloud auth application-default login`); the
fake-gcs emulator endpoint is honored via `NAVIGATOR_STORAGE_ENDPOINT`.

If you are _curating_ the gallery (adding or replacing a photo), use `build` from the source JPEGs and then `upload`
instead — see the [photography-assets section of `cli/README.md`](../cli/README.md#photography-assets).
