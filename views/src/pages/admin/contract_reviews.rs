//! Admin attorney-review screen for an inbound contract review —
//! `/portal/admin/contract-reviews/:id`.
//!
//! Renders the machine-proposed findings for per-finding attorney action.
//! There is **no bulk-accept**: each finding is its own form with an explicit
//! *Accept* and *Reject* submit; the risk summary is its own form; and the
//! whole review can only be approved once every finding has been acted on.

use maud::{html, Markup};
use uuid::Uuid;

use crate::PageLayout;

/// One finding, as the attorney sees it.
pub struct FindingView<'a> {
    pub index: usize,
    pub clause_ref: &'a str,
    pub deviation: &'a str,
    pub severity: &'a str,
    pub suggested_redline: &'a str,
    pub attorney_note: &'a str,
    pub accepted: bool,
    /// Whether a decision has been recorded for this finding yet.
    pub acted: bool,
}

/// The whole review screen's data.
pub struct ReviewView<'a> {
    pub review_id: Uuid,
    pub playbook_name: &'a str,
    /// `pending` / `analyzed` / `approved` / `rejected`.
    pub status: &'a str,
    pub notation_state: &'a str,
    pub risk_summary: &'a str,
    pub findings: Vec<FindingView<'a>>,
    pub all_acted: bool,
    pub error: Option<&'a str>,
    pub csrf_token: &'a str,
}

