//! The weekly NRS-sync summary email — Foundation-branded.
//!
//! The statutes reference is a Neon Law **Foundation** access-to-justice
//! surface, so the summary wears the Foundation logo (via
//! [`EmailBrand::Foundation`]), never the firm's. The body is the
//! "run summary only" form: aggregate chapter/section counts plus any
//! per-chapter failures — no per-section diff (that richer "what changed"
//! digest is deferred phase-4 work).
//!
//! [`summary_markdown`] and [`build_summary_email`] are pure so the
//! rendered subject/body unit-test without a worker.

use std::fmt::Write as _;

use workflows::email::{render_email_html, EmailBrand};
use workflows::OutboundEmail;

use crate::workflow::ScrapeReport;

/// Default summary-email recipient when `STATUTES_NOTIFY_EMAIL` is unset.
const DEFAULT_NOTIFY_EMAIL: &str = "nick@neonlaw.com";

/// The summary-email recipient: `STATUTES_NOTIFY_EMAIL`, else the default.
/// Takes a `key -> value` lookup so it is unit-testable without mutating
/// process env.
#[must_use]
pub fn notify_recipient<F: Fn(&str) -> Option<String>>(get: F) -> String {
    get("STATUTES_NOTIFY_EMAIL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_NOTIFY_EMAIL.to_string())
}

/// Render the run-summary email body as markdown — the single source of
/// truth for both the plain-text part and (rendered) the HTML alternative.
#[must_use]
pub fn summary_markdown(report: &ScrapeReport) -> String {
    let mut out = String::with_capacity(512);
    let _ = writeln!(out, "The weekly Nevada Revised Statutes sync ran.");
    out.push('\n');
    let _ = writeln!(out, "**Run:** {}", report.run_at);
    out.push('\n');
    let _ = writeln!(
        out,
        "**Chapters:** {} ok · {} absent · {} failed",
        report.chapters_ok, report.chapters_absent, report.chapters_failed
    );
    let _ = writeln!(
        out,
        "**Sections:** {} seen · {} new · {} amended · {} repealed",
        report.sections_seen,
        report.sections_created,
        report.sections_revised,
        report.sections_repealed
    );

    if report.failures.is_empty() {
        let _ = writeln!(out, "\nNo chapter failures.");
    } else {
        let _ = writeln!(out, "\n**Chapter failures:**");
        for f in &report.failures {
            let _ = writeln!(out, "- NRS {} — {}", f.chapter, f.error);
        }
    }

    out.push('\n');
    let _ = writeln!(
        out,
        "This is an automated Neon Law Foundation job that refreshes the \
         public NRS reference at /statutes."
    );
    out
}

/// Build the Foundation-branded `OutboundEmail` for a completed weekly run.
/// The markdown body is both the text part and the source rendered into the
/// inline-styled HTML alternative; `base_url` is the public origin serving
/// `/logo-foundation.png` (see [`workflows::email::base_url_from_env`]).
#[must_use]
pub fn build_summary_email(
    report: &ScrapeReport,
    recipient: &str,
    base_url: &str,
) -> OutboundEmail {
    let subject = format!(
        "NRS statutes sync — {} chapters, {} sections changed",
        report.chapters_ok,
        report.sections_changed()
    );
    let body = summary_markdown(report);
    let html = render_email_html(&body, base_url, EmailBrand::Foundation);
    OutboundEmail::new(recipient.to_string(), subject, body).with_html(html)
}

#[cfg(test)]
mod tests {
    use super::{build_summary_email, notify_recipient, summary_markdown};
    use crate::workflow::{ChapterFailure, ScrapeReport};

    fn report(failures: Vec<ChapterFailure>) -> ScrapeReport {
        ScrapeReport {
            run_at: "2026-06-07T10:00:00Z".into(),
            chapters_ok: 12,
            chapters_absent: 1,
            chapters_failed: failures.len(),
            sections_seen: 1284,
            sections_created: 0,
            sections_revised: 3,
            sections_repealed: 0,
            failures,
        }
    }

    #[test]
    fn notify_recipient_defaults_then_honors_env() {
        assert_eq!(notify_recipient(|_| None), "nick@neonlaw.com");
        assert_eq!(
            notify_recipient(|_| Some(String::new())),
            "nick@neonlaw.com"
        );
        assert_eq!(
            notify_recipient(
                |k| (k == "STATUTES_NOTIFY_EMAIL").then(|| "ops@example.org".to_string())
            ),
            "ops@example.org"
        );
    }

    #[test]
    fn markdown_carries_counts_and_no_failure_line_when_clean() {
        let md = summary_markdown(&report(vec![]));
        assert!(md.contains("**Run:** 2026-06-07T10:00:00Z"));
        assert!(md.contains("12 ok · 1 absent · 0 failed"));
        assert!(md.contains("1284 seen · 0 new · 3 amended · 0 repealed"));
        assert!(md.contains("No chapter failures."));
    }

    #[test]
    fn markdown_lists_each_failure() {
        let md = summary_markdown(&report(vec![ChapterFailure {
            chapter: "118A".into(),
            error: "timeout".into(),
        }]));
        assert!(md.contains("**Chapter failures:**"));
        assert!(md.contains("- NRS 118A — timeout"));
        assert!(!md.contains("No chapter failures."));
    }

    #[test]
    fn email_subject_counts_changed_sections_and_html_wears_foundation_logo() {
        let email = build_summary_email(&report(vec![]), "ops@example.org", "https://mail.test");
        assert_eq!(email.to, "ops@example.org");
        // 3 amended + 0 new = 3 changed.
        assert!(
            email.subject.contains("12 chapters, 3 sections changed"),
            "subject: {}",
            email.subject
        );
        let html = email.html_body.expect("HTML alternative present");
        // Foundation branding — the Foundation logo, never the firm's.
        assert!(html.contains("logo-foundation.png"), "{html}");
        assert!(!html.contains("logo-firm.png"), "{html}");
        // The markdown body is rendered (bold became <strong>).
        assert!(html.contains("<strong>Run:</strong>"));
    }
}
