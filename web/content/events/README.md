# Nebula Show-And-Tell Markdown

Show-and-tells are reviewable markdown files, stored like blog posts:

```text
YYYYMMDD_slug.md
```

The filename date must match `starts_at`. Nebula chooses the public URL for each show-and-tell; for example,
`20260702_seattle_agentic_workflows_for_lawyers.md` is currently served at
`/foundation/nebula/show-and-tell/seattle-summer-2026`.

Every show-and-tell requires this front matter:

```yaml
title: Agentic Workflows for Lawyers
description: >
  One sentence summary for lists, search, and calendar descriptions.
starts_at: "2026-07-02T11:00:00"
ends_at: "2026-07-02T15:00:00"
timezone: America/Los_Angeles
location_name: Private lounge
location_address: 1920 4th Ave, downtown Seattle
external_event_provider: luma
external_event_url: https://luma.com/k26256ut
video_url:
recap_url:
```

`starts_at` and `ends_at` are local Pacific wall times. The iCalendar route emits `TZID=America/Los_Angeles`, so
calendar clients convert the show-and-tell for each viewer's timezone.

Validate event content before opening a PR:

```bash
cargo run -p cli -- validate-events web/content/events
```
