//! Email threading — the "headless Front" loop on top of inbound parse.
//!
//! When SendGrid Inbound Parse hands `web::inbound_email` a message, this
//! module turns it into a threaded support exchange:
//!
//! - **First contact** (a message to any address *without* a token, e.g.
//!   `test@parse.neonlaw.com` or, once the Workspace rule is in,
//!   `support@neonlaw.com`) opens a new `email_conversations` row and
//!   emails the staff cockpit (`NAVIGATOR_STAFF_NOTIFY_EMAIL`) a
//!   notification whose `Reply-To` is the conversation's token address
//!   `c<token>@<parse_host>`.
//! - **A reply to a token address** looks the conversation up by token.
//!   If the sender is staff (a `persons` row with a staff/admin role),
//!   the reply is relayed out to the external party as `support@…`; if
//!   the sender is the external party, staff are re-notified.
//!
//! The token in `Reply-To` is the whole threading mechanism: staff and
//! client both reply to the same shared address, and we disambiguate by
//! authenticated sender, never by address. No internal address ever
//! appears in an outbound external header.
//!
//! Threading is **opt-in per deployment**: it runs only when both
//! `NAVIGATOR_PARSE_HOST` and `NAVIGATOR_STAFF_NOTIFY_EMAIL` are set
//! (see [`ThreadConfig::from_env`]). Otherwise the webhook just archives
//! the raw `.eml` as before, so the repo ships no Neon-specific defaults.

use std::fmt::Write as _;
use std::sync::Arc;

use cloud::StorageService;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use workflows::{MachineKind, StateMachineRuntime};

use store::email_conversations as conv;
use store::entity::email_conversation::{
    STATUS_AWAITING_CLIENT, STATUS_AWAITING_STAFF, STATUS_CLOSED,
};
use store::entity::email_conversation_message::{
    DIRECTION_FROM_EXTERNAL, DIRECTION_FROM_STAFF, DIRECTION_SYSTEM, DIRECTION_TO_EXTERNAL,
    DIRECTION_TO_STAFF,
};
use store::entity::{notation, person};
use store::{documents, Db};

use crate::email::{EmailService, OutboundEmail, SendReceipt, DEFAULT_FROM_EMAIL};
use crate::inbound_email::{InboundAttachment, InboundEmail};

/// Runtime config for the threading layer, read from the environment.
/// Both fields are required; when either is unset the inbound webhook
/// skips threading and only archives the raw message (legacy behavior).
#[derive(Debug, Clone)]
pub struct ThreadConfig {
    /// Subdomain whose MX points at SendGrid Inbound Parse — the host of
    /// every `Reply-To` token address (`c<token>@<parse_host>`).
    pub parse_host: String,
    /// Where staff notifications are sent (e.g. `nick+aida@neonlaw.com`).
    /// A reply from that mailbox's owner relays back to the external
    /// party.
    pub staff_notify_email: String,
    /// Firm domain a staff reply's DKIM verdict must pass for before the
    /// reply is trusted to relay or fire a workflow command (e.g.
    /// `neonlaw.com`). `None` leaves the channel gated on staff-sender +
    /// unguessable token only — the opt-in posture that lets the live
    /// pipeline first confirm SendGrid's `dkim` field arrives before
    /// enforcement is flipped on. Sourced from `NAVIGATOR_DKIM_REQUIRE_DOMAIN`.
    pub verify_dkim_domain: Option<String>,
}

impl ThreadConfig {
    /// Read `NAVIGATOR_PARSE_HOST` + `NAVIGATOR_STAFF_NOTIFY_EMAIL`.
    /// Returns `None` when either is missing or empty — threading stays
    /// off and the webhook only archives the raw message.
    /// `NAVIGATOR_DKIM_REQUIRE_DOMAIN` is optional: when set it enables the
    /// command-channel DKIM gate; when unset the channel stays on the
    /// staff-sender + token gate alone.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        Some(Self {
            parse_host: non_empty(std::env::var("NAVIGATOR_PARSE_HOST").ok())?,
            staff_notify_email: non_empty(std::env::var("NAVIGATOR_STAFF_NOTIFY_EMAIL").ok())?,
            verify_dkim_domain: non_empty(std::env::var("NAVIGATOR_DKIM_REQUIRE_DOMAIN").ok()),
        })
    }
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Errors from threading. The webhook treats these as best-effort: the
/// raw `.eml` is already archived, so a failure is logged rather than
/// surfaced to SendGrid (a non-2xx would trigger a retry and duplicate
/// the conversation).
#[derive(Debug, Error)]
pub enum ThreadError {
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("send error: {0}")]
    Send(#[from] crate::email::EmailError),
    #[error("workflow runtime error: {0}")]
    Runtime(String),
    #[error("document ingest error: {0}")]
    Storage(String),
}

impl From<documents::IngestError> for ThreadError {
    fn from(e: documents::IngestError) -> Self {
        match e {
            documents::IngestError::Db(d) => Self::Db(d),
            documents::IngestError::Storage(s) => Self::Storage(s.to_string()),
        }
    }
}

/// A directive parsed from a staff reply. Commands are line-oriented: a
/// line whose first token is `@<verb>` is a command, is stripped from the
/// relayed prose, and is recorded on the message's `command_payload`.
///
/// The command channel is privileged — a command only runs when the
/// inbound reply both threads to a known conversation (the unguessable
/// token) and comes from a staff/admin `persons` row. Cryptographic
/// sender authentication (DKIM `d=neonlaw.com` + the SendGrid webhook
/// signature) is the next hardening layer; see `inbound_email.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    /// Fire a workflow signal on the conversation's linked notation.
    /// `@approve` → `approved`, `@deny [reason]` → `rejected`,
    /// `@signal <condition> [value]` → arbitrary condition.
    Signal {
        condition: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<String>,
    },
    /// Close the conversation; suppress the relay.
    Close,
    /// Internal note to self; suppress the relay.
    Internal,
    /// `@cleared` — the firm-wide conflict check is clear for this
    /// prospective client; release the relay gate. Recorded in the
    /// transcript so subsequent relays flow without re-prompting.
    ConflictCleared,
    /// `@link <notation_id>` — bind this conversation to a running workflow
    /// notation so the `@approve`/`@deny`/`@signal` command channel fires on
    /// it and inbound attachments file onto its matter. The id is captured
    /// verbatim and parsed/validated at execution time. This is what makes
    /// the staff-review gate and attachment-filing actually fire on a live
    /// matter — nothing else sets `email_conversations.notation_id` in prod.
    Link { notation_id: String },
}

/// The result of parsing a staff reply: the prose to relay (with command
/// lines removed) and the directives found, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedReply {
    pub relay_body: String,
    pub commands: Vec<Command>,
}

/// The instruction the firm-wide conflict gate gives staff — in the
/// first-contact prompt and the held-relay bounce alike. One source so the
/// policy phrasing (and the `@cleared` verb) never drifts between the two.
const CONFLICT_CHECK_INSTRUCTION: &str = "Run the firm-wide conflict check across all attorneys; \
                                          reply with @cleared to release the relay.";

/// The dependencies every email-loop handler needs, bundled so each one
/// takes a single `&ThreadCtx` instead of re-threading the same
/// `(db, storage, email, runtime, cfg)` convoy by hand. Each handler
/// rebinds only the fields it uses.
struct ThreadCtx<'a> {
    db: &'a Db,
    storage: &'a Arc<dyn StorageService>,
    email: &'a dyn EmailService,
    runtime: &'a dyn StateMachineRuntime,
    cfg: &'a ThreadConfig,
}

/// Thread one freshly-received inbound message. `raw_key` is the
/// object-storage key of the archived `.eml`.
///
/// # Errors
///
/// Propagates database and send errors for the caller to log.
pub async fn thread_inbound(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    email: &dyn EmailService,
    runtime: &dyn StateMachineRuntime,
    cfg: &ThreadConfig,
    inbound: &InboundEmail,
    raw_key: &str,
) -> Result<(), ThreadError> {
    let ctx = ThreadCtx {
        db,
        storage,
        email,
        runtime,
        cfg,
    };
    let body = extract_body(inbound);
    match token_from_to(&inbound.to, &cfg.parse_host) {
        None => open_first_contact(&ctx, inbound, raw_key, &body).await,
        Some(token) => continue_thread(&ctx, inbound, raw_key, &body, &token).await,
    }
}

