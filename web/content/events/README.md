# Nebula Show-And-Tell Markdown

Show-and-tells are reviewable markdown files, stored like blog posts:

```text
YYYYMMDD_slug.md
```

The filename date must match `starts_at`. The optional `public_slug` chooses the public URL for each show-and-tell; when
it is absent, the filename slug is used. For example, `public_slug: seattle-summer-2026` serves the file at
`/foundation/nebula/show-and-tell/seattle-summer-2026`.

Every show-and-tell requires this front matter:

```yaml
title: "Seattle Show and Tell: Agentic Workflows for Lawyers"
description: >
  One sentence summary for lists, search, and calendar descriptions.
public_slug: seattle-summer-2026
starts_at: "2026-07-02T11:00:00"
ends_at: "2026-07-02T15:00:00"
timezone: America/Los_Angeles
location_name: Private lounge
location_address: 1920 4th Ave, downtown Seattle
external_event_provider: luma
invite_link: https://luma.com/k26256ut
image_url: /public/events/nebula-show-and-tell/nlf-lawyers-seattle.png
image_alt: Lawyers gathered in Seattle with a Neon Law Foundation flag
video_url:
recap_url:
```

`starts_at` and `ends_at` are local wall times in the declared `timezone`. Supported event time zones are
`America/Los_Angeles`, `America/Denver`, `America/Chicago`, and `America/New_York`. The iCalendar route emits the
matching `TZID`, so calendar clients convert the show-and-tell for each viewer's timezone.

Use `invite_link` for the Luma RSVP URL. `image_url` should point at a committed public asset, normally under
`/public/events/nebula-show-and-tell/`.

Validate event content before opening a PR:

```bash
cargo run -p cli -- validate-events web/content/events
```
