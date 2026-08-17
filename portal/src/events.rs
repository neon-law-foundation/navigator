//! Nebula show-and-tells loaded from dated markdown files under
//! `server/content/events/`.
//!
//! Events mirror the blog convention: one `YYYYMMDD_slug.md` file per
//! public show-and-tell, with reviewable front matter and a rendered markdown body.
//! The extra event fields form the authoring contract that the CLI validates
//! in PRs.

use std::path::Path;
use std::sync::Arc;

use chrono::{NaiveDate, NaiveDateTime};
use walkdir::WalkDir;

use crate::content_loader::ContentLoadError;
use crate::marketing;

const NON_EVENT_FILES: &[&str] = &["README.md", ".gitkeep"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub slug: String,
    pub public_slug: String,
    pub date: NaiveDate,
    pub title: String,
    pub description: String,
    pub body_html: String,
    pub starts_at: NaiveDateTime,
    pub ends_at: NaiveDateTime,
    pub timezone: String,
    pub image_url: Option<String>,
    pub image_alt: Option<String>,
    /// Luma event URL. Luma owns everything about attending — where, how, the
    /// guest list, add-to-calendar — and the page just invites the visitor to
    /// check it out there. Required on every event (rule E004).
    pub luma_url: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EventIndex {
    events: Arc<Vec<Event>>,
}

impl EventIndex {
    #[must_use]
    pub fn new(events: Vec<Event>) -> Self {
        Self {
            events: Arc::new(events),
        }
    }

    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    #[must_use]
    pub fn get(&self, slug: &str) -> Option<&Event> {
        self.events.iter().find(|event| event.slug == slug)
    }

    /// Look up an event by its public slug.
    #[must_use]
    pub fn get_public(&self, slug: &str) -> Option<&Event> {
        self.events.iter().find(|event| event.public_slug == slug)
    }

    #[must_use]
    pub fn upcoming(&self, today: NaiveDate) -> Vec<&Event> {
        let mut events: Vec<_> = self
            .events
            .iter()
            .filter(|event| event.date >= today)
            .collect();
        // Sort ascending (nearest first) so the "soonest upcoming" promise holds
        // regardless of insertion order — `EventIndex::new` carries no ordering
        // contract, mirroring `past`'s explicit descending sort.
        events.sort_by(|a, b| {
            a.starts_at
                .cmp(&b.starts_at)
                .then_with(|| a.slug.cmp(&b.slug))
        });
        events
    }

    #[must_use]
    pub fn past(&self, today: NaiveDate) -> Vec<&Event> {
        let mut events: Vec<_> = self
            .events
            .iter()
            .filter(|event| event.date < today)
            .collect();
        events.sort_by(|a, b| {
            b.starts_at
                .cmp(&a.starts_at)
                .then_with(|| a.slug.cmp(&b.slug))
        });
        events
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EventLoadError {
    #[error(transparent)]
    Content(#[from] ContentLoadError),
    #[error("{path}: {message}")]
    Invalid { path: String, message: String },
}

#[derive(Debug, serde::Deserialize)]
struct EventFrontmatter {
    title: String,
    description: String,
    #[serde(default)]
    public_slug: Option<String>,
    starts_at: String,
    ends_at: String,
    timezone: String,
    #[serde(default)]
    image_url: Option<String>,
    #[serde(default)]
    image_alt: Option<String>,
    #[serde(default)]
    luma_url: Option<String>,
}

fn parse_event_filename(stem: &str) -> Option<(NaiveDate, String)> {
    let (date_part, slug) = stem.split_once('_')?;
    if slug.is_empty() {
        return None;
    }
    let date = NaiveDate::parse_from_str(date_part, "%Y%m%d").ok()?;
    Some((date, views::slug::to_url(slug)))
}

pub fn load_dir(dir: &Path) -> Result<EventIndex, EventLoadError> {
    let mut events = Vec::new();
    if !dir.exists() {
        return Ok(EventIndex::empty());
    }
    for entry in WalkDir::new(dir).follow_links(false) {
        let entry = entry.map_err(|e| ContentLoadError::Io {
            path: dir.display().to_string(),
            source: std::io::Error::other(e),
        })?;
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if NON_EVENT_FILES.contains(&name) {
            continue;
        }
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let Some((date, slug)) = parse_event_filename(stem) else {
            tracing::warn!(
                file = name,
                "skipping event file: name is not YYYYMMDD_slug.md"
            );
            continue;
        };
        events.push(read_and_parse(path, date, &slug)?);
    }
    events.sort_by(|a, b| {
        a.starts_at
            .cmp(&b.starts_at)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    Ok(EventIndex::new(events))
}

/// Load and fully validate a single event markdown file.
///
/// This is the per-file typed pass `navigator validate` runs over every
/// `Event`-classified file, anywhere in the tree: the name must be
/// `YYYYMMDD_slug.md`, the timestamps must parse, `ends_at` must follow
/// `starts_at`, the timezone must be one we emit a `VTIMEZONE` for, and the
/// filename date must match `starts_at`. Unlike [`load_dir`], which tolerantly
/// skips a file whose name is not the event convention, this errors on it —
/// the caller has already classified the file as an event, so a malformed
/// name is a defect to report, not a file to ignore.
pub fn load_file(path: &Path) -> Result<Event, EventLoadError> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let (date, slug) = parse_event_filename(stem).ok_or_else(|| EventLoadError::Invalid {
        path: path.display().to_string(),
        message: "event filename must be YYYYMMDD_slug.md".to_string(),
    })?;
    read_and_parse(path, date, &slug)
}

/// Read `path` and parse it into an [`Event`] with the already-resolved
/// filename `date`/`slug`. Shared by [`load_dir`] and [`load_file`].
fn read_and_parse(path: &Path, date: NaiveDate, slug: &str) -> Result<Event, EventLoadError> {
    let raw = std::fs::read_to_string(path).map_err(|e| ContentLoadError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    parse_event(&raw, slug, date, &path.display().to_string())
}

fn parse_event(
    raw: &str,
    slug: &str,
    date: NaiveDate,
    path: &str,
) -> Result<Event, EventLoadError> {
    let frontmatter = frontmatter(raw).ok_or_else(|| EventLoadError::Invalid {
        path: path.to_string(),
        message: "missing YAML front matter".to_string(),
    })?;
    let fields: EventFrontmatter =
        serde_yaml::from_str(frontmatter).map_err(|source| EventLoadError::Invalid {
            path: path.to_string(),
            message: format!("invalid event front matter: {source}"),
        })?;
    let starts_at = parse_local_datetime(&fields.starts_at, path, "starts_at")?;
    let ends_at = parse_local_datetime(&fields.ends_at, path, "ends_at")?;
    if ends_at <= starts_at {
        return Err(EventLoadError::Invalid {
            path: path.to_string(),
            message: "ends_at must be after starts_at".to_string(),
        });
    }
    require_non_empty(&fields.title, path, "title")?;
    require_non_empty(&fields.description, path, "description")?;
    if fields.timezone.trim().is_empty() {
        return Err(EventLoadError::Invalid {
            path: path.to_string(),
            message: "timezone is required".to_string(),
        });
    }
    if !is_supported_timezone(&fields.timezone) {
        return Err(EventLoadError::Invalid {
            path: path.to_string(),
            message: format!("unsupported timezone `{}`", fields.timezone),
        });
    }
    let public_slug = fields
        .public_slug
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(|| slug.to_string(), views::slug::to_url);
    if starts_at.date() != date {
        return Err(EventLoadError::Invalid {
            path: path.to_string(),
            message: "filename date must match starts_at date".to_string(),
        });
    }
    // Luma hosts the event and its RSVPs, so an event with no `luma_url` has
    // nothing to invite anyone to. Reject it at load time (not just at
    // `validate` time via rule E004) so a published page can never render
    // without its only call to action. This mirrors the old load-time check
    // that every event declare a place to attend.
    let luma_url = fields
        .luma_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToOwned::to_owned);
    if luma_url.is_none() {
        return Err(EventLoadError::Invalid {
            path: path.to_string(),
            message: "event must declare a `luma_url` (Luma hosts the event and its RSVPs)"
                .to_string(),
        });
    }
    let rendered = marketing::loader::parse(raw, slug).ok_or_else(|| EventLoadError::Invalid {
        path: path.to_string(),
        message: "event markdown must include title and description front matter".to_string(),
    })?;
    Ok(Event {
        slug: slug.to_string(),
        public_slug,
        date,
        title: fields.title,
        description: fields.description,
        body_html: rendered.body_html,
        starts_at,
        ends_at,
        timezone: fields.timezone,
        image_url: fields.image_url.filter(|url| !url.trim().is_empty()),
        image_alt: fields.image_alt.filter(|alt| !alt.trim().is_empty()),
        luma_url,
    })
}

fn frontmatter(raw: &str) -> Option<&str> {
    let after_open = raw.strip_prefix("---\n")?;
    let end = after_open.find("\n---")?;
    Some(&after_open[..end])
}

fn parse_local_datetime(
    value: &str,
    path: &str,
    field: &str,
) -> Result<NaiveDateTime, EventLoadError> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").map_err(|source| {
        EventLoadError::Invalid {
            path: path.to_string(),
            message: format!("{field} must be YYYY-MM-DDTHH:MM:SS local time: {source}"),
        }
    })
}

fn is_supported_timezone(timezone: &str) -> bool {
    matches!(
        timezone,
        "America/Los_Angeles" | "America/Denver" | "America/Chicago" | "America/New_York"
    )
}

fn require_non_empty(value: &str, path: &str, field: &str) -> Result<(), EventLoadError> {
    if value.trim().is_empty() {
        return Err(EventLoadError::Invalid {
            path: path.to_string(),
            message: format!("{field} is required"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{load_dir, load_file, parse_event_filename, Event, EventIndex};
    use chrono::{Datelike, NaiveDate, Timelike, Weekday};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn bundled_events_directory_loads_seattle_event() {
        let ix = load_dir(std::path::Path::new(crate::DEFAULT_EVENTS_DIR)).unwrap();
        let event = ix
            .get("seattle-agentic-workflows-for-lawyers")
            .expect("Seattle event should load from bundled events");
        assert_eq!(event.public_slug, "seattle-summer-2026");
        assert!(ix.get_public("seattle-summer-2026").is_some());
        assert_eq!(
            event.starts_at.date(),
            NaiveDate::from_ymd_opt(2026, 7, 2).unwrap()
        );
        assert_eq!(event.starts_at.weekday(), Weekday::Thu);
        assert_eq!(event.starts_at.hour(), 11);
        assert_eq!(event.ends_at.hour(), 15);
        assert_eq!(event.timezone, "America/Los_Angeles");
        assert_eq!(event.luma_url.as_deref(), Some("https://luma.com/k26256ut"));
        assert_eq!(
            event.image_url.as_deref(),
            Some("/public/events/nebula-show-and-tell/nlf-lawyers-seattle.png")
        );
        assert!(event
            .body_html
            .contains("agentic workflows mean for lawyers"));
    }

    #[test]
    fn load_file_parses_a_single_well_formed_event() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("20260702_launch.md");
        fs::write(
            &path,
            "---\n\
title: Launch\n\
description: A launch event.\n\
starts_at: \"2026-07-02T11:00:00\"\n\
ends_at: \"2026-07-02T15:00:00\"\n\
timezone: America/Los_Angeles\n\
luma_url: https://luma.com/launch\n\
---\n\nBody.\n",
        )
        .unwrap();
        let event = load_file(&path).expect("well-formed event loads");
        assert_eq!(event.slug, "launch");
        assert_eq!(
            event.starts_at.date(),
            NaiveDate::from_ymd_opt(2026, 7, 2).unwrap()
        );
    }

    #[test]
    fn load_file_rejects_a_non_convention_filename() {
        // Unlike `load_dir`, which tolerantly skips a misnamed file, the
        // per-file pass errors — the caller already knows it is an event.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("not-an-event-name.md");
        fs::write(
            &path,
            "---\n\
title: Bad\n\
description: Bad event.\n\
starts_at: \"2026-07-02T11:00:00\"\n\
ends_at: \"2026-07-02T15:00:00\"\n\
timezone: America/Los_Angeles\n\
---\n\nBody.\n",
        )
        .unwrap();
        let err = load_file(&path).unwrap_err().to_string();
        assert!(
            err.contains("event filename must be YYYYMMDD_slug.md"),
            "got: {err}"
        );
    }

    #[test]
    fn load_file_rejects_filename_date_mismatch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("20260101_launch.md");
        fs::write(
            &path,
            "---\n\
title: Launch\n\
description: A launch event.\n\
starts_at: \"2026-07-02T11:00:00\"\n\
ends_at: \"2026-07-02T15:00:00\"\n\
timezone: America/Los_Angeles\n\
---\n\nBody.\n",
        )
        .unwrap();
        let err = load_file(&path).unwrap_err().to_string();
        assert!(
            err.contains("filename date must match starts_at date"),
            "got: {err}"
        );
    }

    #[test]
    fn load_dir_rejects_missing_required_frontmatter() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("20260702_bad.md"),
            "---\ntitle: Bad\n---\n\nBody.\n",
        )
        .unwrap();
        let err = load_dir(dir.path()).unwrap_err().to_string();
        assert!(err.contains("invalid event front matter"), "got: {err}");
    }

    #[test]
    fn load_dir_rejects_missing_event_date_time() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("20260702_bad.md"),
            "---\n\
title: Bad\n\
description: Bad event.\n\
ends_at: \"2026-07-02T15:00:00\"\n\
timezone: America/Los_Angeles\n\
---\n\nBody.\n",
        )
        .unwrap();
        let err = load_dir(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("missing field `starts_at`"),
            "expected missing starts_at error, got: {err}"
        );
    }

    #[test]
    fn load_dir_defaults_public_slug_to_filename_slug() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("20260702_source_slug.md"),
            "---\n\
title: Source Slug\n\
description: Event uses its source slug.\n\
starts_at: \"2026-07-02T11:00:00\"\n\
ends_at: \"2026-07-02T15:00:00\"\n\
timezone: America/Los_Angeles\n\
luma_url: https://luma.com/source-slug\n\
---\n\nBody.\n",
        )
        .unwrap();
        let ix = load_dir(dir.path()).unwrap();
        let event = ix.get("source-slug").unwrap();
        assert_eq!(event.public_slug, "source-slug");
        assert!(ix.get_public("source-slug").is_some());
    }

    #[test]
    fn load_dir_rejects_event_without_luma_url() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("20260702_bad.md"),
            "---\n\
title: Bad\n\
description: Bad event.\n\
starts_at: \"2026-07-02T11:00:00\"\n\
ends_at: \"2026-07-02T15:00:00\"\n\
timezone: America/Los_Angeles\n\
---\n\nBody.\n",
        )
        .unwrap();
        let err = load_dir(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("must declare a `luma_url`"),
            "expected missing-luma_url error, got: {err}"
        );
    }

    #[test]
    fn load_dir_rejects_unsupported_timezone() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("20260702_bad.md"),
            "---\n\
title: Bad\n\
description: Bad event.\n\
starts_at: \"2026-07-02T11:00:00\"\n\
ends_at: \"2026-07-02T15:00:00\"\n\
timezone: UTC\n\
---\n\nBody.\n",
        )
        .unwrap();
        let err = load_dir(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("unsupported timezone"),
            "expected unsupported timezone error, got: {err}"
        );
    }

    #[test]
    fn parse_filename_matches_blog_convention() {
        let (date, slug) = parse_event_filename("20260702_seattle_agentic").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 7, 2).unwrap());
        assert_eq!(slug, "seattle-agentic");
    }

    fn event_on(slug: &str, date: NaiveDate) -> Event {
        Event {
            slug: slug.into(),
            public_slug: slug.into(),
            date,
            title: slug.into(),
            description: String::new(),
            body_html: String::new(),
            starts_at: date.and_hms_opt(18, 0, 0).unwrap(),
            ends_at: date.and_hms_opt(20, 0, 0).unwrap(),
            timezone: "America/Los_Angeles".into(),
            image_url: None,
            image_alt: None,
            luma_url: None,
        }
    }

    #[test]
    fn event_index_splits_upcoming_and_past_from_today() {
        let ix = EventIndex::new(vec![
            event_on("past", NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()),
            event_on("today", NaiveDate::from_ymd_opt(2026, 7, 2).unwrap()),
        ]);
        let today = NaiveDate::from_ymd_opt(2026, 7, 2).unwrap();
        assert_eq!(ix.upcoming(today)[0].slug, "today");
        assert_eq!(ix.past(today)[0].slug, "past");
    }

    #[test]
    fn upcoming_and_past_sort_independently_of_insertion_order() {
        // Insert deliberately out of chronological order: the index carries no
        // ordering contract, so the split methods must impose their own order.
        let ix = EventIndex::new(vec![
            event_on("aug", NaiveDate::from_ymd_opt(2026, 8, 1).unwrap()),
            event_on("jun", NaiveDate::from_ymd_opt(2026, 6, 1).unwrap()),
            event_on("jul", NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()),
            event_on("may", NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()),
        ]);
        let today = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        // Upcoming: nearest first (ascending).
        let upcoming: Vec<_> = ix.upcoming(today).iter().map(|e| e.slug.clone()).collect();
        assert_eq!(upcoming, vec!["jul", "aug"]);
        // Past: newest first (descending).
        let past: Vec<_> = ix.past(today).iter().map(|e| e.slug.clone()).collect();
        assert_eq!(past, vec!["jun", "may"]);
    }
}
