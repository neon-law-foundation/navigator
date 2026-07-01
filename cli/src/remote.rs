//! Authenticated HTTP client for the `navigator` CLI.
//!
//! Every command here is a thin wrapper over an **existing** `web` route,
//! sent with `Authorization: Bearer <token>` — no parallel JSON API. The
//! server resolves the bearer back into the caller's session and runs the
//! same handler the browser does, so `is_staff_tier`, the `staff_review`
//! gate, and `authored_by` provenance all hold unchanged.
//!
//! | command | route |
//! | --- | --- |
//! | `projects list` | `GET /portal/projects.csv` |
//! | `project open`   | `POST /portal/projects` (303 → review URL) |
//! | `notation create`  | `POST /portal/admin/retainers/new` |
//! | `retainer approve` | `POST /portal/admin/notations/:id/approve-send` |
//! | `retainer send`    | `POST /portal/admin/notations/:id/send` |
//! | `notation status`  | `GET /portal/admin/notations/:id/review?format=json` |

use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use comfy_table::{presets::UTF8_FULL, Cell, ContentArrangement, Table};
use uuid::Uuid;

use crate::credentials::{self, default_credentials_path, HostCredential};
use crate::login::resolve_base;
use crate::palette;

/// Fields for `navigator project open`.
pub struct MatterOpen {
    pub name: String,
    pub template: String,
    pub client_name: String,
    pub client_email: String,
    pub scope: String,
    pub description: String,
}

/// Resolve `(base_url, bearer_token)` for `host`, erroring clearly when
/// there's no login or the stored token has expired.
fn resolve(host: Option<&str>) -> Result<(String, String)> {
    let creds = credentials::load(&default_credentials_path())?;
    let base = resolve_base(host, &creds)?;
    let cred: &HostCredential = creds
        .get(&base)
        .ok_or_else(|| anyhow!("not logged in to {base} — run `navigator login --host …`"))?;
    if cred.is_expired(now_secs()) {
        return Err(anyhow!(
            "the stored token for {base} has expired — run `navigator login --host {base}`"
        ));
    }
    Ok((base, cred.token.clone()))
}

/// `navigator projects list [--host h] [--json]`.
pub async fn projects_list(host: Option<&str>, json: bool) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let resp = reqwest::Client::new()
            .get(format!("{base}/portal/projects.csv"))
            .bearer_auth(&token)
            .send()
            .await
            .context("GET /portal/projects.csv")?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("projects list failed: {status}"));
        }
        let rows = parse_csv(&body);
        print_projects(&rows, json)?;
        Ok(())
    })
    .await
}

/// `navigator project open …` — POST the matter-open form and surface the
/// notation id + the review URL the server redirects to.
pub async fn matter_open(host: Option<&str>, m: &MatterOpen) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        // No-redirect client so we can read the 303 `Location` (the review
        // URL) instead of following it.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("build http client")?;
        let resp = client
            .post(format!("{base}/portal/projects"))
            .bearer_auth(&token)
            .form(&[
                ("name", m.name.as_str()),
                ("status", "open"),
                ("send_retainer", "true"),
                ("retainer_template_code", m.template.as_str()),
                ("custom_text__client_name", m.client_name.as_str()),
                ("custom_text__client_email", m.client_email.as_str()),
                ("scope_of_services", m.scope.as_str()),
                ("description", m.description.as_str()),
            ])
            .send()
            .await
            .context("POST /portal/projects")?;
        let status = resp.status();
        if status.as_u16() != 303 {
            // A validation failure re-renders the form as 422; anything
            // other than the redirect means no matter was opened.
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "project open did not park at the review screen (status {status}). \
                 The server reported: {}",
                first_line(&body),
            ));
        }
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let notation_id = location
            .trim_start_matches("/portal/admin/notations/")
            .trim_end_matches("/review");
        println!(
            "{} {}",
            palette::dim("opened matter; retainer parked at staff_review — notation"),
            palette::highlight(notation_id),
        );
        println!("{} {base}{location}", palette::dim("review:"));
        println!(
            "{}",
            palette::dim(format!(
                "approve (render + park) with: navigator retainer approve {notation_id}\n\
                 then dispatch the envelope with: navigator retainer send {notation_id}"
            )),
        );
        Ok(())
    })
    .await
}

