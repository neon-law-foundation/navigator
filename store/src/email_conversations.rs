//! Helpers for the `email_conversations` + `email_conversation_messages`
//! tables — the threaded support inbox behind `support@neonlaw.com`.
//!
//! `web` reaches these so it can open a thread when a new message lands,
//! look one up by its `Reply-To` token when a reply comes back, append
//! each hop to the transcript, and project the conversation's `status`.
//! The thread token is **caller-supplied** — `web` mints an unguessable
//! value; `store` stays free of randomness so its behavior is
//! deterministic under test.

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::entity::email_conversation::STATUS_OPEN;
use crate::entity::{email_conversation, email_conversation_message};
use crate::Db;

/// What to record when opening a new support thread. `status` defaults to
/// [`STATUS_OPEN`] via [`open`].
#[derive(Debug, Clone)]
pub struct NewConversation<'a> {
    /// Unguessable thread token; the VERP key carried in `Reply-To`.
    pub token: &'a str,
    pub external_email: &'a str,
    pub external_name: Option<&'a str>,
    pub subject: &'a str,
    /// Matched `persons.id`, if the sender is already known.
    pub person_id: Option<Uuid>,
    /// Linked matter, if this thread drives one.
    pub notation_id: Option<Uuid>,
}

/// One hop to append to a thread's transcript. See the `DIRECTION_*`
/// constants on [`email_conversation_message`].
///
/// `Default` lets callers fill only the fields a given hop needs and
/// elide the optional tail (`raw_storage_key` / `provider_message_id` /
/// `in_reply_to` / `command_payload`) with `..Default::default()` — the
/// required fields are always set explicitly at the call site.
#[derive(Debug, Clone, Default)]
pub struct NewMessage<'a> {
    pub conversation_id: Uuid,
    pub direction: &'a str,
    pub from_addr: &'a str,
    pub to_addr: &'a str,
    pub subject: &'a str,
    pub body_text: &'a str,
    pub raw_storage_key: Option<&'a str>,
    pub provider_message_id: Option<&'a str>,
    pub in_reply_to: Option<&'a str>,
    pub command_payload: Option<&'a str>,
}

