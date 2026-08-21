---
name: public-images
description: >
  Put a photograph on a public page — the whole path from a JPEG on your desk to bytes serving from every deployment's
  assets bucket. The bytes NEVER enter git (`server/public/img/` is gitignored and `.dockerignore`d), so "it renders
  locally" proves nothing about production: publication is a separate, per-deployment `upload`. Trigger when asked to
  add, replace, or swap an image on a public page, when a hero/blog/marketing picture 404s in staging or production,
  when editing `views::assets::GALLERY`, or before reaching for `<img src="/public/...">` in a Dioxus view. Covers the
  manifest entry, the responsive `<picture>` seam, `build --only`, the per-deployment upload, and the `verify` gate.
  Video has no lane at all — see §7 before promising one. The tracked-asset exceptions (logos, team photos) are §1.
  Capture-and-embed-in-a-PR is [[pr-image-upload]]; looking at the result in a browser is [[web-preview]].
---

# Putting an image on a public page

The one thing that makes this different from every other web framework: **image bytes are not in the repository.**
`server/public/img/` is in `.gitignore` *and* `.dockerignore`, so a fresh clone has empty slots and a container image
carries none of it. Production serves images from the deployment's own public assets bucket, through the app's
`/assets/{key}` route.

Two consequences that bite in exactly this order:

1. A page can render perfectly on your machine and 404 its hero in production, because you built the variants locally
   and never uploaded them.
2. Each deployment has its **own** bucket. Publishing to staging does not publish to production. There is no shared
   origin.

## 1. First decide whether this is an asset-lane image

Not every image goes through the bucket. One kind stays tracked in git and ships inside the container:

| Kind | Where | Why it is tracked |
| --- | --- | --- |
| Brand marks | `server/public/logo-*.svg`, `logo-*.png` | The header paints them before anything else resolves. |

That is the whole exception, and it is this repository's own marks. A photograph of a person has no tracked lane — the
`/team` surface is retired, so a portrait added here would ship a real person's likeness to serve no page. Another
organization's mark is not an asset-lane question at all but a trademark one: do not add one without written permission,
and never ship one no page uses.

Everything photographic — heroes, blog images, marketing pictures — goes through the asset lane below. If you are about
to add a multi-hundred-kilobyte photograph to `git`, stop: that is the lane's whole reason for existing.

## 2. Add the manifest entry

`views::assets::GALLERY` is the single source of truth, shared by the build pipeline and the view layer. Adding a photo
is a manifest edit plus a JPEG — not a code change at the call site.

```rust
GalleryImage {
    slug: "berkeley-bay",              // URL + directory stem, and the `--only` key
    theme: Theme::Firm,                // editorial axis: Nevada | Global | Beauty | Firm
    aspect: Aspect::Hero,              // Hero | Wide | Landscape | Square | Portrait
    alt: "The San Francisco Bay and the Golden Gate seen across Berkeley from the hills",
    source: "berkeley-bay.jpg",        // filename under the build's `--src` directory
},
```

`alt` lives in the manifest, not in the view, so every page that renders the photo describes it the same way. Write a
real description: a hero is a page's opening statement, and an empty `alt` tells a screen-reader user the firm chose to
lead with nothing.

## 3. Render it through the responsive seam

Never hand-write `<img src="/public/img/...">` in a view. The URL depends on `NAVIGATOR_ASSET_BASE_URL`, which a wasm
view cannot read. The server resolves it once and injects plain strings:

```rust
// In the router (portal), at router-build time:
let picture = views::assets::responsive_picture("berkeley-bay", "100vw");
```

`responsive_picture` returns the three `<source>` sets — AVIF, then WebP, then JPEG, smallest-format first because the
browser takes the first `type` it supports — plus the JPEG `fallback_src`, the manifest `alt`, and your `sizes`. It
returns `None` for a slug outside the manifest, so a typo renders no image rather than a `<picture>` of 404s.

The view takes that as wasm-safe data and renders it. Two Dioxus gotchas:

- `srcset` and `sizes` are **not** in Dioxus's `source` element definition. Write them as raw attributes
  (`"srcset": "…"`), not typed ones, or you get `cannot find value 'srcset' in module dioxus_elements::source`.
- Give the `<picture>` `display: contents` in CSS. It is an inline wrapper with no box of its own, and the `<img>`
  inside it is what actually fills the layout.