/// Fields for `navigator subscription create`.
pub struct SubscriptionCreate {
    pub product: String,
    pub contact_name: String,
    pub contact_email: String,
    pub coupon: Option<String>,
    pub discount_percent: Option<i64>,
    pub discount_amount_cents: Option<i64>,
    pub project_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
    pub person_id: Option<Uuid>,
    pub active: bool,
}

/// Fields for `navigator coupon create`.
pub struct CouponCreate {
    pub code: String,
    pub discount_percent: Option<i64>,
    pub discount_amount_cents: Option<i64>,
    pub product: Option<String>,
    pub expires: Option<String>,
    pub max_redemptions: Option<i64>,
}

/// Pull a human message out of an error response body — the handler's
/// `{"error": …}` JSON, else the first line of whatever came back.
fn error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| first_line(body).clone())
}

/// Render a subscription/coupon JSON object's discount for a table cell.
fn fmt_discount(v: &serde_json::Value) -> String {
    if let Some(p) = v
        .get("discount_percent")
        .and_then(serde_json::Value::as_i64)
    {
        return format!("{p}%");
    }
    if let Some(c) = v
        .get("discount_amount_cents")
        .and_then(serde_json::Value::as_i64)
    {
        return format!("${}.{:02} off", c / 100, c % 100);
    }
    "—".to_string()
}

/// `navigator subscription create …` — open a recurring subscription via
/// `POST /portal/admin/subscriptions?format=json`. Starts `pending` (so it
/// is not billed until the project's retainer is signed) unless `--active`.
pub async fn subscription_create(host: Option<&str>, s: &SubscriptionCreate) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let mut form: Vec<(&str, String)> = vec![
            ("product_code", s.product.clone()),
            ("contact_name", s.contact_name.clone()),
            ("contact_email", s.contact_email.clone()),
        ];
        if let Some(c) = &s.coupon {
            form.push(("coupon", c.clone()));
        }
        if let Some(p) = s.discount_percent {
            form.push(("discount_percent", p.to_string()));
        }
        if let Some(a) = s.discount_amount_cents {
            form.push(("discount_amount_cents", a.to_string()));
        }
        if let Some(p) = s.project_id {
            form.push(("project_id", p.to_string()));
        }
        if let Some(e) = s.entity_id {
            form.push(("entity_id", e.to_string()));
        }
        if let Some(p) = s.person_id {
            form.push(("person_id", p.to_string()));
        }
        if s.active {
            form.push(("active", "true".to_string()));
        }
        let resp = reqwest::Client::new()
            .post(format!("{base}/portal/admin/subscriptions?format=json"))
            .bearer_auth(&token)
            .form(&form)
            .send()
            .await
            .context("POST /portal/admin/subscriptions")?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "subscription create failed ({status}): {}",
                error_message(&body)
            ));
        }
        let v: serde_json::Value =
            serde_json::from_str(&body).context("parse subscription json")?;
        let id = v
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let st = v
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        println!(
            "{} {}",
            palette::dim("opened subscription"),
            palette::highlight(id),
        );
        println!(
            "{} {}  ·  {} {}",
            palette::dim("product:"),
            s.product,
            palette::dim("status:"),
            st,
        );
        if st == "pending" {
            println!(
                "{}",
                palette::dim(
                    "pending until the project's retainer is signed — it activates automatically \
                     on signature; until then the recurring-billing run skips it"
                ),
            );
        }
        Ok(())
    })
    .await
}

/// `navigator subscription list` — `GET /portal/admin/subscriptions?format=json`.
pub async fn subscriptions_list(host: Option<&str>, json: bool) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let resp = reqwest::Client::new()
            .get(format!("{base}/portal/admin/subscriptions?format=json"))
            .bearer_auth(&token)
            .send()
            .await
            .context("GET /portal/admin/subscriptions")?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("subscriptions list failed: {status}"));
        }
        if json {
            println!("{body}");
            return Ok(());
        }
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&body).context("parse subscriptions json")?;
        if rows.is_empty() {
            println!("{}", palette::dim("no subscriptions"));
            return Ok(());
        }
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                Cell::new("product"),
                Cell::new("contact"),
                Cell::new("status"),
                Cell::new("discount"),
                Cell::new("last billed"),
            ]);
        for r in &rows {
            let get = |k: &str| {
                r.get(k)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string()
            };
            table.add_row(vec![
                Cell::new(get("product_code")),
                Cell::new(get("contact_email")),
                Cell::new(get("status")),
                Cell::new(fmt_discount(r)),
                Cell::new(
                    r.get("last_invoiced_period")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("—"),
                ),
            ]);
        }
        println!("{table}");
        Ok(())
    })
    .await
}