async fn open_first_contact(
    ctx: &ThreadCtx<'_>,
    inbound: &InboundEmail,
    raw_key: &str,
    body: &str,
) -> Result<(), ThreadError> {
    let db = ctx.db;
    let external_email = extract_addr(&inbound.from);
    let external_name = extract_name(&inbound.from);
    let token = mint_token();
    let person_id = person_lookup(db, &external_email).await?.map(|p| p.id);

    // Auto-route: a known client who has exactly one open matter threads
    // straight onto it — no manual `@link` needed for the common case. An
    // unknown sender or an ambiguous (multi-matter) client stays unlinked and
    // is triaged by staff. This never weakens the conflict gate: a sender we
    // can auto-link already has a `persons` row, so they were never a
    // prospect.
    let notation_id = match person_id {
        Some(pid) => store::projects::sole_open_matter_for_person(db, pid).await?,
        None => None,
    };

    let conversation_id = conv::open(
        db,
        &conv::NewConversation {
            token: &token,
            external_email: &external_email,
            external_name: external_name.as_deref(),
            subject: &inbound.subject,
            person_id,
            notation_id,
        },
    )
    .await?;

    conv::append(
        db,
        &conv::NewMessage {
            conversation_id,
            direction: DIRECTION_FROM_EXTERNAL,
            from_addr: &external_email,
            to_addr: &inbound.to,
            subject: &inbound.subject,
            body_text: body,
            raw_storage_key: Some(raw_key),
            provider_message_id: inbound.message_id.as_deref(),
            ..Default::default()
        },
    )
    .await?;

    let mut notes: Vec<String> = Vec::new();
    // Firm-wide imputed-conflicts gate (RPC 1.10): a first contact from a
    // sender with no matched `persons` row is a prospective client. Prompt
    // staff to run the conflict check before the first substantive relay —
    // the relay itself is held until `@cleared` (see `handle_staff_reply`).
    if person_id.is_none() {
        notes.push(format!(
            "⚠ Prospective client — {} is not yet in the system. {CONFLICT_CHECK_INSTRUCTION}",
            external_name.as_deref().unwrap_or(&external_email)
        ));
    }
    // A brand-new conversation has no linked matter, so attachments can't
    // be filed as documents yet — they live in the archived `.eml`. Note
    // the count so staff know to link the thread to a matter to ingest them.
    if !inbound.attachments.is_empty() {
        notes.push(format!(
            "{} attachment(s) received and archived in the raw message; link this thread to a \
             matter to file them as documents.",
            inbound.attachments.len()
        ));
    }
    let extra_note = (!notes.is_empty()).then(|| notes.join("\n\n"));

    notify_staff(
        ctx,
        conversation_id,
        &token,
        external_name.as_deref(),
        &external_email,
        &inbound.subject,
        body,
        extra_note.as_deref(),
    )
    .await?;
    // Mirror the exchange into the matter's conversation log (no-op until the
    // thread is matter-linked; idempotent if it already is).
    sync_conversation_to_spine(ctx, conversation_id).await
}

/// Mirror a conversation's client-facing email hops into the matter's
/// `communications` spine, so the privileged conversation log shows the email
/// exchange interleaved with document comments. Idempotent — keyed on
/// `(channel, the conversation-message id)` — so it is safe to call after
/// every hop and as a back-fill the instant a thread is linked to a matter.
///
/// A no-op until the conversation is matter-linked (the spine is
/// project-scoped). Only the client conversation is mirrored: `from_external`
/// (the client writing in) and `to_external` (the firm's relayed reply).
/// Staff notifications and `system` hops are firm plumbing, not the
/// conversation with the client, so they are skipped.
async fn sync_conversation_to_spine(
    ctx: &ThreadCtx<'_>,
    conversation_id: uuid::Uuid,
) -> Result<(), ThreadError> {
    let db = ctx.db;
    let Some(conversation) = conv::by_id(db, conversation_id).await? else {
        return Ok(());
    };
    let Some(notation_id) = conversation.notation_id else {
        return Ok(());
    };
    let Some(notation) = notation::Entity::find_by_id(notation_id).one(db).await? else {
        return Ok(());
    };
    let project_id = notation.project_id;

    for m in conv::messages(db, conversation_id).await? {
        let (channel, direction, author) = match m.direction.as_str() {
            DIRECTION_FROM_EXTERNAL => (
                store::communications::channel::EMAIL_INBOUND,
                store::communications::direction::INBOUND,
                conversation.person_id,
            ),
            DIRECTION_TO_EXTERNAL => (
                store::communications::channel::EMAIL_OUTBOUND,
                store::communications::direction::OUTBOUND,
                None,
            ),
            _ => continue,
        };
        // The conversation-message id is the stable idempotency key, so a
        // re-sync of an already-mirrored hop returns the existing spine row.
        let source_ref = m.id.to_string();
        store::communications::ingest(
            db,
            &store::communications::IngestArgs {
                project_id,
                channel,
                direction,
                author_person_id: author,
                counterparty: Some(conversation.external_email.as_str()),
                subject: Some(m.subject.as_str()),
                body: &m.body_text,
                source_ref: Some(&source_ref),
                blob_id: None,
                occurred_at: &m.inserted_at,
            },
        )
        .await?;
    }
    Ok(())
}

/// Send one outbound hop from `support@` on a conversation and journal it
/// to the transcript with the provider's returned message-id. Always
/// carries the per-conversation `Reply-To` token and the thread's
/// message-id chain, so a support exchange threads in the recipient's
/// client and no internal address ever leaks — the three outbound paths
/// (staff notification, client relay, conflict-hold prompt) can't forget
/// either, because the helper owns them.
async fn send_and_journal(
    ctx: &ThreadCtx<'_>,
    conversation_id: uuid::Uuid,
    token: &str,
    direction: &str,
    to_addr: &str,
    subject: &str,
    body: &str,
) -> Result<SendReceipt, ThreadError> {
    let reply_to = reply_address(token, &ctx.cfg.parse_host);
    let thread_refs = thread_message_ids(ctx.db, conversation_id).await?;
    let html = workflows::email::render_email_html(
        body,
        &workflows::email::base_url_from_env(),
        workflows::email::EmailBrand::Firm,
    );
    let receipt = ctx
        .email
        .send(
            OutboundEmail::new(to_addr, subject, body)
                .with_html(html)
                .with_reply_to(reply_to.as_str())
                .with_thread_refs(&thread_refs),
        )
        .await?;
    conv::append(
        ctx.db,
        &conv::NewMessage {
            conversation_id,
            direction,
            from_addr: DEFAULT_FROM_EMAIL,
            to_addr,
            subject,
            body_text: body,
            provider_message_id: receipt.message_id.as_deref(),
            ..Default::default()
        },
    )
    .await?;
    Ok(receipt)
}

#[allow(clippy::too_many_arguments)]
async fn notify_staff(
    ctx: &ThreadCtx<'_>,
    conversation_id: uuid::Uuid,
    token: &str,
    external_name: Option<&str>,
    external_email: &str,
    subject: &str,
    body: &str,
    extra_note: Option<&str>,
) -> Result<(), ThreadError> {
    let display = external_name.unwrap_or(external_email);
    let out_subject = format!("[{display}] {subject}");
    let out_body = staff_notification_body(display, external_email, subject, body, extra_note);

    send_and_journal(
        ctx,
        conversation_id,
        token,
        DIRECTION_TO_STAFF,
        ctx.cfg.staff_notify_email.as_str(),
        &out_subject,
        &out_body,
    )
    .await?;
    conv::set_status(ctx.db, conversation_id, STATUS_AWAITING_STAFF).await?;
    Ok(())
}

async fn continue_thread(
    ctx: &ThreadCtx<'_>,
    inbound: &InboundEmail,
    raw_key: &str,
    body: &str,
    token: &str,
) -> Result<(), ThreadError> {
    let db = ctx.db;
    let Some(conversation) = conv::by_token(db, token).await? else {
        tracing::warn!(
            token,
            "inbound reply for an unknown conversation token; ignoring"
        );
        return Ok(());
    };
    let sender = extract_addr(&inbound.from);
    let sender_is_staff = person_lookup(db, &sender)
        .await?
        .is_some_and(|p| p.role.is_staff_tier());

    if sender_is_staff {
        handle_staff_reply(ctx, inbound, raw_key, body, token, &conversation, &sender).await?;
    } else {
        // Client follow-up on an open thread — re-notify staff.
        conv::append(
            db,
            &conv::NewMessage {
                conversation_id: conversation.id,
                direction: DIRECTION_FROM_EXTERNAL,
                from_addr: &sender,
                to_addr: &inbound.to,
                subject: &inbound.subject,
                body_text: body,
                raw_storage_key: Some(raw_key),
                provider_message_id: inbound.message_id.as_deref(),
                ..Default::default()
            },
        )
        .await?;
        // File any attachments as documents on the linked matter and fold a
        // review request into the staff notification.
        let attachments_note =
            process_attachments(ctx, &conversation, &inbound.attachments).await?;
        notify_staff(
            ctx,
            conversation.id,
            token,
            conversation.external_name.as_deref(),
            &conversation.external_email,
            &conversation.subject,
            body,
            attachments_note.as_deref(),
        )
        .await?;
    }
    // Mirror the (possibly newly-linked, via `@link`) exchange into the
    // matter's conversation log. Idempotent and a no-op until linked.
    sync_conversation_to_spine(ctx, conversation.id).await
}