#[must_use]
pub fn review_page(v: &ReviewView<'_>) -> Markup {
    let editable = v.status == "analyzed" && v.notation_state == "staff_review";
    let base = format!("/portal/admin/contract-reviews/{}", v.review_id);
    let body = html! {
        section.admin {
            div.container {
                header.page-header {
                    h1 { "Contract review" }
                    p.lead { "Measured against playbook: " strong { (v.playbook_name) } }
                    p { (status_badge(v.status)) }
                }
                @if let Some(err) = v.error {
                    div.alert.alert-danger role="alert" { (err) }
                }
                @if !editable {
                    div.alert.alert-info role="alert" {
                        "This review is " (v.status) " — it is no longer editable."
                    }
                }

                (risk_summary_card(&base, v, editable))

                h2.mt-4 { "Findings" }
                @if v.findings.is_empty() {
                    p.empty { "The analysis flagged no positions." }
                }
                @for f in &v.findings {
                    (finding_card(&base, f, v.csrf_token, editable))
                }

                @if editable {
                    (decision_bar(&base, v))
                }
            }
        }
    };
    PageLayout::new("Contract review — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

fn status_badge(status: &str) -> Markup {
    let cls = match status {
        "approved" => "text-bg-success",
        "rejected" => "text-bg-secondary",
        "analyzed" => "text-bg-warning",
        _ => "text-bg-light",
    };
    html! { span class=(format!("badge {cls}")) { (status) } }
}

fn risk_summary_card(base: &str, v: &ReviewView<'_>, editable: bool) -> Markup {
    let action = format!("{base}/summary");
    html! {
        div.card.mb-3 {
            div.card-body {
                h2.card-title.h5 { "Risk summary" }
                @if editable {
                    form method="post" action=(action) {
                        input type="hidden" name="_csrf" value=(v.csrf_token);
                        textarea.form-control name="risk_summary" rows="4" { (v.risk_summary) }
                        button.btn.btn-outline-primary.btn-sm.mt-2 type="submit" { "Save summary" }
                    }
                } @else {
                    p.card-text { (v.risk_summary) }
                }
            }
        }
    }
}

fn finding_card(base: &str, f: &FindingView<'_>, csrf: &str, editable: bool) -> Markup {
    let action = format!("{base}/findings/{}", f.index);
    html! {
        div.card.mb-3 {
            div.card-body {
                div.d-flex.justify-content-between.align-items-start {
                    h3.card-title.h6 { (f.clause_ref) }
                    (decision_badge(f))
                }
                p.card-text { (f.deviation) }
                @if editable {
                    form method="post" action=(action) {
                        input type="hidden" name="_csrf" value=(csrf);
                        div.mb-2 {
                            label.form-label { "Severity" }
                            select.form-select name="severity" {
                                @for (val, label) in [("low", "Low"), ("medium", "Medium"), ("high", "High")] {
                                    option value=(val) selected[f.severity == val] { (label) }
                                }
                            }
                        }
                        div.mb-2 {
                            label.form-label { "Suggested redline" }
                            textarea.form-control name="suggested_redline" rows="2" { (f.suggested_redline) }
                        }
                        div.mb-2 {
                            label.form-label { "Attorney note" }
                            textarea.form-control name="attorney_note" rows="2" { (f.attorney_note) }
                        }
                        button.btn.btn-success.btn-sm.me-2 type="submit" name="decision" value="accept" {
                            "Accept"
                        }
                        button.btn.btn-outline-secondary.btn-sm type="submit" name="decision" value="reject" {
                            "Reject"
                        }
                    }
                } @else {
                    @if !f.suggested_redline.is_empty() {
                        p.card-text { strong { "Suggested redline: " } (f.suggested_redline) }
                    }
                    @if !f.attorney_note.is_empty() {
                        p.card-text { strong { "Attorney note: " } (f.attorney_note) }
                    }
                }
            }
        }
    }
}

fn decision_badge(f: &FindingView<'_>) -> Markup {
    html! {
        @if !f.acted {
            span.badge.text-bg-warning { "Needs action" }
        } @else if f.accepted {
            span.badge.text-bg-success { "Accepted" }
        } @else {
            span.badge.text-bg-secondary { "Rejected" }
        }
    }
}

fn decision_bar(base: &str, v: &ReviewView<'_>) -> Markup {
    let approve = format!("{base}/approve");
    let reject = format!("{base}/reject");
    html! {
        div.d-flex.gap-2.mt-4 {
            form method="post" action=(approve) {
                input type="hidden" name="_csrf" value=(v.csrf_token);
                button.btn.btn-primary type="submit" disabled[!v.all_acted] {
                    "Approve & deliver memo"
                }
            }
            form method="post" action=(reject) {
                input type="hidden" name="_csrf" value=(v.csrf_token);
                button.btn.btn-outline-danger type="submit" { "Reject review" }
            }
        }
        @if !v.all_acted {
            p.text-muted.mt-2 {
                "Act on every finding (accept or reject) before approving."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{review_page, FindingView, ReviewView};
    use uuid::Uuid;

    const RID: Uuid = Uuid::from_u128(7);

    fn finding(acted: bool, accepted: bool) -> FindingView<'static> {
        FindingView {
            index: 0,
            clause_ref: "§7.2 Liability",
            deviation: "Liability is uncapped.",
            severity: "high",
            suggested_redline: "Add a mutual cap.",
            attorney_note: "",
            accepted,
            acted,
        }
    }

    fn view(findings: Vec<FindingView<'static>>, all_acted: bool) -> ReviewView<'static> {
        ReviewView {
            review_id: RID,
            playbook_name: "Vendor MSA",
            status: "analyzed",
            notation_state: "staff_review",
            risk_summary: "One high-severity deviation.",
            findings,
            all_acted,
            error: None,
            csrf_token: "TOK",
        }
    }

    #[test]
    fn editable_review_renders_per_finding_accept_reject_forms() {
        let html = review_page(&view(vec![finding(false, false)], false)).into_string();
        assert!(html.contains("Vendor MSA"));
        assert!(html.contains("§7.2 Liability"));
        // Per-finding accept/reject submit buttons, not a bulk action.
        assert!(html.contains("value=\"accept\""));
        assert!(html.contains("value=\"reject\""));
        assert!(html.contains(&format!(
            "action=\"/portal/admin/contract-reviews/{RID}/findings/0\""
        )));
        assert!(html.contains("Needs action"));
        // CSRF threaded into the forms.
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"TOK\""));
    }

    #[test]
    fn approve_is_disabled_until_all_findings_acted() {
        let html = review_page(&view(vec![finding(false, false)], false)).into_string();
        assert!(html.contains("Approve &amp; deliver memo"));
        assert!(html.contains("disabled"));
        assert!(html.contains("Act on every finding"));
    }

    #[test]
    fn approve_enabled_when_all_acted() {
        let html = review_page(&view(vec![finding(true, true)], true)).into_string();
        assert!(html.contains("Accepted"));
        // The approve button is present.
        assert!(html.contains("Approve &amp; deliver memo"));
    }

    #[test]
    fn approved_review_is_read_only() {
        let mut v = view(vec![finding(true, true)], true);
        v.status = "approved";
        v.notation_state = "END";
        let html = review_page(&v).into_string();
        assert!(html.contains("no longer editable"));
        // No accept/reject submit buttons in read-only mode.
        assert!(!html.contains("value=\"accept\""));
    }
}