/// `navigator coupon create …` — mint a reusable discount via
/// `POST /portal/admin/coupons?format=json`.
pub async fn coupon_create(host: Option<&str>, c: &CouponCreate) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let mut form: Vec<(&str, String)> = vec![("code", c.code.clone())];
        if let Some(p) = c.discount_percent {
            form.push(("discount_percent", p.to_string()));
        }
        if let Some(a) = c.discount_amount_cents {
            form.push(("discount_amount_cents", a.to_string()));
        }
        if let Some(p) = &c.product {
            form.push(("product_code", p.clone()));
        }
        if let Some(e) = &c.expires {
            form.push(("expires_at", e.clone()));
        }
        if let Some(m) = c.max_redemptions {
            form.push(("max_redemptions", m.to_string()));
        }
        let resp = reqwest::Client::new()
            .post(format!("{base}/portal/admin/coupons?format=json"))
            .bearer_auth(&token)
            .form(&form)
            .send()
            .await
            .context("POST /portal/admin/coupons")?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "coupon create failed ({status}): {}",
                error_message(&body)
            ));
        }
        let v: serde_json::Value = serde_json::from_str(&body).context("parse coupon json")?;
        println!(
            "{} {}  ·  {} {}",
            palette::dim("minted coupon"),
            palette::highlight(c.code.as_str()),
            palette::dim("discount:"),
            fmt_discount(&v),
        );
        Ok(())
    })
    .await
}

/// `navigator coupon list` — `GET /portal/admin/coupons?format=json`.
pub async fn coupons_list(host: Option<&str>, json: bool) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let resp = reqwest::Client::new()
            .get(format!("{base}/portal/admin/coupons?format=json"))
            .bearer_auth(&token)
            .send()
            .await
            .context("GET /portal/admin/coupons")?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("coupons list failed: {status}"));
        }
        if json {
            println!("{body}");
            return Ok(());
        }
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&body).context("parse coupons json")?;
        if rows.is_empty() {
            println!("{}", palette::dim("no coupons"));
            return Ok(());
        }
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                Cell::new("code"),
                Cell::new("discount"),
                Cell::new("scope"),
                Cell::new("redeemed"),
                Cell::new("expires"),
                Cell::new("active"),
            ]);
        for r in &rows {
            let redeemed = match (
                r.get("redeemed_count").and_then(serde_json::Value::as_i64),
                r.get("max_redemptions").and_then(serde_json::Value::as_i64),
            ) {
                (Some(used), Some(max)) => format!("{used} / {max}"),
                (Some(used), None) => used.to_string(),
                _ => "0".to_string(),
            };
            table.add_row(vec![
                Cell::new(
                    r.get("code")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                ),
                Cell::new(fmt_discount(r)),
                Cell::new(
                    r.get("product_code")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("any"),
                ),
                Cell::new(redeemed),
                Cell::new(
                    r.get("expires_at")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("never"),
                ),
                Cell::new(
                    r.get("active")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                ),
            ]);
        }
        println!("{table}");
        Ok(())
    })
    .await
}

/// `navigator notation create <template-code> --client-email …` — create a
/// questionnaire-driven notation through the walker entry the browser uses
/// (`POST /portal/admin/retainers/new`) and surface the notation id. Unlike
/// `project open` — which opens a matter *and* sends a retainer in one
/// action, parking at `staff_review` — this leaves the questionnaire ready
/// to walk from the terminal with `intake answer`.
pub async fn notation_create(
    host: Option<&str>,
    template: &str,
    client_email: &str,
    project: Option<&str>,
) -> ExitCode {
    run(async {
        if let Some(project) = project {
            return Err(anyhow!(
                "project-scoped notation creation is not available through this live-site route yet. \
                 Omit `--project` for bundled/template-example catalog templates, or use the future \
                 project-scoped path with `--project {project}` once it lands."
            ));
        }
        let (base, token) = resolve(host)?;
        // No-redirect client so we read the 303 `Location` (the step URL)
        // rather than following it into the walker's HTML.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("build http client")?;
        let resp = client
            .post(format!("{base}/portal/admin/retainers/new"))
            .bearer_auth(&token)
            .form(&[
                ("client_email", client_email),
                ("custom_text__client_email", client_email),
                ("retainer_template_code", template),
            ])
            .send()
            .await
            .context("POST /portal/admin/retainers/new")?;
        let status = resp.status();
        if status.as_u16() != 303 {
            // The walker re-renders its form (200) on a bad email/template;
            // anything but the redirect means no notation was created.
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "notation create did not start the questionnaire (status {status}). \
                 The server reported: {}",
                first_line(&body),
            ));
        }
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let notation_id = location
            .trim_start_matches("/portal/admin/notations/")
            .trim_end_matches("/step");
        println!(
            "{} {}",
            palette::dim("created notation; questionnaire ready — notation"),
            palette::highlight(notation_id),
        );
        println!(
            "{}",
            palette::dim(format!(
                "answer it from the terminal with: navigator intake answer {notation_id}"
            )),
        );
        Ok(())
    })
    .await
}