Handle the unpublished case in the view. `hero: Option<…>` that degrades to no image is not defensive padding — an
un-uploaded photo is a real state of a deployment, and the page must not open on a broken image.

See [webapp/src/home.rs](../../../webapp/src/home.rs) for the worked example.

## 4. Build the variants

```bash
cargo run -p cli -- ops assets build --src ~/photos --only berkeley-bay
```

`--only` is load-bearing. Without it the build walks the **whole** manifest and fails on the first source JPEG you do
not have on disk — which is every photo except the one you are adding. An unknown slug is an error that lists the
manifest, not an empty success.

Output lands at `server/public/img/<slug>/<slug>-{400,800,1200}w.{avif,webp,jpg}` — nine files per photo. That is the
local `/public` mount, so the dev loop serves them immediately with no further setup.

## 5. Publish to every deployment

Each deployment reads its own bucket, named by `NAVIGATOR_ASSETS_BUCKET` in `deployments/<name>/config.toml`:

| Deployment | Bucket | Serves |
| --- | --- | --- |
| `neon-law-stg` | `neon-law-stg-assets` | `https://staging.neonlaw.com/assets` |
| the production deployment | its `<deployment>-assets` | its public host's `/assets` |
| any other deployment | its `config.toml` | its `config.toml` |

```bash
cargo run -p cli -- ops assets upload --bucket neon-law-stg-assets
cargo run -p cli -- ops assets upload --bucket <production>-assets
```

Auth is ADC (`gcloud auth application-default login`). Upload walks `server/public/img/` and pushes every recognized
image under key `img/<rel>`, so re-uploading an unchanged tree is idempotent and cheap.

**These are real cloud writes to staging and production.** Per `CLAUDE.md` an agent proposes them and the operator runs
them; do not run them unattended.

Staging first, then production. The `Cache-Control` is a bounded week and deliberately not `immutable`, because the
variant URLs carry no cache-bust token — so replacing a photo at the same slug goes live once the old TTL expires, not
instantly. If you need it instantly, use a new slug.

## 6. Verify, because rendering is not publishing

```bash
# The published origin, exactly as a browser would fetch it:
NAVIGATOR_ASSET_BASE_URL=https://www.neonlaw.com/assets cargo run -p cli -- ops assets verify

# A running local dev loop:
cargo run -p cli -- ops assets verify --base-url http://localhost:<web-port>/public
```

`verify` walks every `img/…` reference under `server/content` and fails listing anything missing. Note the scope: it
reads **content markdown**, so a hero referenced only from Rust code is outside its reach. For those, fetch the fallback
URL directly:

```bash
curl -I https://www.neonlaw.com/assets/img/berkeley-bay/berkeley-bay-1200w.jpg
```

Restoring a machine that has empty slots (a fresh clone, or someone else's photo):

```bash
export NAVIGATOR_ASSETS_BUCKET=neon-law-stg-assets
cargo run -p cli -- ops assets pull
```

## 7. Video: there is no lane

Nothing in the workspace transcodes, stores, or serves video. `content_type_for` in
[cli/src/assets.rs](../../../cli/src/assets.rs) recognizes `avif`, `webp`, `jpg`, `jpeg`, and `png` and silently skips
everything else — so an `.mp4` dropped into `server/public/img/` uploads **nothing** while reporting success.
`views::assets` has no video shape, and the `<picture>` seam is image-only.

If someone asks for video, say that plainly and treat building the lane as its own issue: the decisions are the storage
prefix, the poster-frame policy, whether it is `<video>` or an embed, the CSP `media-src` allowance, and the bandwidth
cost of a bucket serving multi-megabyte files with a one-week TTL. Do not quietly drop an `.mp4` into the image lane and
call it done — it will report success and then serve a 404.

## Checklist

1. Is it photographic? → asset lane. A brand mark or team photo? → tracked in git (§1).
2. Manifest entry in `views::assets::GALLERY`, with real `alt` (§2).
3. Render through `responsive_picture`, never a hand-written `/public/img/` path (§3).
4. `assets build --src <dir> --only <slug>` (§4).
5. `assets upload --bucket <each deployment's bucket>` — propose, don't run (§5).
6. Verify against the real origin before calling it shipped (§6).