/// File a client's inbound attachments into the canonical `documents`
/// lane and record the ingest as a `system` hop in the transcript, so a
/// PDF a client emails to `support@` becomes a reviewable matter document.
///
/// Ingestion needs an owning project, which we resolve through the
/// conversation's linked `notation` — so it runs only when the thread is
/// tied to a matter (mirroring how [`fire_signal`] no-ops without one).
/// On an unlinked thread the bytes stay in the archived `.eml` and we
/// return a note telling staff to link the thread first. Returns the
/// staff-facing review note, or `None` when there were no attachments.
async fn process_attachments(
    ctx: &ThreadCtx<'_>,
    conversation: &store::entity::email_conversation::Model,
    attachments: &[InboundAttachment],
) -> Result<Option<String>, ThreadError> {
    let (db, storage) = (ctx.db, ctx.storage);
    if attachments.is_empty() {
        return Ok(None);
    }
    let Some(notation_id) = conversation.notation_id else {
        tracing::info!(
            conversation_id = %conversation.id,
            count = attachments.len(),
            "inbound attachments on a thread with no linked matter; archived in raw .eml only"
        );
        return Ok(Some(format!(
            "{} attachment(s) received and archived in the raw message; link this thread to a \
             matter to file them as documents.",
            attachments.len()
        )));
    };
    let Some(notation) = notation::Entity::find_by_id(notation_id).one(db).await? else {
        tracing::warn!(%notation_id, "conversation references a missing notation; attachments not filed");
        return Ok(None);
    };

    // File each attachment as the external sender, so the matter repo's
    // `git log` attributes it to whoever emailed it in.
    let author = repos::Author {
        name: conversation
            .external_name
            .as_deref()
            .unwrap_or(conversation.external_email.as_str()),
        email: conversation.external_email.as_str(),
    };

    let mut lines = Vec::new();
    for att in attachments {
        let filename = non_empty_or(att.filename.as_str(), "attachment");
        let content_type = non_empty_or(att.content_type.as_str(), "application/octet-stream");
        let ingested = crate::matter_documents::record_document(
            db,
            storage,
            author,
            &documents::IngestArgs {
                project_id: notation.project_id,
                source: documents::source::EMAIL,
                filename,
                kind: "unclassified",
                content_type,
                description: Some("received via support@ email"),
                source_revision_id: None,
            },
            &att.bytes,
        )
        .await?;
        lines.push(format!(
            "• {filename} ({} bytes) → document {}",
            ingested.byte_size, ingested.document_id
        ));
    }

    let summary = format!(
        "{} document(s) received for review and filed to the matter:\n{}",
        attachments.len(),
        lines.join("\n")
    );

    conv::append(
        db,
        &conv::NewMessage {
            conversation_id: conversation.id,
            direction: DIRECTION_SYSTEM,
            from_addr: &conversation.external_email,
            to_addr: DEFAULT_FROM_EMAIL,
            subject: &conversation.subject,
            body_text: &summary,
            ..Default::default()
        },
    )
    .await?;

    Ok(Some(summary))
}

/// `s` trimmed, or `fallback` when `s` is blank.
fn non_empty_or<'a>(s: &'a str, fallback: &'a str) -> &'a str {
    if s.trim().is_empty() {
        fallback
    } else {
        s
    }
}

/// Handle a reply from a staff/admin sender on a known conversation: the
/// privileged path that may relay to the client and fire workflow
/// commands. Gated by [`dkim_passes_for_domain`] when the config enables
/// it; otherwise trusted on staff-sender + unguessable token alone.
async fn handle_staff_reply(
    ctx: &ThreadCtx<'_>,
    inbound: &InboundEmail,
    raw_key: &str,
    body: &str,
    token: &str,
    conversation: &store::entity::email_conversation::Model,
    sender: &str,
) -> Result<(), ThreadError> {
    let (db, cfg) = (ctx.db, ctx.cfg);
    // Command-channel authentication (Scorpio's non-negotiable): a staff
    // reply may relay or fire a workflow signal only when its DKIM verdict
    // passes for the firm domain. Without this a forged `From:
    // nick@neonlaw.com` to a leaked token could approve a retainer or relay
    // arbitrary content to the client as support@. Enforced only when
    // configured; the raw `.eml` is archived regardless, and the failed
    // attempt is journaled for the transcript.
    if let Some(domain) = cfg.verify_dkim_domain.as_deref() {
        if !dkim_passes_for_domain(&inbound.dkim, domain) {
            tracing::warn!(
                token,
                sender,
                dkim = %inbound.dkim,
                "staff reply failed DKIM for {domain}; not relaying or executing commands"
            );
            conv::append(
                db,
                &conv::NewMessage {
                    conversation_id: conversation.id,
                    direction: DIRECTION_FROM_STAFF,
                    from_addr: sender,
                    to_addr: &inbound.to,
                    subject: &inbound.subject,
                    body_text: body,
                    raw_storage_key: Some(raw_key),
                    provider_message_id: inbound.message_id.as_deref(),
                    ..Default::default()
                },
            )
            .await?;
            return Ok(());
        }
    }

    let cleaned = strip_quoted(body);
    let parsed = parse_reply(&cleaned);
    let command_payload = (!parsed.commands.is_empty())
        .then(|| serde_json::to_string(&parsed.commands).unwrap_or_default());

    // The full cleaned reply (commands included) is journaled; the relay
    // carries only the prose with command lines stripped.
    conv::append(
        db,
        &conv::NewMessage {
            conversation_id: conversation.id,
            direction: DIRECTION_FROM_STAFF,
            from_addr: sender,
            to_addr: &inbound.to,
            subject: &inbound.subject,
            body_text: &cleaned,
            raw_storage_key: Some(raw_key),
            provider_message_id: inbound.message_id.as_deref(),
            command_payload: command_payload.as_deref(),
            ..Default::default()
        },
    )
    .await?;

    // Execute directives. `@close`/`@internal` suppress the relay;
    // `@signal`/`@approve`/`@deny` fire a workflow signal on the linked
    // notation (the production staff-review gate) and still relay any
    // accompanying prose. A successful `@link` updates this local model so a
    // same-message `@approve` and the relay gate below both see the new link.
    let mut conversation = conversation.clone();
    let mut suppress_relay = false;
    for command in &parsed.commands {
        match command {
            Command::Close => {
                conv::set_status(db, conversation.id, STATUS_CLOSED).await?;
                suppress_relay = true;
            }
            Command::Internal => suppress_relay = true,
            // Clearance is journaled via this message's `command_payload`;
            // the relay gate below reads it back. Nothing else to do here.
            Command::ConflictCleared => {}
            Command::Link { notation_id } => {
                if let Some(linked) = link_notation(ctx, &conversation, notation_id, token).await? {
                    conversation.notation_id = Some(linked);
                }
            }
            Command::Signal { condition, value } => {
                fire_signal(ctx, &conversation, condition, value.as_deref()).await?;
            }
        }
    }

    if !suppress_relay && !parsed.relay_body.trim().is_empty() {
        // Firm-wide imputed-conflicts gate (RPC 1.10): the first substantive
        // relay to a prospective client is held until staff have run the
        // conflict check and released it with `@cleared`. A prospective
        // client is an external party with neither a matched `persons` row
        // nor a linked matter (`notation_id`) — anyone past intake is already
        // through this gate. The check is a gate in the loop, never a
        // courtesy after the fact.
        let is_prospect = conversation.person_id.is_none() && conversation.notation_id.is_none();
        if is_prospect && !is_conflict_cleared(db, conversation.id).await? {
            hold_relay_for_conflict_check(ctx, &conversation, token).await?;
        } else {
            relay_to_external(ctx, &conversation, token, &parsed.relay_body).await?;
        }
    }
    Ok(())
}