/// `navigator intake answer <id>` — walk the notation's questionnaire one
/// question at a time over the same `/portal/admin/notations/:id/step`
/// route the browser POSTs, reading each question's metadata from the
/// `?format=json` branch. Interactive by default (prompts at the
/// terminal); non-interactive when `--answer` / `--person` flags are
/// supplied, consuming scalar answers in order and the people rows for the
/// first `people_list` question.
pub async fn intake_answer(
    host: Option<&str>,
    notation_id: Uuid,
    answers: Vec<String>,
    persons: Vec<String>,
) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let client = reqwest::Client::new();
        let interactive = answers.is_empty() && persons.is_empty();
        // Fail fast on a malformed `--person` before touching the server.
        let parsed_persons: Vec<Vec<(String, String)>> = persons
            .iter()
            .map(|s| crate::intake::parse_person(s))
            .collect::<Result<_>>()?;
        let mut answer_queue: VecDeque<String> = answers.into();
        let mut persons_consumed = false;
        let mut answered = 0u32;

        loop {
            let step = fetch_step(&client, &base, &token, notation_id).await?;
            let Some(question) = step.question else {
                if answered == 0 {
                    println!(
                        "{}",
                        palette::dim(format!("notation {notation_id} has no open questions"))
                    );
                } else {
                    println!(
                        "{} {} ({answered} answered)",
                        palette::dim("questionnaire complete — notation"),
                        palette::highlight(notation_id.to_string()),
                    );
                }
                break;
            };

            let fields: Vec<(String, String)> =
                if store::question_registry::answer_type_is_aggregate(&question.answer_type) {
                    let rows = if interactive {
                        read_people_list(&question)?
                    } else {
                        if persons_consumed {
                            return Err(anyhow!(
                            "question `{}` is a people_list but every --person row was already \
                             consumed by an earlier one; this matter has more than one — answer \
                             it interactively",
                            question.code,
                        ));
                        }
                        persons_consumed = true;
                        crate::intake::people_list_fields(&parsed_persons)
                    };
                    rows
                } else {
                    let value = if interactive {
                        prompt_scalar(&question)?
                    } else {
                        answer_queue.pop_front().ok_or_else(|| {
                            anyhow!(
                                "ran out of --answer values at question `{}` ({})",
                                question.code,
                                question.prompt,
                            )
                        })?
                    };
                    vec![("value".to_string(), value)]
                };

            let resp = client
                .post(format!("{base}/portal/admin/notations/{notation_id}/step"))
                .bearer_auth(&token)
                .form(&fields)
                .send()
                .await
                .context("POST step")?;
            let status = resp.status();
            if !status.is_success() && !status.is_redirection() {
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow!(
                    "answering `{}` failed: {}",
                    question.code,
                    server_error(status, &body),
                ));
            }
            println!(
                "{} {}",
                palette::dim("answered"),
                palette::highlight(&question.code)
            );
            answered += 1;
        }
        Ok(())
    })
    .await
}