/// Open a new conversation at `status = open`, returning its id.
///
/// # Errors
///
/// Propagates any database error (including a unique-token violation).
pub async fn open(db: &Db, new: &NewConversation<'_>) -> Result<Uuid, sea_orm::DbErr> {
    let row = email_conversation::ActiveModel {
        token: ActiveValue::Set(new.token.to_string()),
        external_email: ActiveValue::Set(new.external_email.to_string()),
        external_name: ActiveValue::Set(new.external_name.map(str::to_string)),
        person_id: ActiveValue::Set(new.person_id),
        subject: ActiveValue::Set(new.subject.to_string()),
        status: ActiveValue::Set(STATUS_OPEN.to_string()),
        notation_id: ActiveValue::Set(new.notation_id),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

/// Look up a conversation by its `Reply-To` token — the threading lookup
/// run on every inbound reply.
///
/// # Errors
///
/// Propagates any database error.
pub async fn by_token(
    db: &Db,
    token: &str,
) -> Result<Option<email_conversation::Model>, sea_orm::DbErr> {
    email_conversation::Entity::find()
        .filter(email_conversation::Column::Token.eq(token))
        .one(db)
        .await
}

/// Load one conversation by id.
///
/// # Errors
///
/// Propagates any database error.
pub async fn by_id(db: &Db, id: Uuid) -> Result<Option<email_conversation::Model>, sea_orm::DbErr> {
    email_conversation::Entity::find_by_id(id).one(db).await
}

/// Append one hop to a thread's transcript, returning its id. Does not
/// touch the conversation's `status` — callers advance that explicitly
/// via [`set_status`] so the projection stays under their control.
///
/// # Errors
///
/// Propagates any database error.
pub async fn append(db: &Db, new: &NewMessage<'_>) -> Result<Uuid, sea_orm::DbErr> {
    let row = email_conversation_message::ActiveModel {
        conversation_id: ActiveValue::Set(new.conversation_id),
        direction: ActiveValue::Set(new.direction.to_string()),
        from_addr: ActiveValue::Set(new.from_addr.to_string()),
        to_addr: ActiveValue::Set(new.to_addr.to_string()),
        subject: ActiveValue::Set(new.subject.to_string()),
        body_text: ActiveValue::Set(new.body_text.to_string()),
        raw_storage_key: ActiveValue::Set(new.raw_storage_key.map(str::to_string)),
        provider_message_id: ActiveValue::Set(new.provider_message_id.map(str::to_string)),
        in_reply_to: ActiveValue::Set(new.in_reply_to.map(str::to_string)),
        command_payload: ActiveValue::Set(new.command_payload.map(str::to_string)),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

/// The full transcript of a conversation, oldest hop first.
///
/// # Errors
///
/// Propagates any database error.
pub async fn messages(
    db: &Db,
    conversation_id: Uuid,
) -> Result<Vec<email_conversation_message::Model>, sea_orm::DbErr> {
    email_conversation_message::Entity::find()
        .filter(email_conversation_message::Column::ConversationId.eq(conversation_id))
        .order_by_asc(email_conversation_message::Column::Id)
        .all(db)
        .await
}

/// Move a conversation to a new `status`. Returns the updated row, or
/// `Ok(None)` if no row matched.
///
/// # Errors
///
/// Propagates any database error.
pub async fn set_status(
    db: &Db,
    id: Uuid,
    status: &str,
) -> Result<Option<email_conversation::Model>, sea_orm::DbErr> {
    let Some(row) = email_conversation::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: email_conversation::ActiveModel = row.into();
    active.status = ActiveValue::Set(status.to_string());
    Ok(Some(active.update(db).await?))
}

/// Link a conversation to a running workflow notation (the `@link` staff
/// command). Once set, the `@approve`/`@deny`/`@signal` command channel
/// fires on this notation and inbound attachments file onto its matter.
/// Returns `None` when no conversation has `id`.
///
/// # Errors
///
/// Propagates any database error.
pub async fn set_notation(
    db: &Db,
    id: Uuid,
    notation_id: Uuid,
) -> Result<Option<email_conversation::Model>, sea_orm::DbErr> {
    let Some(row) = email_conversation::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: email_conversation::ActiveModel = row.into();
    active.notation_id = ActiveValue::Set(Some(notation_id));
    Ok(Some(active.update(db).await?))
}

#[cfg(test)]
mod tests {
    use super::{append, by_token, messages, open, set_status, NewConversation, NewMessage};
    use crate::entity::email_conversation::{STATUS_AWAITING_STAFF, STATUS_OPEN};
    use crate::entity::email_conversation_message::{DIRECTION_FROM_EXTERNAL, DIRECTION_TO_STAFF};

    #[tokio::test]
    async fn open_then_thread_back_by_token() {
        let db = crate::test_support::pg().await;

        let id = open(
            &db,
            &NewConversation {
                token: "tok_pisces_001",
                external_email: "pisces@example.com",
                external_name: Some("Pisces"),
                subject: "Question about my LLC",
                person_id: None,
                notation_id: None,
            },
        )
        .await
        .unwrap();

        // The inbound reply path looks the thread up by its Reply-To token.
        let found = by_token(&db, "tok_pisces_001").await.unwrap().unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.status, STATUS_OPEN);
        assert_eq!(found.external_name.as_deref(), Some("Pisces"));

        // An unknown token threads to nothing.
        assert!(by_token(&db, "tok_nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn transcript_is_ordered_and_status_projects() {
        let db = crate::test_support::pg().await;
        let id = open(
            &db,
            &NewConversation {
                token: "tok_pisces_002",
                external_email: "pisces@example.com",
                external_name: None,
                subject: "PDF to authorize",
                person_id: None,
                notation_id: None,
            },
        )
        .await
        .unwrap();

        append(
            &db,
            &NewMessage {
                conversation_id: id,
                direction: DIRECTION_FROM_EXTERNAL,
                from_addr: "pisces@example.com",
                to_addr: "support@neonlaw.com",
                subject: "PDF to authorize",
                body_text: "Please review the attached.",
                raw_storage_key: Some("inbound/1234-pisces.eml"),
                provider_message_id: Some("<msg-1@mail>"),
                in_reply_to: None,
                command_payload: None,
            },
        )
        .await
        .unwrap();
        append(
            &db,
            &NewMessage {
                conversation_id: id,
                direction: DIRECTION_TO_STAFF,
                from_addr: "support@neonlaw.com",
                to_addr: "nick+aida@neonlaw.com",
                subject: "[Pisces] PDF to authorize",
                body_text: "Pisces sent a PDF to authorize.",
                raw_storage_key: None,
                provider_message_id: Some("<msg-2@sg>"),
                in_reply_to: None,
                command_payload: None,
            },
        )
        .await
        .unwrap();

        let transcript = messages(&db, id).await.unwrap();
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[0].direction, DIRECTION_FROM_EXTERNAL);
        assert_eq!(transcript[1].direction, DIRECTION_TO_STAFF);
        assert_eq!(transcript[1].to_addr, "nick+aida@neonlaw.com");

        let updated = set_status(&db, id, STATUS_AWAITING_STAFF)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, STATUS_AWAITING_STAFF);
    }

    #[tokio::test]
    async fn token_is_unique() {
        let db = crate::test_support::pg().await;
        let new = NewConversation {
            token: "tok_dupe",
            external_email: "a@example.com",
            external_name: None,
            subject: "first",
            person_id: None,
            notation_id: None,
        };
        open(&db, &new).await.unwrap();
        let err = open(&db, &new).await.unwrap_err();
        assert!(
            crate::is_unique_violation(&err),
            "expected unique violation, got {err:?}"
        );
    }
}
