//! Admin cron-schedule reference page.
//!
//! Lists the Kubernetes `CronJob`s that drive Neon Law Navigator's scheduled
//! work (today: the nightly Archives export) with their cron expression,
//! a human cadence, what each does, and a "Run now" trigger for jobs
//! that expose one.
//!
//! This is a *declared* reference — it documents the schedules that ship
//! in `examples/deploy/k8s/exports/`, not a live cluster read (`web` has
//! no Kubernetes API access). Keep `CRON_JOBS` in sync when a `CronJob`
//! is added; see `docs/cronjobs.md`.

use maud::{html, Markup};

use crate::PageLayout;

/// One scheduled job, as the page needs it.
pub struct CronJobEntry {
    /// Display name.
    pub name: &'static str,
    /// Cron expression (UTC), exactly as deployed.
    pub schedule: &'static str,
    /// Human cadence, in the workspace's Pacific convention.
    pub cadence: &'static str,
    /// What the job does.
    pub description: &'static str,
    /// POST route that triggers the job on demand, if it has one.
    pub manual_run: Option<&'static str>,
}

/// The `CronJob`s Neon Law Navigator ships. Mirrors the manifests under
/// `examples/deploy/k8s/exports/`. Add a row when a `CronJob` is added.
const CRON_JOBS: &[CronJobEntry] = &[
    CronJobEntry {
        name: "Archives nightly export",
        schedule: "0 10 * * *",
        cadence: "Daily · 02:00 PST",
        description: "Snapshots every database table to Parquet on GCS, summarizes GCP cost, \
                      and emails the diagnostic report.",
        manual_run: Some("/portal/admin/archives/run"),
    },
    CronJobEntry {
        name: "NRS statutes sync",
        schedule: "0 10 * * 0",
        cadence: "Weekly · Sun 02:00 PST",
        description: "Scrapes the practice-relevant Nevada Revised Statutes chapters into the \
                      insert-only statutes tables, then emails a Foundation-branded run summary \
                      — a two-step Restate workflow backing the public /statutes reference.",
        manual_run: None,
    },
    CronJobEntry {
        name: "Billing canary",
        schedule: "0 14 * * 0",
        cadence: "Weekly · Sun 06:00 PST",
        description: "Find-or-creates one stable canary contact in Xero, then emails a \
                      confirmation — a two-step Restate workflow that proves the billing \
                      integration still agrees with Xero's API.",
        manual_run: None,
    },
    CronJobEntry {
        name: "Recurring billing",
        schedule: "0 11 * * *",
        cadence: "Daily · 03:00 PST",
        description: "Raises one Xero invoice per active subscription per month for every \
                      recurring product (Nexus, Nautilus). The per-month period guard makes \
                      the daily fire idempotent, so a subscription opened mid-month is billed \
                      on the next run; emails a per-run diagnostic.",
        manual_run: None,
    },
    CronJobEntry {
        name: "Billing digest",
        schedule: "0 13 * * *",
        cadence: "Daily · 05:00 PST",
        description: "Emails firm ops a trailing-30-day GCP cost report — gross / credits / net \
                      by service, the free-trial credit burned to date, and the real cost once \
                      the trial credit expires (gross minus only the perpetual free-tier). A \
                      two-step Restate workflow; a no-op where no billing export is configured.",
        manual_run: None,
    },
    CronJobEntry {
        name: "Durable-execution heartbeat",
        schedule: "0 */6 * * *",
        cadence: "Every 6h · 00/06/12/18 UTC",
        description: "Liveness canary for the durable-execution engine itself — a two-step \
                      Restate workflow (beat → notify) that depends on nothing (no database, \
                      no object storage, no third-party API), so a green run can only mean the \
                      engine accepted an invocation and ran it to completion. Emails firm ops \
                      every six hours with the Restate Cloud + GCP console links to check when \
                      a beat is missing.",
        manual_run: None,
    },
];

#[must_use]
pub fn schedules(csrf_token: &str) -> Markup {
    let body = html! {
        section.admin {
            h1."mb-2" { "Cron schedules" }
            p."text-body-secondary"."mb-4" {
                "Scheduled jobs that run on the cluster, shown with the cron expression (UTC) and "
                "the Pacific cadence. Jobs that expose a trigger can be run on demand here."
            }
            div."table-responsive" {
                table."table"."align-middle".admin-schedules {
                    thead { tr {
                        th { "Job" }
                        th { "Schedule (UTC)" }
                        th { "Cadence" }
                        th { "What it does" }
                        th { "Run now" }
                    } }
                    tbody {
                        @for job in CRON_JOBS {
                            tr {
                                td { strong { (job.name) } }
                                td { code { (job.schedule) } }
                                td { (job.cadence) }
                                td { (job.description) }
                                td {
                                    @if let Some(route) = job.manual_run {
                                        form method="post" action=(route) {
                                            input type="hidden" name="_csrf" value=(csrf_token);
                                            button type="submit" class="btn btn-sm btn-primary" {
                                                "Run now"
                                            }
                                        }
                                    } @else {
                                        span."text-body-secondary" { "—" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    PageLayout::new("Cron schedules")
        .with_description("Neon Law Navigator scheduled-job reference.")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{schedules, CRON_JOBS};

    #[test]
    fn lists_the_archives_export_with_schedule_and_trigger() {
        let html = schedules("tok-9").into_string();
        assert!(html.contains("Archives nightly export"));
        assert!(html.contains("0 10 * * *"));
        assert!(html.contains("02:00 PST"));
        // The manual trigger renders as a CSRF-protected POST form.
        assert!(html.contains("action=\"/portal/admin/archives/run\""));
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"tok-9\""));
        assert!(html.contains("Run now"));
    }

    #[test]
    fn lists_the_statutes_sync_weekly_workflow() {
        let html = schedules("tok-1").into_string();
        assert!(html.contains("NRS statutes sync"));
        assert!(html.contains("0 10 * * 0"));
        assert!(html.contains("Sun 02:00 PST"));
        assert!(html.contains("/statutes"));
    }

    #[test]
    fn lists_the_recurring_billing_daily_workflow() {
        let html = schedules("tok-2").into_string();
        assert!(html.contains("Recurring billing"));
        assert!(html.contains("0 11 * * *"));
        assert!(html.contains("03:00 PST"));
        assert!(html.contains("subscription"));
    }

    #[test]
    fn lists_the_billing_digest_daily_workflow() {
        let html = schedules("tok-4").into_string();
        assert!(html.contains("Billing digest"));
        assert!(html.contains("0 13 * * *"));
        assert!(html.contains("05:00 PST"));
        assert!(html.contains("trailing-30-day GCP cost"));
    }

    #[test]
    fn lists_the_durable_execution_heartbeat() {
        let html = schedules("tok-3").into_string();
        assert!(html.contains("Durable-execution heartbeat"));
        assert!(html.contains("0 */6 * * *"));
        assert!(html.contains("Every 6h"));
        assert!(html.contains("Liveness canary"));
    }

    #[test]
    fn every_job_carries_a_schedule_and_cadence() {
        for job in CRON_JOBS {
            assert!(!job.schedule.is_empty(), "{} missing schedule", job.name);
            assert!(!job.cadence.is_empty(), "{} missing cadence", job.name);
        }
    }
}