/// `navigator notation approve <id>` — render + park the notation's
/// document (`POST …/approve-send`). The generic sibling of `retainer
/// approve`: it fills the bound packet (a formation's official Secretary-of-State form,
/// or a retainer PDF) for attorney review. Idempotent server-side — a
/// re-approve once the PDF exists is a success, not an error.
pub async fn notation_approve(host: Option<&str>, notation_id: Uuid) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let resp = reqwest::Client::new()
            .post(format!(
                "{base}/portal/admin/notations/{notation_id}/approve-send"
            ))
            .bearer_auth(&token)
            // `Content-Length: 0` for the same LB gotcha as `retainer approve`.
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .body(Vec::<u8>::new())
            .send()
            .await
            .context("POST approve-send")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("approve failed: {}", server_error(status, &body)));
        }
        let st = fetch_status(&base, &token, notation_id).await?;
        println!(
            "{} {} — state {} (document_ready {})",
            palette::dim("approved — notation"),
            palette::highlight(notation_id.to_string()),
            palette::highlight(&st.state),
            st.document_ready,
        );
        Ok(())
    })
    .await
}

/// `navigator notation document <id> --out <path>` — download the
/// notation's rendered document (the filled packet) to a local file via
/// the participation-gated `…/documents/document` route, the same per-
/// notation PDF the review surface shows.
pub async fn notation_document(host: Option<&str>, notation_id: Uuid, out: &Path) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        // Follow redirects: a signed-URL storage backend 302s to the blob;
        // the FsStorage dev backend streams 200 through the app.
        let resp = reqwest::Client::new()
            .get(format!(
                "{base}/portal/admin/notations/{notation_id}/documents/document"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .context("GET document")?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(anyhow!(
                "no rendered document for notation {notation_id} — answer the questionnaire \
                 (or `navigator notation approve {notation_id}`) first"
            ));
        }
        if !status.is_success() {
            return Err(anyhow!("document download failed: {status}"));
        }
        let bytes = resp.bytes().await.context("read document bytes")?;
        std::fs::write(out, &bytes).with_context(|| format!("write {}", out.display()))?;
        println!(
            "{} {} ({} bytes)",
            palette::dim("wrote the filled packet to"),
            palette::highlight(out.display().to_string()),
            bytes.len(),
        );
        Ok(())
    })
    .await
}

/// `navigator retainer approve <id>` — POST approve-send. This renders +
/// parks: the worker durably renders + persists the retainer PDF and the
/// workflow waits at `document_open__retainer_pdf`. It does NOT send — the
/// binding envelope goes out only on the separate `retainer send`, after
/// the PDF is confirmed present.
pub async fn retainer_approve(host: Option<&str>, notation_id: Uuid) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let resp = reqwest::Client::new()
            .post(format!(
                "{base}/portal/admin/notations/{notation_id}/approve-send"
            ))
            .bearer_auth(&token)
            // Force `Content-Length: 0`. The handler takes no form fields,
            // but a bodyless POST carries no length header, and GCP's HTTPS
            // load balancer rejects that with `411 Length Required` before
            // the request ever reaches the app. reqwest omits the header for
            // an empty body, so set it explicitly.
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .body(Vec::<u8>::new())
            .send()
            .await
            .context("POST approve-send")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("approve failed: {}", server_error(status, &body)));
        }
        // Read the authoritative post-state for the operator.
        let st = fetch_status(&base, &token, notation_id).await?;
        println!(
            "{} {} — state {} (document_ready {})",
            palette::dim("approved; worker rendering the retainer PDF — notation"),
            palette::highlight(notation_id.to_string()),
            palette::highlight(&st.state),
            st.document_ready,
        );
        println!(
            "{}",
            palette::dim(format!(
                "dispatch the envelope with: navigator retainer send {notation_id}"
            )),
        );
        Ok(())
    })
    .await
}

/// `navigator retainer send <id>` — POST the deliberate send. On prod this
/// emits exactly one real envelope, so it is a deliberate authenticated
/// human command (never an LLM-routable tool). Honors the readiness gate:
/// a `409` means the worker hasn't rendered the PDF yet — print the
/// server's reason and exit non-zero so the operator retries rather than
/// the command silently loops against a misconfigured worker.
pub async fn retainer_send(host: Option<&str>, notation_id: Uuid) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        // `Content-Length: 0` for the same LB gotcha as `retainer approve`.
        let resp = reqwest::Client::new()
            .post(format!("{base}/portal/admin/notations/{notation_id}/send"))
            .bearer_auth(&token)
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .body(Vec::<u8>::new())
            .send()
            .await
            .context("POST send")?;
        let status = resp.status();
        if status.as_u16() == 409 {
            // Not yet: the PDF isn't rendered. Print the server's reason
            // verbatim and tell the operator to retry.
            let body = resp.text().await.unwrap_or_default();
            let reason = json_reason(&body).unwrap_or_else(|| "document not ready yet".to_string());
            return Err(anyhow!(
                "not ready to send: {reason}\n\
                 retry: navigator retainer send {notation_id}"
            ));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("send failed: {}", server_error(status, &body)));
        }
        let st = fetch_status(&base, &token, notation_id).await?;
        println!(
            "{} {} — state {}{}",
            palette::dim("sent for signature; notation"),
            palette::highlight(notation_id.to_string()),
            palette::highlight(&st.state),
            st.signature_request_id
                .as_deref()
                .map(|id| format!(" (signature request {id})"))
                .unwrap_or_default(),
        );
        Ok(())
    })
    .await
}

