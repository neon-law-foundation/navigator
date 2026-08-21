---
name: authoring-slides
description: >
  Author a workshop or presentation slide deck in the stepped-markdown format the workshop loader parses, and put media
  on a slide so it survives the projector and every deployment. Trigger when adding, reordering, or rewriting slides in
  `server/content/workshops/`, when a deck needs a picture, when a slide image 404s in staging or production, or when
  someone asks for "a new slide" or "add this to slide N". Encodes the anatomy (chapter, slide, divider, presenter
  notes), the two media lanes and which to pick, the 120-character alt budget the `S101` linter enforces, the CSS that
  makes a picture fit a fixed 16:9 canvas, and the grounding test that fails the build when a code slide drifts from the
  file it cites. Video is written with the image syntax and is bucket-lane only — see §6. Publishing photographs on
  ordinary marketing pages is [[public-images]]; looking at the result is [[web-preview]].
---

# Authoring a workshop or presentation slide deck

A deck is one markdown file under `server/content/workshops/navigator/`, listed in `NAVIGATOR_MANIFEST` in
[portal/src/workshops/loader.rs](../../../portal/src/workshops/loader.rs). The same file renders four ways — a long-form
page, a contact sheet of thumbnails, one slide per step, and a bare full-screen projector view — so anything you write
has to survive all four. That is the constraint the rest of this skill exists to serve.

## 0. A slide's words belong to its author

A deck is a script for a person standing in front of a room, so the text on a slide face and in its presenter notes is
that person's voice. Carry it verbatim. Reflow it, lint it, and fix its shape, and leave every word as written.

That splits any edit to an existing deck into two kinds of change:

- **Shape** — the `---` divider, heading level, line wrapping, trailing whitespace, one crammed bullet split into two.
  Make these freely: the linter and the guards in §1 require them.
- **Substance** — a word, a claim, a name, a joke, the argument a note makes, the title of a slide. These are the
  author's. Raise the question and let the author answer it.

`M024` is the case worth naming, because its fix looks mechanical and is not. A duplicate heading is resolved by
retitling a slide, which is substance, so report the collision and the candidates instead of choosing one.

## 1. Anatomy

A leading YAML frontmatter block, fenced by a `---` line above and below, declares the page kind so the linter
classifies it. It is page chrome, stripped before the file is split, so its delimiters never read as slide dividers:

```yaml
kind: workshop
title: Rust in Peace
description: One sentence, used by the linter and the page chrome.
```

The body below it is a strict three-level structure. A `#` title, `##` chapters, `###` slides, and on every slide a
`---` thematic break dividing the face from the presenter notes:

```markdown
# Rust in Peace

Lede prose, before any chapter. This becomes the intro.

## Intro

Optional chapter preamble. Rendered on the overview, but not a slide.

### Las Vegas, 2011

The slide face: what the room reads.

---

The presenter notes: what you say. Shown under the slide, hidden in projector mode.

## Wrap Up
```

Three rules the suite enforces against the real baked content rather than a fixture, so a new deck cannot ship in the
wrong shape:

1. Every chapter holds at least one slide, and the chapter ranges cover every slide exactly once.
2. Every slide has a non-empty face **and** non-empty notes. A slide with no `---` divider fails the build.
3. No slide bullet crams two `**Term** — definition` pairs into one list item. Markdown folds an indented continuation
   line into the preceding item, so a wrapped bullet silently collapses into a wall of text. Give each term its own
   bullet.

The chapter names are yours: the suite prescribes neither a first chapter nor a last one. The first two rules live in
`every_material_has_chapters_and_section_notes`, the third in `no_slide_bullet_crams_multiple_terms_into_one_item`. The
title, description, audience, and benefit shown on the `/workshops` or `/presentations` index come from the
**manifest**, not from the markdown.

## 2. Line length is a hard 120

`S101` caps every line at 120 characters and skips fenced code blocks only. There is no exemption for a long URL, a
link, or an image, and it is an error rather than a warning. `S102` applies the opposite pressure: it flags a line that
could pull up the next line's first word and still fit. So the fix for an over-long line is to **refill the whole
paragraph**, never to push one word down — that just leaves an orphan the packing rule then flags in turn.