/// True once a `@cleared` directive has been recorded on the conversation
/// (the current reply's directives are already journaled by the time the
/// relay gate consults this).
async fn is_conflict_cleared(db: &Db, conversation_id: uuid::Uuid) -> Result<bool, sea_orm::DbErr> {
    for m in conv::messages(db, conversation_id).await? {
        let Some(payload) = m.command_payload.as_deref() else {
            continue;
        };
        if let Ok(commands) = serde_json::from_str::<Vec<Command>>(payload) {
            if commands.contains(&Command::ConflictCleared) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Hold a staff relay that targets a not-yet-cleared prospective client:
/// journal a `system` hop and bounce a prompt back to the cockpit asking
/// for the firm-wide conflict check before the message reaches the client.
/// Nothing is relayed to the external party.
async fn hold_relay_for_conflict_check(
    ctx: &ThreadCtx<'_>,
    conversation: &store::entity::email_conversation::Model,
    token: &str,
) -> Result<(), ThreadError> {
    tracing::warn!(
        conversation_id = %conversation.id,
        external = %conversation.external_email,
        "relay held: firm-wide conflict check not cleared for this prospective client"
    );
    let note = format!(
        "Your reply was NOT relayed. {} is a prospective client not yet in the system. \
         {CONFLICT_CHECK_INSTRUCTION}",
        conversation
            .external_name
            .as_deref()
            .unwrap_or(&conversation.external_email)
    );
    // Journaled as a `system` hop (not `to_staff`): this records the loop's
    // decision to hold, and `system` hops are excluded from the message-id
    // chain so the prompt never pollutes the client-facing References.
    let subject = format!("[conflict check] {}", conversation.subject);
    send_and_journal(
        ctx,
        conversation.id,
        token,
        DIRECTION_SYSTEM,
        ctx.cfg.staff_notify_email.as_str(),
        &subject,
        &note,
    )
    .await?;
    conv::set_status(ctx.db, conversation.id, STATUS_AWAITING_STAFF).await?;
    Ok(())
}

async fn relay_to_external(
    ctx: &ThreadCtx<'_>,
    conversation: &store::entity::email_conversation::Model,
    token: &str,
    cleaned: &str,
) -> Result<(), ThreadError> {
    let subject = re_subject(&conversation.subject);
    send_and_journal(
        ctx,
        conversation.id,
        token,
        DIRECTION_TO_EXTERNAL,
        conversation.external_email.as_str(),
        &subject,
        cleaned,
    )
    .await?;
    conv::set_status(ctx.db, conversation.id, STATUS_AWAITING_CLIENT).await?;
    Ok(())
}

/// The RFC 5322 message-ids of the inbound hops on a conversation, oldest
/// first — the chain put in outbound `References`/`In-Reply-To` so the
/// attorney's mail client threads the whole support exchange. Only inbound
/// hops (`from_external`/`from_staff`) carry a real RFC message-id;
/// outbound hops carry SendGrid's `X-Message-Id` (a different namespace),
/// so they're excluded.
async fn thread_message_ids(
    db: &Db,
    conversation_id: uuid::Uuid,
) -> Result<Vec<String>, sea_orm::DbErr> {
    Ok(conv::messages(db, conversation_id)
        .await?
        .into_iter()
        .filter(|m| m.direction == DIRECTION_FROM_EXTERNAL || m.direction == DIRECTION_FROM_STAFF)
        .filter_map(|m| m.provider_message_id)
        .collect())
}

async fn person_lookup(db: &Db, email: &str) -> Result<Option<person::Model>, sea_orm::DbErr> {
    person::Entity::find()
        .filter(person::Column::Email.eq(email))
        .one(db)
        .await
}

/// Execute `@link <notation_id>`: validate the id, confirm the matter
/// exists, then bind the conversation to it so the command channel and
/// attachment-filing fire on a live matter. Either outcome is reported back
/// to the cockpit as a `system` hop (never relayed to the client); a bad or
/// unknown id leaves the conversation unlinked. Returns the linked notation
/// id on success.
async fn link_notation(
    ctx: &ThreadCtx<'_>,
    conversation: &store::entity::email_conversation::Model,
    raw_id: &str,
    token: &str,
) -> Result<Option<uuid::Uuid>, ThreadError> {
    let outcome = match uuid::Uuid::parse_str(raw_id.trim()) {
        Err(_) => {
            tracing::warn!(conversation_id = %conversation.id, raw_id, "could not @link: invalid notation id");
            Err(format!(
                "Could not link: \"{raw_id}\" is not a valid matter id. \
                 Reply @link <notation_id> with the matter's id."
            ))
        }
        Ok(notation_id)
            if notation::Entity::find_by_id(notation_id)
                .one(ctx.db)
                .await?
                .is_none() =>
        {
            tracing::warn!(conversation_id = %conversation.id, %notation_id, "could not @link: no such matter");
            Err(format!(
                "Could not link: no matter found for {notation_id}."
            ))
        }
        Ok(notation_id) => {
            conv::set_notation(ctx.db, conversation.id, notation_id).await?;
            tracing::info!(conversation_id = %conversation.id, %notation_id, "conversation linked to matter via @link");
            Ok(notation_id)
        }
    };

    // Report either outcome back to the cockpit as a `system` hop — never
    // relayed to the client.
    let note = match &outcome {
        Ok(notation_id) => format!(
            "Linked this conversation to matter {notation_id}. \
             Staff commands (@approve / @deny / @signal) and inbound attachments \
             now act on this matter."
        ),
        Err(msg) => msg.clone(),
    };
    send_and_journal(
        ctx,
        conversation.id,
        token,
        DIRECTION_SYSTEM,
        ctx.cfg.staff_notify_email.as_str(),
        &format!("[link] {}", conversation.subject),
        &note,
    )
    .await?;
    Ok(outcome.ok())
}

/// Fire a workflow signal on the conversation's linked notation, then
/// mirror the resulting state into the `notations` row (the same pattern
/// the e-signature webhook uses) so the admin UI reflects the advance. A
/// command on a conversation with no linked notation is a logged no-op.
async fn fire_signal(
    ctx: &ThreadCtx<'_>,
    conversation: &store::entity::email_conversation::Model,
    condition: &str,
    value: Option<&str>,
) -> Result<(), ThreadError> {
    let (db, runtime) = (ctx.db, ctx.runtime);
    let Some(notation_id) = conversation.notation_id else {
        tracing::warn!(
            conversation_id = %conversation.id,
            condition,
            "staff command signal but conversation has no linked notation; ignoring"
        );
        return Ok(());
    };
    let next = runtime
        .signal(MachineKind::Workflow, notation_id, condition, value)
        .await
        .map_err(|e| ThreadError::Runtime(e.to_string()))?;
    if let Some(row) = notation::Entity::find_by_id(notation_id).one(db).await? {
        let mut active: notation::ActiveModel = row.into();
        active.state = ActiveValue::Set(next.as_str().to_string());
        active.update(db).await?;
    }
    tracing::info!(%notation_id, condition, next_state = %next.as_str(), "staff command advanced workflow");
    Ok(())
}

/// Parse a staff reply into the prose to relay plus any directives. A
/// line whose trimmed form is `@<verb> …` for a recognized verb is a
/// command and is removed from the relay; unrecognized `@…` lines pass
/// through untouched (e.g. an `@mention` to the client).
fn parse_reply(body: &str) -> ParsedReply {
    let mut kept = Vec::new();
    let mut commands = Vec::new();
    for line in body.lines() {
        if let Some(command) = parse_command_line(line) {
            commands.push(command);
        } else {
            kept.push(line);
        }
    }
    ParsedReply {
        relay_body: kept.join("\n").trim().to_string(),
        commands,
    }
}

fn parse_command_line(line: &str) -> Option<Command> {
    let rest = line.trim().strip_prefix('@')?;
    let mut parts = rest.split_whitespace();
    let verb = parts.next()?.to_lowercase();
    match verb.as_str() {
        "approve" => Some(Command::Signal {
            condition: "approved".to_string(),
            value: None,
        }),
        "deny" | "reject" => {
            let reason = parts.collect::<Vec<_>>().join(" ");
            Some(Command::Signal {
                condition: "rejected".to_string(),
                value: (!reason.is_empty()).then_some(reason),
            })
        }
        "signal" => {
            let condition = parts.next()?.to_string();
            let value = parts.collect::<Vec<_>>().join(" ");
            Some(Command::Signal {
                condition,
                value: (!value.is_empty()).then_some(value),
            })
        }
        "close" => Some(Command::Close),
        "internal" => Some(Command::Internal),
        "cleared" | "clear" => Some(Command::ConflictCleared),
        // Always a command (even bare) so the `@link` line is stripped from
        // the relay and never leaks to the client; a missing/invalid id is
        // reported back to the cockpit at execution time.
        "link" => Some(Command::Link {
            notation_id: parts.next().unwrap_or_default().to_string(),
        }),
        // Unknown `@verb` — not a command; leave the line in the relay.
        _ => None,
    }
}

/// True when SendGrid's DKIM verdict reports `pass` for `domain`.
///
/// The verdict field is a Ruby-hash-shaped string — `{@neonlaw.com : pass}`,
/// or `{@a.com : pass, @b.com : fail}` when a message carries multiple
/// signatures. We require an explicit `pass` for the target firm domain;
/// any other domain's result, a `fail`/`none`, or an empty/absent field
/// is untrusted.
fn dkim_passes_for_domain(dkim_field: &str, domain: &str) -> bool {
    let target = domain.trim().trim_start_matches('@').to_lowercase();
    if target.is_empty() {
        return false;
    }
    let inner = dkim_field
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}');
    inner.split(',').any(|entry| {
        let mut parts = entry.splitn(2, ':');
        let d = parts
            .next()
            .unwrap_or_default()
            .trim()
            .trim_start_matches('@')
            .to_lowercase();
        let result = parts.next().unwrap_or_default().trim().to_lowercase();
        d == target && result == "pass"
    })
}

/// The plain-text body of an inbound message. In raw mode SendGrid sends
/// no parsed `text` part, so fall back to MIME-parsing the raw bytes.
fn extract_body(inbound: &InboundEmail) -> String {
    if !inbound.text.trim().is_empty() {
        return inbound.text.clone();
    }
    mail_parser::MessageParser::default()
        .parse(&inbound.raw)
        .and_then(|m| m.body_text(0).map(std::borrow::Cow::into_owned))
        .unwrap_or_default()
}

/// An unguessable 32-hex-char thread token (16 random bytes).
fn mint_token() -> String {
    let bytes: [u8; 16] = rand::random();
    let mut out = String::with_capacity(32);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// The `Reply-To` address that threads a reply back to a conversation.
fn reply_address(token: &str, parse_host: &str) -> String {
    format!("c{token}@{parse_host}")
}

/// Pull the bare, lowercased email out of a header value that may be
/// `Name <addr@host>` or just `addr@host`.
fn extract_addr(raw: &str) -> String {
    let s = raw.trim();
    if let (Some(lt), Some(gt)) = (s.find('<'), s.find('>')) {
        if lt < gt {
            return s[lt + 1..gt].trim().to_lowercase();
        }
    }
    s.to_lowercase()
}

/// The display name from a `Name <addr>` header value, if present.
fn extract_name(raw: &str) -> Option<String> {
    let s = raw.trim();
    let lt = s.find('<')?;
    let name = s[..lt].trim().trim_matches('"').trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// If any address in `to` is a token address on `parse_host`
/// (`c<32-hex>@parse_host`), return the token. Scans all addresses so a
/// reply carrying extra recipients still threads.
fn token_from_to(to: &str, parse_host: &str) -> Option<String> {
    let suffix = format!("@{}", parse_host.to_lowercase());
    to.split([' ', '\t', '\r', '\n', ',', ';', '<', '>'])
        .map(|t| t.trim().trim_matches('"').to_lowercase())
        .find_map(|addr| {
            let local = addr.strip_suffix(&suffix)?;
            let token = local.strip_prefix('c')?;
            (token.len() == 32 && token.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| token.to_string())
        })
}

/// Strip quoted history and signature from a staff reply so only the new
/// prose is relayed. Heuristic: cut at the first Gmail-style attribution
/// line, the first quoted (`>`) line, or the `-- ` signature delimiter.
fn strip_quoted(body: &str) -> String {
    let mut kept = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        if line == "-- "
            || line.starts_with('>')
            || (trimmed.starts_with("On ") && trimmed.ends_with("wrote:"))
        {
            break;
        }
        kept.push(line);
    }
    kept.join("\n").trim_end().to_string()
}

/// Ensure a subject carries a single `Re:` prefix for the relay.
fn re_subject(subject: &str) -> String {
    if subject.trim_start().to_lowercase().starts_with("re:") {
        subject.to_string()
    } else {
        format!("Re: {subject}")
    }
}

fn staff_notification_body(
    display: &str,
    external_email: &str,
    subject: &str,
    body: &str,
    extra_note: Option<&str>,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "New message via {DEFAULT_FROM_EMAIL}");
    let _ = writeln!(s);
    let _ = writeln!(s, "From:    {display} <{external_email}>");
    let _ = writeln!(s, "Subject: {subject}");
    let _ = writeln!(s);
    let _ = writeln!(s, "{}", body.trim_end());
    if let Some(note) = extra_note {
        let _ = writeln!(s);
        let _ = writeln!(s, "{}", note.trim_end());
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "--");
    let _ = writeln!(
        s,
        "Reply to this email to respond. Your reply is relayed from \
         {DEFAULT_FROM_EMAIL}; the client never sees your address."
    );
    s
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use std::sync::Arc;

    use cloud::StorageService;

    use super::{
        dkim_passes_for_domain, extract_addr, extract_name, mint_token, parse_reply, re_subject,
        strip_quoted, thread_inbound, token_from_to, Command, ThreadConfig,
    };
    use crate::email::CapturingEmail;
    use crate::inbound_email::{InboundAttachment, InboundEmail};

    /// A throwaway filesystem-backed `StorageService` for tests. The temp
    /// dir is intentionally leaked so it outlives the test even though the
    /// `TempDir` handle is dropped — the document-ingest path needs the
    /// directory to stay writable for the whole run.
    async fn storage() -> Arc<dyn StorageService> {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::mem::forget(tmp);
        Arc::new(cloud::FsStorage::new(root).await.unwrap())
    }
    use workflows::{
        MachineKind, StateMachineRuntime, StateName, WorkflowEvent, WorkflowRuntimeError,
        WorkflowSpec,
    };

    /// Minimal `StateMachineRuntime` that records the signals fired at it,
    /// so command tests can assert `@approve` reached the workflow.
    #[derive(Default)]
    struct RecordingRuntime {
        signals: Mutex<Vec<(uuid::Uuid, String, Option<String>)>>,
    }

    #[async_trait::async_trait]
    impl StateMachineRuntime for RecordingRuntime {
        async fn start(
            &self,
            _kind: MachineKind,
            _notation_id: uuid::Uuid,
            _spec: &WorkflowSpec,
        ) -> Result<(), WorkflowRuntimeError> {
            Ok(())
        }
        async fn signal(
            &self,
            _kind: MachineKind,
            notation_id: uuid::Uuid,
            condition: &str,
            payload: Option<&str>,
        ) -> Result<StateName, WorkflowRuntimeError> {
            self.signals.lock().unwrap().push((
                notation_id,
                condition.to_string(),
                payload.map(str::to_string),
            ));
            Ok(StateName::from(condition))
        }
        async fn current_state(
            &self,
            _kind: MachineKind,
            _notation_id: uuid::Uuid,
        ) -> Option<StateName> {
            None
        }
        async fn events(&self, _kind: MachineKind, _notation_id: uuid::Uuid) -> Vec<WorkflowEvent> {
            Vec::new()
        }
    }

    #[test]
    fn mint_token_is_32_hex_and_unique() {
        let a = mint_token();
        let b = mint_token();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn token_from_to_matches_only_token_addresses() {
        let host = "parse.neonlaw.com";
        let token = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            token_from_to(&format!("c{token}@{host}"), host).as_deref(),
            Some(token)
        );
        // first contact — no token
        assert_eq!(token_from_to("test@parse.neonlaw.com", host), None);
        // right shape, wrong host
        assert_eq!(token_from_to(&format!("c{token}@evil.com"), host), None);
        // display-name wrapped + extra recipient still threads
        assert_eq!(
            token_from_to(&format!("\"Support\" <c{token}@{host}>, cc@x.com"), host).as_deref(),
            Some(token)
        );
    }

    #[test]
    fn addr_and_name_extraction() {
        assert_eq!(
            extract_addr("AIDA Smoke <smoke@neonlaw.com>"),
            "smoke@neonlaw.com"
        );
        assert_eq!(extract_addr("plain@example.com"), "plain@example.com");
        assert_eq!(extract_addr("UPPER@Example.COM"), "upper@example.com");
        assert_eq!(
            extract_name("AIDA Smoke <smoke@neonlaw.com>").as_deref(),
            Some("AIDA Smoke")
        );
        assert_eq!(extract_name("plain@example.com"), None);
    }

    #[test]
    fn strip_quoted_cuts_history_and_signature() {
        let body = "Approved — here is your answer.\n\nOn Tue, Jun 3, 2026 at 9:00 AM Pisces <c@x.com> wrote:\n> original question";
        assert_eq!(strip_quoted(body), "Approved — here is your answer.");

        let with_sig = "Thanks, that works.\n-- \nNick\nNeon Law";
        assert_eq!(strip_quoted(with_sig), "Thanks, that works.");
    }

    #[test]
    fn dkim_verdict_parsing_requires_pass_for_the_target_domain() {
        assert!(dkim_passes_for_domain(
            "{@neonlaw.com : pass}",
            "neonlaw.com"
        ));
        // case-insensitive on the domain; tolerant of the @ in config
        assert!(dkim_passes_for_domain(
            "{@NeonLaw.com : pass}",
            "@neonlaw.com"
        ));
        // multi-signature: the firm domain passes even if another fails
        assert!(dkim_passes_for_domain(
            "{@sendgrid.me : fail, @neonlaw.com : pass}",
            "neonlaw.com"
        ));
        // a fail for the firm domain is not trusted
        assert!(!dkim_passes_for_domain(
            "{@neonlaw.com : fail}",
            "neonlaw.com"
        ));
        // a pass for a different domain does not authorize the firm domain
        assert!(!dkim_passes_for_domain("{@evil.com : pass}", "neonlaw.com"));
        // empty / absent verdict is untrusted
        assert!(!dkim_passes_for_domain("", "neonlaw.com"));
        assert!(!dkim_passes_for_domain("{}", "neonlaw.com"));
    }

    #[test]
    fn re_subject_prefixes_once() {
        assert_eq!(
            re_subject("Question about my LLC"),
            "Re: Question about my LLC"
        );
        assert_eq!(re_subject("Re: already replied"), "Re: already replied");
    }

    fn cfg() -> ThreadConfig {
        ThreadConfig {
            parse_host: "parse.neonlaw.com".into(),
            staff_notify_email: "nick+aida@neonlaw.com".into(),
            verify_dkim_domain: None,
        }
    }

    fn inbound(from: &str, to: &str, subject: &str, text: &str) -> InboundEmail {
        InboundEmail {
            from: from.into(),
            to: to.into(),
            subject: subject.into(),
            text: text.into(),
            raw: Vec::new(),
            dkim: String::new(),
            attachments: Vec::new(),
            message_id: None,
        }
    }

    async fn seed_staff(db: &store::Db, email: &str) {
        use sea_orm::ActiveModelTrait;
        use store::entity::person;
        person::ActiveModel {
            name: sea_orm::ActiveValue::Set("Nick".into()),
            email: sea_orm::ActiveValue::Set(email.into()),
            role: sea_orm::ActiveValue::Set(person::Role::Admin),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed staff person");
    }

    /// Seed a known client so a conversation with this external party opens
    /// with a `person_id` — past the prospective-client conflict gate.
    async fn seed_client(db: &store::Db, email: &str) {
        use sea_orm::ActiveModelTrait;
        use store::entity::person;
        person::ActiveModel {
            name: sea_orm::ActiveValue::Set("Pisces".into()),
            email: sea_orm::ActiveValue::Set(email.into()),
            role: sea_orm::ActiveValue::Set(person::Role::Client),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed client person");
    }

    #[tokio::test]
    async fn first_contact_opens_conversation_and_notifies_staff() {
        let db = store::test_support::pg().await;
        let cap = CapturingEmail::new();

        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &RecordingRuntime::default(),
            &cfg(),
            &inbound(
                "Pisces <pisces@example.com>",
                "test@parse.neonlaw.com",
                "Question about my LLC",
                "Hi, I have a question.",
            ),
            "inbound/1-pisces.eml",
        )
        .await
        .unwrap();

        let sent = cap.captured();
        assert_eq!(sent.len(), 1, "one staff notification");
        let note = &sent[0];
        assert_eq!(note.to, "nick+aida@neonlaw.com");
        assert_eq!(note.subject, "[Pisces] Question about my LLC");
        let reply_to = note.reply_to.as_deref().expect("reply_to set");
        assert!(reply_to.ends_with("@parse.neonlaw.com"));
        assert!(reply_to.starts_with('c'));
        // never leak an internal address to a header the client would see
        assert!(note.body.contains("Hi, I have a question."));
    }

    #[tokio::test]
    async fn staff_reply_relays_to_the_external_party() {
        let db = store::test_support::pg().await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();
        let cfg = cfg();
        // A known client — past the prospective-client conflict gate.
        seed_client(&db, "pisces@example.com").await;

        // 1. external first contact
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg,
            &inbound(
                "Pisces <pisces@example.com>",
                "test@parse.neonlaw.com",
                "Question about my LLC",
                "Hi, I have a question.",
            ),
            "inbound/1-pisces.eml",
        )
        .await
        .unwrap();
        let token_addr = cap.captured()[0].reply_to.clone().unwrap();

        // 2. staff replies to the token address (with quoted history)
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg,
            &inbound(
                "Nick <nick@neonlaw.com>",
                &format!("\"Support\" <{token_addr}>"),
                "Re: Question about my LLC",
                "Happy to help — here's your answer.\n\nOn Tue wrote:\n> Hi",
            ),
            "inbound/2-nick.eml",
        )
        .await
        .unwrap();

        let sent = cap.captured();
        assert_eq!(sent.len(), 2, "notification + relay");
        let relay = &sent[1];
        assert_eq!(relay.to, "pisces@example.com", "relayed to the client");
        assert_eq!(relay.body, "Happy to help — here's your answer.");
        assert_eq!(relay.subject, "Re: Question about my LLC");
        // the relay must not expose the attorney's address anywhere
        assert!(!relay.to.contains("nick@"));
        assert_ne!(relay.reply_to.as_deref(), Some("nick@neonlaw.com"));
    }

    #[test]
    fn parse_reply_extracts_commands_and_relay_prose() {
        // @approve alongside prose
        let p = parse_reply("Here you go.\n@approve");
        assert_eq!(p.relay_body, "Here you go.");
        assert_eq!(
            p.commands,
            vec![Command::Signal {
                condition: "approved".into(),
                value: None
            }]
        );
        // @deny with a reason
        let p = parse_reply("@deny missing signature");
        assert_eq!(p.relay_body, "");
        assert_eq!(
            p.commands,
            vec![Command::Signal {
                condition: "rejected".into(),
                value: Some("missing signature".into())
            }]
        );
        // generic @signal <condition> <value>
        let p = parse_reply("@signal filed receipt-123");
        assert_eq!(
            p.commands,
            vec![Command::Signal {
                condition: "filed".into(),
                value: Some("receipt-123".into())
            }]
        );
        // @close is a command; the prose stays in relay_body (the caller
        // suppresses the relay on Close)
        let p = parse_reply("ok\n@close");
        assert_eq!(p.commands, vec![Command::Close]);
        assert_eq!(p.relay_body, "ok");
        // a mid-line @mention is NOT a command — only lines starting with @
        let p = parse_reply("hi @pisces\nthanks");
        assert!(p.commands.is_empty());
        assert_eq!(p.relay_body, "hi @pisces\nthanks");
        // @cleared (and its @clear alias) release the conflict gate
        assert_eq!(
            parse_reply("@cleared").commands,
            vec![Command::ConflictCleared]
        );
        assert_eq!(
            parse_reply("@clear").commands,
            vec![Command::ConflictCleared]
        );
        // @link captures the matter id verbatim; the line never relays
        let p = parse_reply("@link 1e2d3c4b-0000-0000-0000-000000000000\nthanks");
        assert_eq!(
            p.commands,
            vec![Command::Link {
                notation_id: "1e2d3c4b-0000-0000-0000-000000000000".into()
            }]
        );
        assert_eq!(p.relay_body, "thanks");
        // bare @link is still a command (id validated at execution) so it is
        // stripped rather than leaking "@link" to the client
        let p = parse_reply("@link");
        assert_eq!(
            p.commands,
            vec![Command::Link {
                notation_id: String::new()
            }]
        );
        assert_eq!(p.relay_body, "");
    }

    #[tokio::test]
    async fn staff_approve_fires_workflow_signal_and_relays_prose() {
        let db = store::test_support::pg().await;
        let notation_id = store::test_support::seed_notation(&db).await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();

        // A conversation already linked to a matter (the staff-review gate).
        let token = "0000000000000000000000000000000a";
        store::email_conversations::open(
            &db,
            &store::email_conversations::NewConversation {
                token,
                external_email: "pisces@example.com",
                external_name: Some("Pisces"),
                subject: "Estate plan",
                person_id: None,
                notation_id: Some(notation_id),
            },
        )
        .await
        .unwrap();

        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg(),
            &inbound(
                "Nick <nick@neonlaw.com>",
                &format!("c{token}@parse.neonlaw.com"),
                "Re: Estate plan",
                "Looks good.\n@approve",
            ),
            "inbound/approve.eml",
        )
        .await
        .unwrap();

        // the workflow signal fired on the linked notation
        let sigs = rt.signals.lock().unwrap();
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0], (notation_id, "approved".to_string(), None));
        // and the accompanying prose still relayed to the client
        let sent = cap.captured();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "pisces@example.com");
        assert_eq!(sent[0].body, "Looks good.");
    }

    #[tokio::test]
    async fn staff_close_suppresses_relay_and_closes_conversation() {
        let db = store::test_support::pg().await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();

        let token = "0000000000000000000000000000000b";
        store::email_conversations::open(
            &db,
            &store::email_conversations::NewConversation {
                token,
                external_email: "pisces@example.com",
                external_name: None,
                subject: "Question",
                person_id: None,
                notation_id: None,
            },
        )
        .await
        .unwrap();

        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg(),
            &inbound(
                "Nick <nick@neonlaw.com>",
                &format!("c{token}@parse.neonlaw.com"),
                "Re: Question",
                "handled offline\n@close",
            ),
            "inbound/close.eml",
        )
        .await
        .unwrap();

        // @close suppresses the relay entirely
        assert!(cap.captured().is_empty(), "no relay on @close");
        // and the conversation is closed
        let conv = store::email_conversations::by_token(&db, token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(conv.status, "closed");
    }

    /// Resolve a notation's matter (project) id for spine assertions.
    async fn project_of(db: &store::Db, notation_id: uuid::Uuid) -> uuid::Uuid {
        use sea_orm::EntityTrait;
        store::entity::notation::Entity::find_by_id(notation_id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .project_id
    }

    #[tokio::test]
    async fn email_exchange_mirrors_into_the_linked_matters_spine() {
        use store::communications::{channel, direction, for_project};

        let db = store::test_support::pg().await;
        let notation_id = store::test_support::seed_notation(&db).await;
        let project_id = project_of(&db, notation_id).await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();

        let token = "00000000000000000000000000000abc";
        seed_linked_conversation(&db, token, notation_id).await;

        // 1. The client writes in (reply to the token address).
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg(),
            &inbound(
                "Pisces <pisces@example.com>",
                &format!("c{token}@parse.neonlaw.com"),
                "Re: Estate plan",
                "Here is the information you asked for.",
            ),
            "inbound/client-1.eml",
        )
        .await
        .unwrap();

        // 2. Staff relays a reply back to the client.
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg(),
            &inbound(
                "Nick <nick@neonlaw.com>",
                &format!("c{token}@parse.neonlaw.com"),
                "Re: Estate plan",
                "Thanks — received, we'll proceed.",
            ),
            "inbound/staff-1.eml",
        )
        .await
        .unwrap();

        let thread = for_project(&db, project_id).await.unwrap();
        let inbound_rows: Vec<_> = thread
            .iter()
            .filter(|c| c.channel == channel::EMAIL_INBOUND)
            .collect();
        let outbound_rows: Vec<_> = thread
            .iter()
            .filter(|c| c.channel == channel::EMAIL_OUTBOUND)
            .collect();
        assert_eq!(inbound_rows.len(), 1, "client message mirrored inbound");
        assert_eq!(outbound_rows.len(), 1, "firm relay mirrored outbound");
        assert_eq!(
            inbound_rows[0].body,
            "Here is the information you asked for."
        );
        assert_eq!(inbound_rows[0].direction, direction::INBOUND);
        assert_eq!(outbound_rows[0].direction, direction::OUTBOUND);

        // Idempotent: re-mirroring the same conversation adds nothing.
        let conv = store::email_conversations::by_token(&db, token)
            .await
            .unwrap()
            .unwrap();
        super::sync_conversation_to_spine(
            &super::ThreadCtx {
                db: &db,
                storage: &storage().await,
                email: &cap,
                runtime: &rt,
                cfg: &cfg(),
            },
            conv.id,
        )
        .await
        .unwrap();
        assert_eq!(
            for_project(&db, project_id).await.unwrap().len(),
            thread.len(),
            "re-sync must not duplicate spine rows"
        );
    }

    #[tokio::test]
    async fn first_contact_auto_links_known_client_with_one_open_matter() {
        use store::communications::channel;

        let db = store::test_support::pg().await;
        // seed_notation makes libra@example.com the client on one open matter.
        let notation_id = store::test_support::seed_notation(&db).await;
        let project_id = project_of(&db, notation_id).await;
        let cap = CapturingEmail::new();

        // First contact (no token) from that known client.
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &RecordingRuntime::default(),
            &cfg(),
            &inbound(
                "Libra <libra@example.com>",
                "support@parse.neonlaw.com",
                "A new question",
                "Hello, one more thing.",
            ),
            "inbound/libra-first.eml",
        )
        .await
        .unwrap();

        // The conversation auto-linked to the sole open matter, so the first
        // inbound hop already lands in that matter's conversation log.
        let thread = for_project_helper(&db, project_id).await;
        assert_eq!(thread.len(), 1);
        assert_eq!(thread[0].channel, channel::EMAIL_INBOUND);
        assert_eq!(thread[0].body, "Hello, one more thing.");
    }

    async fn for_project_helper(
        db: &store::Db,
        project_id: uuid::Uuid,
    ) -> Vec<store::entity::communication::Model> {
        store::communications::for_project(db, project_id)
            .await
            .unwrap()
    }

    /// A `ThreadConfig` with the command-channel DKIM gate enabled.
    fn cfg_dkim() -> ThreadConfig {
        ThreadConfig {
            verify_dkim_domain: Some("neonlaw.com".into()),
            ..cfg()
        }
    }

    /// Open a conversation linked to `notation_id` under a fixed token.
    async fn seed_linked_conversation(db: &store::Db, token: &str, notation_id: uuid::Uuid) {
        store::email_conversations::open(
            db,
            &store::email_conversations::NewConversation {
                token,
                external_email: "pisces@example.com",
                external_name: Some("Pisces"),
                subject: "Estate plan",
                person_id: None,
                notation_id: Some(notation_id),
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn dkim_failure_blocks_staff_command_and_relay_when_enforced() {
        let db = store::test_support::pg().await;
        let notation_id = store::test_support::seed_notation(&db).await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();

        let token = "0000000000000000000000000000000c";
        seed_linked_conversation(&db, token, notation_id).await;

        // A reply that claims to be from staff but whose DKIM verdict is a
        // fail for the firm domain — the forged-From / leaked-token case.
        let mut msg = inbound(
            "Nick <nick@neonlaw.com>",
            &format!("c{token}@parse.neonlaw.com"),
            "Re: Estate plan",
            "Looks good.\n@approve",
        );
        msg.dkim = "{@neonlaw.com : fail}".into();

        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg_dkim(),
            &msg,
            "inbound/forged.eml",
        )
        .await
        .unwrap();

        // The privileged actions are both refused: no workflow signal, no relay.
        assert!(
            rt.signals.lock().unwrap().is_empty(),
            "no workflow signal may fire on a DKIM failure"
        );
        assert!(
            cap.captured().is_empty(),
            "no content may relay on a DKIM failure"
        );
    }

    #[tokio::test]
    async fn dkim_pass_allows_staff_command_when_enforced() {
        let db = store::test_support::pg().await;
        let notation_id = store::test_support::seed_notation(&db).await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();

        let token = "0000000000000000000000000000000d";
        seed_linked_conversation(&db, token, notation_id).await;

        let mut msg = inbound(
            "Nick <nick@neonlaw.com>",
            &format!("c{token}@parse.neonlaw.com"),
            "Re: Estate plan",
            "Looks good.\n@approve",
        );
        msg.dkim = "{@neonlaw.com : pass}".into();

        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg_dkim(),
            &msg,
            "inbound/genuine.eml",
        )
        .await
        .unwrap();

        // DKIM passed → the signal fires and the prose relays, exactly as
        // the un-gated path does.
        let sigs = rt.signals.lock().unwrap();
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0], (notation_id, "approved".to_string(), None));
        let sent = cap.captured();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "pisces@example.com");
        assert_eq!(sent[0].body, "Looks good.");
    }

    /// Open a support conversation with no linked matter — the prod state
    /// before any `@link`, where `@approve` would no-op.
    async fn seed_unlinked_conversation(db: &store::Db, token: &str) {
        store::email_conversations::open(
            db,
            &store::email_conversations::NewConversation {
                token,
                external_email: "pisces@example.com",
                external_name: Some("Pisces"),
                subject: "Estate plan",
                person_id: None,
                notation_id: None,
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn link_binds_conversation_and_same_reply_approve_fires() {
        let db = store::test_support::pg().await;
        let notation_id = store::test_support::seed_notation(&db).await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();

        let token = "0000000000000000000000000000000e";
        seed_unlinked_conversation(&db, token).await;

        // One staff reply links the thread to the matter, then approves on it.
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg(),
            &inbound(
                "Nick <nick@neonlaw.com>",
                &format!("c{token}@parse.neonlaw.com"),
                "Re: Estate plan",
                &format!("@link {notation_id}\n@approve"),
            ),
            "inbound/link.eml",
        )
        .await
        .unwrap();

        // The conversation is bound to the matter...
        let conv = store::email_conversations::by_token(&db, token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(conv.notation_id, Some(notation_id));
        // ...and the same-message @approve fired on it (the freshly linked
        // notation is visible to the later command in the same reply).
        let sigs = rt.signals.lock().unwrap();
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0], (notation_id, "approved".to_string(), None));
        // Nothing relayed to the client — both lines were commands; the only
        // outbound is the cockpit-facing [link] confirmation.
        assert!(
            cap.captured().iter().all(|m| m.to != "pisces@example.com"),
            "@link/@approve must not relay to the client"
        );
    }

    #[tokio::test]
    async fn link_to_unknown_matter_leaves_conversation_unlinked() {
        let db = store::test_support::pg().await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();

        let token = "0000000000000000000000000000000f";
        seed_unlinked_conversation(&db, token).await;

        let bogus = "11111111-2222-3333-4444-555555555555";
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg(),
            &inbound(
                "Nick <nick@neonlaw.com>",
                &format!("c{token}@parse.neonlaw.com"),
                "Re: Estate plan",
                &format!("@link {bogus}\n@approve"),
            ),
            "inbound/link-bad.eml",
        )
        .await
        .unwrap();

        // An id with no matter behind it links nothing and fires nothing.
        let conv = store::email_conversations::by_token(&db, token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(conv.notation_id, None);
        assert!(rt.signals.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn first_contact_threads_notification_to_client_message_id() {
        let db = store::test_support::pg().await;
        let cap = CapturingEmail::new();

        let mut msg = inbound(
            "Pisces <pisces@example.com>",
            "test@parse.neonlaw.com",
            "Question about my LLC",
            "Hi, I have a question.",
        );
        msg.message_id = Some("client-1@mail.example.com".into());

        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &RecordingRuntime::default(),
            &cfg(),
            &msg,
            "inbound/1-pisces.eml",
        )
        .await
        .unwrap();

        let sent = cap.captured();
        assert_eq!(sent.len(), 1);
        // The staff notification references the client's message so the
        // attorney's mail client threads the exchange.
        assert_eq!(
            sent[0].in_reply_to.as_deref(),
            Some("<client-1@mail.example.com>")
        );
        assert_eq!(
            sent[0].references.as_deref(),
            Some("<client-1@mail.example.com>")
        );
    }

    #[tokio::test]
    async fn staff_relay_threads_with_the_full_message_id_chain() {
        let db = store::test_support::pg().await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();
        let cfg = cfg();
        // A known client — past the prospective-client conflict gate.
        seed_client(&db, "pisces@example.com").await;

        // 1. Client first contact carries a message-id.
        let mut first = inbound(
            "Pisces <pisces@example.com>",
            "test@parse.neonlaw.com",
            "Question about my LLC",
            "Hi.",
        );
        first.message_id = Some("client-1@mail".into());
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg,
            &first,
            "inbound/1.eml",
        )
        .await
        .unwrap();
        let token_addr = cap.captured()[0].reply_to.clone().unwrap();

        // 2. Staff reply (its own message-id) relays back to the client.
        let mut reply = inbound(
            "Nick <nick@neonlaw.com>",
            &format!("\"Support\" <{token_addr}>"),
            "Re: Question about my LLC",
            "Here's your answer.",
        );
        reply.message_id = Some("staff-1@mail".into());
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg,
            &reply,
            "inbound/2.eml",
        )
        .await
        .unwrap();

        let sent = cap.captured();
        let relay = &sent[1];
        assert_eq!(relay.to, "pisces@example.com");
        // References carries the whole inbound chain, oldest first;
        // In-Reply-To points at the most recent inbound hop (the staff reply).
        assert_eq!(
            relay.references.as_deref(),
            Some("<client-1@mail> <staff-1@mail>")
        );
        assert_eq!(relay.in_reply_to.as_deref(), Some("<staff-1@mail>"));
    }

    #[tokio::test]
    async fn first_contact_from_unknown_prompts_conflict_check() {
        let db = store::test_support::pg().await;
        let cap = CapturingEmail::new();
        // pisces is NOT seeded → a prospective client.
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &RecordingRuntime::default(),
            &cfg(),
            &inbound(
                "Pisces <pisces@example.com>",
                "test@parse.neonlaw.com",
                "New matter",
                "Can you help?",
            ),
            "inbound/prospect.eml",
        )
        .await
        .unwrap();

        let sent = cap.captured();
        assert_eq!(sent.len(), 1);
        // The notification prompts staff to run the firm-wide conflict check.
        assert!(sent[0].body.contains("Prospective client"));
        assert!(sent[0].body.contains("conflict check"));
        assert!(sent[0].body.contains("@cleared"));
    }

    #[tokio::test]
    async fn relay_to_uncleared_prospect_is_held() {
        let db = store::test_support::pg().await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();
        let cfg = cfg();

        // Prospect first contact (pisces unseeded, no linked matter).
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg,
            &inbound(
                "Pisces <pisces@example.com>",
                "test@parse.neonlaw.com",
                "New matter",
                "Help?",
            ),
            "inbound/1.eml",
        )
        .await
        .unwrap();
        let token_addr = cap.captured()[0].reply_to.clone().unwrap();

        // Staff replies with prose but NO @cleared → the relay is held.
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg,
            &inbound(
                "Nick <nick@neonlaw.com>",
                &format!("\"Support\" <{token_addr}>"),
                "Re: New matter",
                "Sure, happy to help.",
            ),
            "inbound/2.eml",
        )
        .await
        .unwrap();

        let sent = cap.captured();
        // Nothing reaches the prospect...
        assert!(
            sent.iter().all(|m| m.to != "pisces@example.com"),
            "no relay to an uncleared prospect"
        );
        // ...and staff are prompted to run the conflict check.
        assert!(
            sent.iter()
                .any(|m| m.to == "nick+aida@neonlaw.com" && m.body.contains("NOT relayed")),
            "staff prompted to run the conflict check before the relay"
        );
    }

    #[tokio::test]
    async fn cleared_releases_the_relay_to_a_prospect() {
        let db = store::test_support::pg().await;
        seed_staff(&db, "nick@neonlaw.com").await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();
        let cfg = cfg();

        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg,
            &inbound(
                "Pisces <pisces@example.com>",
                "test@parse.neonlaw.com",
                "New matter",
                "Help?",
            ),
            "inbound/1.eml",
        )
        .await
        .unwrap();
        let token_addr = cap.captured()[0].reply_to.clone().unwrap();
        let staff_to = format!("\"Support\" <{token_addr}>");

        // Staff clears the check and answers in one reply.
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg,
            &inbound(
                "Nick <nick@neonlaw.com>",
                &staff_to,
                "Re: New matter",
                "@cleared\nHappy to help.",
            ),
            "inbound/2.eml",
        )
        .await
        .unwrap();

        let relay = cap
            .captured()
            .into_iter()
            .find(|m| m.to == "pisces@example.com")
            .expect("relay released after @cleared");
        assert_eq!(relay.body, "Happy to help.");

        // Clearance persists: a later plain reply relays without re-prompting.
        thread_inbound(
            &db,
            &storage().await,
            &cap,
            &rt,
            &cfg,
            &inbound(
                "Nick <nick@neonlaw.com>",
                &staff_to,
                "Re: New matter",
                "Just following up.",
            ),
            "inbound/3.eml",
        )
        .await
        .unwrap();
        assert_eq!(
            cap.captured()
                .iter()
                .filter(|m| m.to == "pisces@example.com")
                .count(),
            2,
            "clearance persists for subsequent relays"
        );
    }

    #[tokio::test]
    async fn attachment_on_linked_thread_files_a_document_and_notifies() {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        use store::entity::document;

        let db = store::test_support::pg().await;
        let storage = storage().await;
        let notation_id = store::test_support::seed_notation(&db).await;
        let project_id = store::entity::notation::Entity::find_by_id(notation_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .project_id;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();

        let token = "0000000000000000000000000000000e";
        seed_linked_conversation(&db, token, notation_id).await;

        // The client replies on the matter thread with a PDF attached.
        let mut msg = inbound(
            "Pisces <pisces@example.com>",
            &format!("c{token}@parse.neonlaw.com"),
            "Re: Estate plan",
            "Here is my signed form.",
        );
        msg.attachments = vec![InboundAttachment {
            filename: "signed-form.pdf".into(),
            content_type: "application/pdf".into(),
            bytes: b"%PDF-1.4 fake".to_vec(),
        }];

        thread_inbound(&db, &storage, &cap, &rt, &cfg(), &msg, "inbound/form.eml")
            .await
            .unwrap();

        // The attachment is filed as a document on the matter's project.
        let docs = document::Entity::find()
            .filter(document::Column::ProjectId.eq(project_id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(docs.len(), 1, "one document filed");
        assert_eq!(docs[0].filename, "signed-form.pdf");
        assert_eq!(docs[0].source, "email");
        assert_eq!(docs[0].kind, "unclassified");

        // The transcript records the ingest as a `system` hop.
        let convo = store::email_conversations::by_token(&db, token)
            .await
            .unwrap()
            .unwrap();
        let msgs = store::email_conversations::messages(&db, convo.id)
            .await
            .unwrap();
        assert!(
            msgs.iter()
                .any(|m| m.direction == "system" && m.body_text.contains("signed-form.pdf")),
            "a system hop records the filed document"
        );

        // Staff are notified with the review request folded into the body.
        let sent = cap.captured();
        assert_eq!(sent.len(), 1, "one staff notification");
        assert!(sent[0].body.contains("document(s) received for review"));
        assert!(sent[0].body.contains("signed-form.pdf"));
    }

    #[tokio::test]
    async fn attachment_on_unlinked_thread_is_archived_not_filed() {
        use sea_orm::EntityTrait;
        use store::entity::document;

        let db = store::test_support::pg().await;
        let storage = storage().await;
        let cap = CapturingEmail::new();
        let rt = RecordingRuntime::default();

        // A conversation with NO linked matter — nothing to file documents under.
        let token = "0000000000000000000000000000000f";
        store::email_conversations::open(
            &db,
            &store::email_conversations::NewConversation {
                token,
                external_email: "pisces@example.com",
                external_name: Some("Pisces"),
                subject: "Question",
                person_id: None,
                notation_id: None,
            },
        )
        .await
        .unwrap();

        let mut msg = inbound(
            "Pisces <pisces@example.com>",
            &format!("c{token}@parse.neonlaw.com"),
            "Re: Question",
            "A document for you.",
        );
        msg.attachments = vec![InboundAttachment {
            filename: "doc.pdf".into(),
            content_type: "application/pdf".into(),
            bytes: b"%PDF bytes".to_vec(),
        }];

        thread_inbound(
            &db,
            &storage,
            &cap,
            &rt,
            &cfg(),
            &msg,
            "inbound/unlinked.eml",
        )
        .await
        .unwrap();

        // No document is filed...
        let docs = document::Entity::find().all(&db).await.unwrap();
        assert!(docs.is_empty(), "no document without a linked matter");
        // ...but staff are told it arrived and how to file it.
        let sent = cap.captured();
        assert_eq!(sent.len(), 1);
        assert!(sent[0]
            .body
            .contains("link this thread to a matter to file them"));
    }
}