/// `navigator retainer clause list <id>` — print the notation's custom
/// clauses from the clause editor's `?format=json` branch.
pub async fn clause_list(host: Option<&str>, notation_id: Uuid, json: bool) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let resp = reqwest::Client::new()
            .get(format!(
                "{base}/portal/admin/notations/{notation_id}/clauses?format=json"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .context("GET clauses (json)")?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(anyhow!("no notation {notation_id} on {base}"));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "clause list failed: {}",
                server_error(status, &body)
            ));
        }
        let body = resp.text().await.unwrap_or_default();
        if json {
            println!("{body}");
            return Ok(());
        }
        let clauses: Vec<Clause> = serde_json::from_str(&body).context("parse clauses json")?;
        if clauses.is_empty() {
            println!("{}", palette::dim("no custom clauses on this notation"));
            return Ok(());
        }
        for c in &clauses {
            let provenance = if c.system_authored {
                palette::dim("[system draft]")
            } else {
                palette::dim("[staff]")
            };
            println!(
                "{} {} {}\n    {}",
                palette::highlight(format!("#{}", c.position)),
                provenance,
                palette::dim(c.id.to_string()),
                c.body.replace('\n', "\n    "),
            );
        }
        Ok(())
    })
    .await
}

/// `navigator retainer clause add <id> --body …` — append one clause.
pub async fn clause_add(host: Option<&str>, notation_id: Uuid, body: &str) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let resp = reqwest::Client::new()
            .post(format!(
                "{base}/portal/admin/notations/{notation_id}/clauses"
            ))
            .bearer_auth(&token)
            .form(&[("body", body)])
            .send()
            .await
            .context("POST clause add")?;
        clause_write_result(resp, notation_id, "added").await
    })
    .await
}

/// `navigator retainer clause edit <id> <cid> --body …` — replace a body.
pub async fn clause_edit(
    host: Option<&str>,
    notation_id: Uuid,
    clause_id: Uuid,
    body: &str,
) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let resp = reqwest::Client::new()
            .post(format!(
                "{base}/portal/admin/notations/{notation_id}/clauses/{clause_id}/edit"
            ))
            .bearer_auth(&token)
            .form(&[("body", body)])
            .send()
            .await
            .context("POST clause edit")?;
        clause_write_result(resp, notation_id, "updated").await
    })
    .await
}

/// Shared handler for a clause add/edit response: the routes 303-redirect
/// back to the clause page on success.
async fn clause_write_result(resp: reqwest::Response, notation_id: Uuid, verb: &str) -> Result<()> {
    let status = resp.status();
    // The clause routes redirect (303/302) on success; a no-redirect client
    // is not used here, so reqwest follows it and we land on the 200 page.
    if !status.is_success() && status.as_u16() != 303 {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "clause {verb} failed: {}",
            server_error(status, &body)
        ));
    }
    println!(
        "{} {}",
        palette::dim(format!("clause {verb} on notation")),
        palette::highlight(notation_id.to_string()),
    );
    Ok(())
}

/// One row of the clause editor's `?format=json` body.
#[derive(Debug, serde::Deserialize)]
struct Clause {
    id: Uuid,
    position: i32,
    body: String,
    #[serde(default)]
    system_authored: bool,
}

/// `navigator notation status <id>` — print the workflow state + the
/// signature request id from the review handler's JSON branch.
pub async fn notation_status(host: Option<&str>, notation_id: Uuid, json: bool) -> ExitCode {
    run(async {
        let (base, token) = resolve(host)?;
        let st = fetch_status(&base, &token, notation_id).await?;
        if json {
            println!("{}", serde_json::to_string_pretty(&st)?);
        } else {
            println!(
                "{} state {}{} (delivery {}, document_ready {})",
                palette::dim(format!("notation {notation_id}")),
                palette::highlight(&st.state),
                st.signature_request_id
                    .as_deref()
                    .map(|id| format!(", signature request {id}"))
                    .unwrap_or_default(),
                st.delivery.as_deref().unwrap_or("—"),
                st.document_ready,
            );
        }
        Ok(())
    })
    .await
}

