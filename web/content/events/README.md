# Nebula Show-And-Tell Markdown

Show-and-tells are reviewable markdown files, stored like blog posts:

```text
YYYYMMDD_slug.md
```

The filename date must match `starts_at`. The optional `public_slug` chooses the public URL for each show-and-tell; when
it is absent, the filename slug is used. For example, `public_slug: seattle-summer-2026` serves the file at
`/foundation/nebula/show-and-tell/seattle-summer-2026`.

A file with a `starts_at` timestamp is an **event**; the rules engine classifies it as such and applies the E-family
event rules. An event must never declare a `questionnaire`/`workflow` (those make a file a notation template) — the two
are mutually exclusive.

Every show-and-tell requires this front matter:

```yaml
title: "Seattle Show and Tell: Agentic Workflows for Lawyers"
description: >
  One sentence summary for lists, search, and calendar descriptions.
public_slug: seattle-summer-2026
draft: true
starts_at: "2026-07-02T11:00:00"
ends_at: "2026-07-02T15:00:00"
timezone: America/Los_Angeles
location_name: Private lounge
location_address: 1920 4th Ave, downtown Seattle
meeting_url:
image_url: /public/events/nebula-show-and-tell/nlf-lawyers-seattle.png
image_alt: Lawyers gathered in Seattle with a Neon Law Foundation flag
video_url:
recap_url:
```

`starts_at` and `ends_at` are local wall times in the declared `timezone`. Supported event time zones are
`America/Los_Angeles`, `America/Denver`, `America/Chicago`, and `America/New_York`. The iCalendar route emits the
matching `TZID`, so calendar clients convert the show-and-tell for each viewer's timezone.

## Draft vs. published

`draft: true` keeps an event out of every public surface (the index, the landing preview, the detail page, and the
iCalendar feed) while still tracking it. Omit the field, or set `draft: false`, to publish. Stage new cities as drafts
and flip them to published when they are official.

## Where and how to attend

An event must declare **at least one** of:

- `location_address` — a physical street address (in-person), optionally with a `location_name` venue label; or
- `meeting_url` — an online join link (Google Meet / Zoom).

A hybrid event may declare both. The public page renders a **Register** form: visitors register with their email on our
own site (we no longer use Luma), and we store only that email against the event. `image_url` should point at a
committed public asset, normally under `/public/events/nebula-show-and-tell/`.

## Validate

Events are linted by the shared rules engine plus the typed loader's deeper checks:

```bash
cargo run -p cli -- validate-events web/content/events
```

The general markdown validator also classifies and lints events when run over a tree that includes them:

```bash
cargo run -p cli -- validate web/content/events
```
