//! Nebula show-and-tells loaded from dated markdown files under
//! `web/content/events/`.
//!
//! Events mirror the blog convention: one `YYYYMMDD_slug.md` file per
//! public show-and-tell, with reviewable front matter and a rendered markdown body.
//! The extra event fields form the authoring contract that the CLI validates
//! in PRs.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use walkdir::WalkDir;

use crate::content_loader::ContentLoadError;
use crate::marketing;

const NON_EVENT_FILES: &[&str] = &["README.md", ".gitkeep"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub slug: String,
    pub date: NaiveDate,
    pub title: String,
    pub description: String,
    pub body_html: String,
    pub starts_at: NaiveDateTime,
    pub ends_at: NaiveDateTime,
    pub timezone: String,
    pub location_name: String,
    pub location_address: String,
    pub external_event_provider: String,
    pub external_event_url: String,
    pub video_url: Option<String>,
    pub recap_url: Option<String>,
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
    starts_at: String,
    ends_at: String,
    timezone: String,
    location_name: String,
    location_address: String,
    external_event_provider: String,
    external_event_url: String,
    #[serde(default)]
    video_url: Option<String>,
    #[serde(default)]
    recap_url: Option<String>,
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
        let raw = std::fs::read_to_string(path).map_err(|e| ContentLoadError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        events.push(parse_event(&raw, &slug, date, &path.display().to_string())?);
    }
    events.sort_by(|a, b| {
        a.starts_at
            .cmp(&b.starts_at)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    Ok(EventIndex::new(events))
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
    // Events are currently Pacific-only because the first programming is
    // Seattle-based. When events expand to other regions, this guard and
    // the matching VTIMEZONE emitter should become a small timezone registry.
    if fields.timezone != "America/Los_Angeles" {
        return Err(EventLoadError::Invalid {
            path: path.to_string(),
            message: "events must use America/Los_Angeles for Pacific time".to_string(),
        });
    }
    require_non_empty(&fields.location_name, path, "location_name")?;
    require_non_empty(&fields.location_address, path, "location_address")?;
    require_non_empty(
        &fields.external_event_provider,
        path,
        "external_event_provider",
    )?;
    require_non_empty(&fields.external_event_url, path, "external_event_url")?;
    if starts_at.date() != date {
        return Err(EventLoadError::Invalid {
            path: path.to_string(),
            message: "filename date must match starts_at date".to_string(),
        });
    }
    let rendered = marketing::loader::parse(raw, slug).ok_or_else(|| EventLoadError::Invalid {
        path: path.to_string(),
        message: "event markdown must include title and description front matter".to_string(),
    })?;
    Ok(Event {
        slug: slug.to_string(),
        date,
        title: fields.title,
        description: fields.description,
        body_html: rendered.body_html,
        starts_at,
        ends_at,
        timezone: fields.timezone,
        location_name: fields.location_name,
        location_address: fields.location_address,
        external_event_provider: fields.external_event_provider,
        external_event_url: fields.external_event_url,
        video_url: fields.video_url.filter(|url| !url.trim().is_empty()),
        recap_url: fields.recap_url.filter(|url| !url.trim().is_empty()),
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

impl Event {
    #[must_use]
    pub fn ics_filename(&self) -> String {
        format!("{}.ics", self.slug)
    }

    #[must_use]
    pub fn ics(&self) -> String {
        self.ics_with_dtstamp(Utc::now())
    }

    #[must_use]
    fn ics_with_dtstamp(&self, dtstamp: DateTime<Utc>) -> String {
        let starts = self.starts_at.format("%Y%m%dT%H%M%S");
        let ends = self.ends_at.format("%Y%m%dT%H%M%S");
        let date_stamp = dtstamp.format("%Y%m%dT%H%M%SZ");
        let mut lines = Vec::from([
            "BEGIN:VCALENDAR".to_string(),
            "VERSION:2.0".to_string(),
            "PRODID:-//Neon Law//Navigator Events//EN".to_string(),
            "CALSCALE:GREGORIAN".to_string(),
        ]);
        lines.extend(vtimezone_lines(&self.timezone));
        lines.extend([
            "METHOD:PUBLISH".to_string(),
            "BEGIN:VEVENT".to_string(),
            format!("UID:{}@neonlaw.com", self.slug),
            format!("DTSTAMP:{date_stamp}"),
            format!("DTSTART;TZID={}:{}", self.timezone, starts),
            format!("DTEND;TZID={}:{}", self.timezone, ends),
            format!("SUMMARY:{}", ics_escape(&self.title)),
            format!("DESCRIPTION:{}", ics_escape(&self.description)),
            format!(
                "LOCATION:{}",
                ics_escape(&format!(
                    "{}, {}",
                    self.location_name, self.location_address
                ))
            ),
            format!("URL:{}", self.external_event_url),
            "END:VEVENT".to_string(),
            "END:VCALENDAR".to_string(),
            String::new(),
        ]);
        lines
            .into_iter()
            .flat_map(|line| fold_ical_line(&line))
            .collect::<Vec<_>>()
            .join("\r\n")
    }
}

fn vtimezone_lines(timezone: &str) -> Vec<String> {
    match timezone {
        "America/Los_Angeles" => vec![
            "BEGIN:VTIMEZONE".to_string(),
            "TZID:America/Los_Angeles".to_string(),
            "X-LIC-LOCATION:America/Los_Angeles".to_string(),
            "BEGIN:DAYLIGHT".to_string(),
            "TZOFFSETFROM:-0800".to_string(),
            "TZOFFSETTO:-0700".to_string(),
            "TZNAME:PDT".to_string(),
            "DTSTART:19700308T020000".to_string(),
            "RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU".to_string(),
            "END:DAYLIGHT".to_string(),
            "BEGIN:STANDARD".to_string(),
            "TZOFFSETFROM:-0700".to_string(),
            "TZOFFSETTO:-0800".to_string(),
            "TZNAME:PST".to_string(),
            "DTSTART:19701101T020000".to_string(),
            "RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU".to_string(),
            "END:STANDARD".to_string(),
            "END:VTIMEZONE".to_string(),
        ],
        _ => Vec::new(),
    }
}

fn ics_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

fn fold_ical_line(line: &str) -> Vec<String> {
    const LIMIT: usize = 75;
    if line.len() <= LIMIT {
        return vec![line.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in line.chars() {
        if current.len() + ch.len_utf8() > LIMIT {
            lines.push(current);
            current = " ".to_string();
        }
        current.push(ch);
    }
    lines.push(current);
    lines
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
    use super::{load_dir, parse_event_filename};
    use chrono::{Datelike, NaiveDate, TimeZone, Timelike, Utc, Weekday};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn bundled_events_directory_loads_seattle_event() {
        let ix = load_dir(std::path::Path::new(crate::DEFAULT_EVENTS_DIR)).unwrap();
        let event = ix
            .get("seattle-agentic-workflows-for-lawyers")
            .expect("Seattle event should load from bundled events");
        assert_eq!(
            event.starts_at.date(),
            NaiveDate::from_ymd_opt(2026, 7, 2).unwrap()
        );
        assert_eq!(event.starts_at.weekday(), Weekday::Thu);
        assert_eq!(event.starts_at.hour(), 11);
        assert_eq!(event.ends_at.hour(), 15);
        assert_eq!(event.timezone, "America/Los_Angeles");
        assert_eq!(event.external_event_provider, "luma");
        assert_eq!(event.external_event_url, "https://luma.com/k26256ut");
        assert!(event
            .body_html
            .contains("agentic workflows mean for lawyers"));
    }

    #[test]
    fn ics_uses_pacific_tzid_for_viewer_timezone_conversion() {
        let ix = load_dir(std::path::Path::new(crate::DEFAULT_EVENTS_DIR)).unwrap();
        let event = ix.get("seattle-agentic-workflows-for-lawyers").unwrap();
        let ics = event.ics();
        assert!(ics.contains("BEGIN:VTIMEZONE"));
        assert!(ics.contains("TZID:America/Los_Angeles"));
        assert!(ics.contains("RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU"));
        assert!(ics.contains("RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU"));
        assert!(ics.contains("DTSTART;TZID=America/Los_Angeles:20260702T110000"));
        assert!(ics.contains("DTEND;TZID=America/Los_Angeles:20260702T150000"));
        assert!(ics.contains("URL:https://luma.com/k26256ut"));
    }

    #[test]
    fn ics_dtstamp_uses_supplied_utc_generation_time() {
        let ix = load_dir(std::path::Path::new(crate::DEFAULT_EVENTS_DIR)).unwrap();
        let event = ix.get("seattle-agentic-workflows-for-lawyers").unwrap();
        let ics = event.ics_with_dtstamp(Utc.with_ymd_and_hms(2026, 6, 24, 7, 8, 9).unwrap());
        assert!(ics.contains("DTSTAMP:20260624T070809Z"));
        assert!(!ics.contains("DTSTAMP:20260702T000000Z"));
    }

    #[test]
    fn ics_folds_long_lines_without_splitting_utf8() {
        let ix = load_dir(std::path::Path::new(crate::DEFAULT_EVENTS_DIR)).unwrap();
        let mut event = ix
            .get("seattle-agentic-workflows-for-lawyers")
            .unwrap()
            .clone();
        event.description = "A long description with enough words to require folding, plus a snowman: ☃. Calendar clients should receive continuation lines."
            .to_string();
        let ics = event.ics_with_dtstamp(Utc.with_ymd_and_hms(2026, 6, 24, 7, 8, 9).unwrap());
        assert!(
            ics.lines().all(|line| line.len() <= 75),
            "every iCalendar content line should be folded: {ics}"
        );
        assert!(
            ics.lines().any(|line| line.starts_with(' ')),
            "expected at least one folded continuation line: {ics}"
        );
        assert!(ics.contains("☃"));
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
location_name: Room\n\
location_address: Seattle\n\
external_event_provider: luma\n\
external_event_url: https://luma.com/k26256ut\n\
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
    fn load_dir_rejects_empty_location() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("20260702_bad.md"),
            "---\n\
title: Bad\n\
description: Bad event.\n\
starts_at: \"2026-07-02T11:00:00\"\n\
ends_at: \"2026-07-02T15:00:00\"\n\
timezone: America/Los_Angeles\n\
location_name: \"\"\n\
location_address: Seattle\n\
external_event_provider: luma\n\
external_event_url: https://luma.com/k26256ut\n\
---\n\nBody.\n",
        )
        .unwrap();
        let err = load_dir(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("location_name is required"),
            "expected location_name error, got: {err}"
        );
    }

    #[test]
    fn load_dir_rejects_non_pacific_timezone() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("20260702_bad.md"),
            "---\n\
title: Bad\n\
description: Bad event.\n\
starts_at: \"2026-07-02T11:00:00\"\n\
ends_at: \"2026-07-02T15:00:00\"\n\
timezone: UTC\n\
location_name: Room\n\
location_address: Seattle\n\
external_event_provider: luma\n\
external_event_url: https://luma.com/k26256ut\n\
---\n\nBody.\n",
        )
        .unwrap();
        let err = load_dir(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("America/Los_Angeles"),
            "expected Pacific timezone error, got: {err}"
        );
    }

    #[test]
    fn parse_filename_matches_blog_convention() {
        let (date, slug) = parse_event_filename("20260702_seattle_agentic").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 7, 2).unwrap());
        assert_eq!(slug, "seattle-agentic");
    }
}