/// The review handler's `?format=json` body.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NotationStatus {
    state: String,
    #[serde(default)]
    signature_request_id: Option<String>,
    #[serde(default)]
    delivery: Option<String>,
    /// Whether the worker has rendered + persisted the document PDF — the
    /// gate `retainer send` honors. Defaults to `false` for an older
    /// server that doesn't yet emit the field.
    #[serde(default)]
    document_ready: bool,
}

/// Render a non-2xx server response into an actionable line. The app's
/// error routes answer with a JSON `{error, reason}` body (the council's
/// "no opaque 500" point); fall back to the first line of a plain-text
/// body when the response isn't that shape.
fn server_error(status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let error = v.get("error").and_then(serde_json::Value::as_str);
        let reason = v.get("reason").and_then(serde_json::Value::as_str);
        match (error, reason) {
            (Some(e), Some(r)) => return format!("{status}: {e} — {r}"),
            (_, Some(r)) => return format!("{status}: {r}"),
            (Some(e), _) => return format!("{status}: {e}"),
            _ => {}
        }
    }
    format!("{status}: {}", first_line(body))
}

/// The `reason` field of a server JSON error body, if present.
fn json_reason(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

/// One step of the questionnaire walker's `?format=json` body.
#[derive(Debug, serde::Deserialize)]
struct StepResponse {
    /// The next question, or `None` once the questionnaire reaches END.
    #[serde(default)]
    question: Option<StepQuestion>,
}

/// The question metadata the walker shows for one step.
#[derive(Debug, serde::Deserialize)]
struct StepQuestion {
    code: String,
    prompt: String,
    answer_type: String,
    /// `(value, label)` choices for a `radio`; empty otherwise.
    #[serde(default)]
    choices: Vec<StepChoice>,
}

#[derive(Debug, serde::Deserialize)]
struct StepChoice {
    value: String,
    label: String,
}

/// GET the current questionnaire step as JSON.
async fn fetch_step(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    notation_id: Uuid,
) -> Result<StepResponse> {
    let resp = client
        .get(format!(
            "{base}/portal/admin/notations/{notation_id}/step?format=json"
        ))
        .bearer_auth(token)
        .send()
        .await
        .context("GET step (json)")?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Err(anyhow!("no notation {notation_id} on {base}"));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("step failed: {}", server_error(status, &body)));
    }
    resp.json::<StepResponse>().await.context("parse step json")
}

/// Interactively read one scalar answer, showing a `radio`'s choices.
fn prompt_scalar(question: &StepQuestion) -> Result<String> {
    println!("{}", palette::highlight(&question.prompt));
    if !question.choices.is_empty() {
        println!("{}", palette::dim("choices:"));
        for c in &question.choices {
            println!("  {} — {}", palette::highlight(&c.value), c.label);
        }
    }
    print!("{} ", palette::dim(format!("{}>", question.code)));
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .context("read answer from stdin")?;
    Ok(line.trim().to_string())
}

/// Interactively read a `people_list` answer row by row; a blank name
/// ends the list. Returns the assembled `p{row}_{part}` form fields.
fn read_people_list(question: &StepQuestion) -> Result<Vec<(String, String)>> {
    println!("{}", palette::highlight(&question.prompt));
    println!(
        "{}",
        palette::dim("enter each person; a blank name ends the list")
    );
    let stdin = std::io::stdin();
    let mut rows: Vec<Vec<(String, String)>> = Vec::new();
    loop {
        print!("{}", palette::dim("name (blank to finish)> "));
        std::io::stdout().flush().ok();
        let mut name = String::new();
        stdin.lock().read_line(&mut name).context("read name")?;
        let name = name.trim().to_string();
        if name.is_empty() {
            break;
        }
        let mut row = vec![("name".to_string(), name)];
        for part in &crate::intake::PARTS[1..] {
            print!("{}", palette::dim(format!("{part}> ")));
            std::io::stdout().flush().ok();
            let mut value = String::new();
            stdin
                .lock()
                .read_line(&mut value)
                .with_context(|| format!("read {part}"))?;
            let value = value.trim().to_string();
            if !value.is_empty() {
                row.push(((*part).to_string(), value));
            }
        }
        rows.push(row);
    }
    Ok(crate::intake::people_list_fields(&rows))
}