Find candidates before reaching for the linter. `awk` counts bytes, so an em-dash reads as three while `S101` counts one
character; treat what it prints as candidates rather than as verdicts:

```bash
awk 'length > 120 {print FILENAME":"NR": "length}' server/content/workshops/navigator/RUST_IN_PEACE.md
```

Then confirm with the linter, which takes **one path per invocation** — a list of paths exits 2 and lints nothing:

```bash
cargo run -p cli --quiet -- validate server/content/workshops/navigator/RUST_IN_PEACE.md
```

## 3. Putting a picture on a slide

Write an ordinary markdown image. The workshop loader routes every image source through the asset seam
(`views::assets::rewrite_image_src`), the same one the blog uses, which is what makes the choice below a choice at all:

```markdown
![The Las Vegas Ruby Group's red ruby profile mark](img/lvrug/lvrug.png)
```

### Which lane

| | **Bucket lane** — `img/…` | **Tracked lane** — `/public/…` |
| --- | --- | --- |
| Lives at | `server/public/img/<slug>/<file>` | `server/public/workshops/<deck>/<file>` |
| In git? | No — gitignored | Yes |
| In the container image? | No — dockerignored | Yes |
| Publishing | `ops assets upload`, **per deployment** | Nothing. It ships with the image. |
| Covered by `assets verify`? | **Yes** | No |

**Default to the bucket lane.** Not because it is simpler — it is not — but because it is the only one `assets verify`
can check, and an unverified slide image is one that 404s in front of a room. The tracked lane's failure mode stays
invisible until a human happens to look at the page.

Take the tracked lane when the picture must not depend on a separate deploy step: a logo the page cannot open without,
or a deck being presented from a laptop that may never run an upload.

Either way the alt text is not optional. A slide is often *only* a picture, so an empty alt attribute tells a
screen-reader user the slide is blank. Mind the budget too: the whole line is capped at 120 characters, and the image
markup plus a 21-character path already spends 26 of them.

### Formats

`content_type_for` in [cli/src/assets.rs](../../../cli/src/assets.rs) recognizes five image extensions — `avif`, `webp`,
`jpg`, `jpeg`, and `png` — plus `mp4` for the video lane in §6. Anything else is **skipped silently**: uploaded as
nothing, reported as success.

- **PNG** for flat-colour art: logos, wordmarks, diagrams, and UI screenshots, where JPEG rings on the hard edges. The
  code carries `png` specifically for these hand-authored assets.
- **JPEG** for photographs, where `assets build` can also emit the responsive AVIF and WebP variants.

`views::assets::GALLERY` and `assets build` serve the responsive picture seam used by **Rust views**. A markdown slide
needs neither: drop the file under `server/public/img/<slug>/` and `upload` carries the bytes through untouched.

### Publishing

A generated or converted slide image is not finished while it exists only in a temporary directory, clipboard
attachment, or image-generation result. Save the full-resolution PNG or JPEG at
`server/public/img/<deck-slug>/<filename>` first. That ignored file is the local preview copy. The matching cloud key is
`img/<deck-slug>/<filename>`.

Each deployment reads its own bucket. Publishing to staging publishes nothing to production, so the bucket lane always
has this order: local copy, staging upload and check, production operator upload and check. Presentations and workshops
mount under the firm site at the root, so both configured environments need the object.

```bash
gcloud auth application-default login
cargo run -p cli -- ops assets verify --base-url http://localhost:<web-port>/public
cargo run -p cli -- ops assets upload --dir server/public/img --bucket neon-law-stg-assets
gcloud storage ls -L gs://neon-law-stg-assets/img/<deck-slug>/<filename>
```

The production upload is a **real production cloud write**. An agent prepares the local file, can publish staging when
authorized, and hands the production command to an operator:

```bash
cargo run -p cli -- ops assets upload --dir server/public/img --bucket <production>-assets
gcloud storage ls -L gs://<production>-assets/img/<deck-slug>/<filename>
```