async fn fetch_status(base: &str, token: &str, notation_id: Uuid) -> Result<NotationStatus> {
    let resp = reqwest::Client::new()
        .get(format!(
            "{base}/portal/admin/notations/{notation_id}/review?format=json"
        ))
        .bearer_auth(token)
        .send()
        .await
        .context("GET notation review (json)")?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Err(anyhow!("no notation {notation_id} on {base}"));
    }
    if !status.is_success() {
        return Err(anyhow!("notation status failed: {status}"));
    }
    resp.json::<NotationStatus>()
        .await
        .context("parse notation status json")
}

fn print_projects(rows: &[Vec<String>], json: bool) -> Result<()> {
    let Some((header, data)) = rows.split_first() else {
        // Not even a header line — empty body.
        if json {
            println!("[]");
        } else {
            println!("{}", palette::dim("no projects"));
        }
        return Ok(());
    };
    if json {
        let objects: Vec<serde_json::Map<String, serde_json::Value>> = data
            .iter()
            .map(|row| {
                header
                    .iter()
                    .zip(row.iter())
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect()
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&objects)?);
        return Ok(());
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(header.iter().map(|h| Cell::new(palette::header(h))));
    for row in data {
        table.add_row(row.iter().map(Cell::new));
    }
    println!("{table}");
    println!("{}", palette::dim(format!("{} project(s)", data.len())));
    Ok(())
}

/// First non-empty line of a response body, trimmed — for terse error
/// reporting without dumping a whole HTML page.
fn first_line(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("(empty response)")
        .chars()
        .take(200)
        .collect()
}

/// Minimal RFC 4180 reader: comma-separated fields, `\r\n` or `\n`
/// records, `"`-quoted fields with doubled internal quotes. Mirrors the
/// server's `admin_csv` writer so the round-trip is exact.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut field = String::new();
    let mut record = Vec::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    let mut saw_any = false;

    while let Some(c) = chars.next() {
        saw_any = true;
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' => in_quotes = true,
                ',' => {
                    record.push(std::mem::take(&mut field));
                }
                '\r' => { /* swallow; the '\n' ends the record */ }
                '\n' => {
                    record.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut record));
                }
                _ => field.push(c),
            }
        }
    }
    // Trailing record with no final newline.
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        rows.push(record);
    }
    let _ = saw_any;
    rows
}

/// Drive an async fallible command to an `ExitCode`, printing any error.
async fn run<F>(fut: F) -> ExitCode
where
    F: std::future::Future<Output = Result<()>>,
{
    match fut.await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("navigator: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::parse_csv;

    #[test]
    fn parses_plain_rows() {
        let csv = "id,name,status\r\n1,Aries,open\r\n2,Taurus,closed\r\n";
        let rows = parse_csv(csv);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], vec!["id", "name", "status"]);
        assert_eq!(rows[1], vec!["1", "Aries", "open"]);
        assert_eq!(rows[2], vec!["2", "Taurus", "closed"]);
    }

    #[test]
    fn parses_quoted_fields_with_commas_and_doubled_quotes() {
        // Mirrors admin_csv's writer: `hello, "world"` round-trips.
        let csv = "id,note\r\n1,\"hello, \"\"world\"\"\"\r\n";
        let rows = parse_csv(csv);
        assert_eq!(rows[1], vec!["1", "hello, \"world\""]);
    }

    #[test]
    fn header_only_body_yields_one_row() {
        let rows = parse_csv("id,name\r\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], vec!["id", "name"]);
    }

    #[test]
    fn empty_body_yields_no_rows() {
        assert!(parse_csv("").is_empty());
    }

    #[test]
    fn tolerates_a_missing_final_newline() {
        let rows = parse_csv("id,name\r\n1,Aries");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1], vec!["1", "Aries"]);
    }

    #[test]
    fn preserves_empty_trailing_field() {
        // A row ending in a comma has a trailing empty field (e.g. a
        // project with no entity name).
        let rows = parse_csv("a,b,c\r\nx,y,\r\n");
        assert_eq!(rows[1], vec!["x", "y", ""]);
    }
}