`upload` walks the whole directory passed with `--dir`, so re-uploading an unchanged tree is idempotent. The PR carries
the Markdown reference; the ignored local tree and the two buckets carry the bytes. If production remains pending, say
so and provide the exact command rather than claiming the slide is published everywhere. After deployment, run `assets
verify` against the public origin a browser actually uses. A bucket-lane slide is not complete until the exact-key
metadata exists in **both** configured buckets, staging and production report the same byte length and hashes, and the
deployed slide's image has non-zero natural dimensions in a browser. `ops ship` independently repeats the complete
presentation/workshop key check against the selected deployment bucket before every full or image-only roll.

### Removing media

`upload` never deletes. An image or clip you drop from a slide **stays publicly fetchable at its URL indefinitely**, so
"removed from the deck" does not mean "off the internet" — a real distinction for a firm publishing client-adjacent
material. `verify` cannot catch this: it only walks references toward objects, never the reverse.

```bash
cargo run -p cli -- ops assets orphans --bucket neon-law-stg-assets --slack
```

It reports and never prunes; deleting is a human step after review. `--slack` posts the same report to the ops channel
through `SLACK_WEBHOOK_URL`, the seam the durable workflows' heartbeat already uses, so a scheduled run surfaces drift
where engineers watch. Only object keys are named, and those are already public URLs, so nothing crosses the trust
boundary the bucket did not already cross.

The reachable set is a **union**, and this is the part to respect: markdown `](img/…)` references *plus* the
`views::assets::GALLERY` variants. Every production photograph is referenced from Rust views through
`responsive_picture` and appears in no markdown at all, so a check built on the content sweep alone reports all of them
as unreferenced — around 126 objects, the entire photo library. The guard test
`a_gallery_photo_is_never_reported_as_an_orphan` fails the build if that union ever regresses.

## 4. How a picture fits the slide

The slide face is a fixed 16:9 canvas with clipped overflow, styled in
[server/public/css/catalog.css](../../../server/public/css/catalog.css). Three consequences follow, and all three are
already handled — do not re-solve them per deck:

- A face containing an image lays out as a **flex column**, and the image's paragraph flexes into whatever height the
  heading and copy leave behind. A fixed share of the canvas is wrong, because the box scales with the viewport while
  the text does not, so any fixed share overflows on a small screen and wastes space on a large one.
- That flex switch is scoped with `:has(img)` so every other slide keeps block layout, where adjoining margins still
  collapse as their authors expect.
- The contact-sheet thumbnail renders the same HTML into a fixed cell that clips rather than scales, so it caps the
  image separately. Without that cap a full-size picture shows as a corner crop.

Composition note: a slide carrying a picture usually wants the heading and the image and nothing else. Move the prose
into the presenter notes, which is where it belongs anyway — the room cannot read a paragraph and study a picture at the
same time.

## 5. Code slides are exact copies

A code slide is introduced by an attribution line of the exact form ``From `workflows/src/guardrail.rs`:`` — the word
"From", the repo-relative path in a code span with no leading slash, then a colon — followed immediately by a fenced
block:

```rust
pub fn lawyer_review_precedes_signature(spec: &WorkflowSpec) -> Result<(), GateViolation> {
```

`rust_in_peace_snippets_are_exact_copies_of_cited_sources` reads each cited path **from the workspace** and fails the
build when the snippet is not a substring of that file. A refactor that renames a function therefore breaks the talk, by
design: the slide is not allowed to drift from the shipped code. Fix the slide, and never weaken the test. The fence
must close, and the test asserts a floor of six grounded snippets so the convention cannot quietly disappear.

## 6. Video

A clip uses the **image syntax** — markdown has none of its own — and the renderer emits a `<video>` when the
destination ends in `.mp4` or `.webm`:

```markdown
![The portal, recorded end to end](img/demo/portal.mp4)
```

Reusing the image syntax rather than hand-written HTML is the whole point: a clip then rides every seam a picture
already rides. Its destination resolves through the asset seam, `assets verify` follows it because that sweep matches
`](img/…)` references, and the linter still sees ordinary markdown rather than raw HTML nothing checks.

Playback is `controls preload="metadata" playsinline`, and deliberately never `autoplay`. A deck opens many slides at
once, so a clip that starts itself is both a bandwidth surprise and an accessibility failure. `<video>` has no `alt`
attribute, so the caption becomes the `aria-label` and the fallback content a browser shows when it cannot play the file
— write a real description, and never a bare filename.

**MP4 (H.264) is required** — it is the only format either the renderer or the bucket accepts. The `<video>` carries a
single `src` rather than a list of `<source>` children, so a second format would be an alternative to choose between
rather than a fallback: one more way to pick wrong, buying no reach H.264 does not already have.

`mov`, `webm`, `mkv`, and everything else are skipped **silently** — uploaded as nothing, reported as success. Nothing
in the workspace transcodes, so convert first. A QuickTime screen recording needs three things to survive a browser:

```bash
ffmpeg -i in.mov -vf "scale='min(1920,iw)':-2" -c:v libx264 -profile:v high \
  -pix_fmt yuv420p -crf 23 -preset slow -movflags +faststart out.mp4
```

`-pix_fmt yuv420p` is the one people miss: QuickTime captures are often 4:4:4, which browsers refuse to decode, and the
symptom is a file that plays perfectly in QuickTime and shows blank on the page. `+faststart` moves the moov atom to the
front so playback begins before the whole file arrives — without it a bucket-served clip stalls on black. `scale=-2`
rounds to the even height H.264 requires.

The accepted set lives in two crates that must move together: `views::markdown::VIDEO_EXTENSIONS` decides what renders
as a video and `content_type_for` decides what the bucket accepts. `every_renderable_video_extension_is_uploadable`
fails the build when they diverge, because the failure it prevents is invisible — a player pointed at a 404.

In practice video is **bucket-lane only**. A clip is far too large to bake into the container image, and it is the one
media kind where the tracked lane is simply wrong. Cache-Control is a bounded week rather than `immutable`, so replacing
a clip at the same key goes live only once the old TTL expires — publish under a new filename when you need the swap to
be immediate.

The CSP names `media-src 'self'` plus the asset origin in both policies. Left to fall back to `default-src 'self'`, a
clip would play from the local `/public` mount and be blocked from the bucket in production — the exact trap where a
local success proves nothing. Slides render through the Dioxus route, so the per-response policy in `dioxus_app` governs
a slide, while the site-wide policy in `lib.rs` covers everything else.

## 7. Seeing it

Content is read at boot, so **restart the server after every markdown edit**. CSS is served live from disk; markdown is
not, and a stale process is the most common reason an edit "did not take":

```bash
set -a; source .devx/env; set +a
cargo run -p neon
```

Presentations and workshops are top-level firm surfaces served by `neon`. Neither catalog has a Foundation-prefixed
alias:

| View | Path |
| --- | --- |
| Material | `/presentations/rust-in-peace` |
| Contact sheet | `/presentations/rust-in-peace/slides` |
| One slide | `/presentations/rust-in-peace/step/2` |
| Projector | `/presentations/rust-in-peace/display/2` |

Check a media slide in **projector** view rather than only the step view, because the projector is what the room sees.
Browsers cache the stylesheet aggressively across reloads, so bust it before trusting anything you measure.

## Checklist

1. Every word of an existing slide is the author's, carried verbatim (§0).
2. A `###` slide with a `---` divider and real presenter notes (§1).
3. Every line at most 120 characters; refill the paragraph rather than orphaning a word (§2).
4. A picture: pick a lane, write real alt text, and use PNG for flat art (§3). A clip: MP4, bucket lane (§6).
5. A code slide: an exact copy of the cited workspace file (§5).
6. `validate` the file, one path per invocation (§2).
7. Restart the server, then look at the result in projector view (§7).
8. Bucket lane: upload and inspect the exact key in staging **and** production; matching size and hashes are required.
9. After deployment, verify the real origin and confirm the slide image has non-zero natural dimensions (§3, §7).
