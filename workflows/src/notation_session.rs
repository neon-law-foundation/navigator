//! Walk a Notation's questionnaire one answer at a time.
//!
//! Both the admin HTML form (`web::retainer_walk`) and the MCP
//! tools (`aida_create_notation`, `aida_answer_notation`) drive a
//! Notation through the same two state machines: a questionnaire
//! that asks one question per signal, then a post-intake workflow.
//! This module owns the questionnaire half; the workflow half is
//! caller-driven for now (the dev-loop short-circuit in
//! `retainer_walk::drive_post_questionnaire_workflow` stays in
//! `web`).
//!
//! The runtime — not the application — is the source of truth for
//! questionnaire state. That mirrors `retainer_walk` exactly: in
//! production, the `workflows-service` worker journals each
//! transition inside `ctx.run("append-…", …)`; in tests, the
//! in-memory runtime records the transition in its own `Vec`.
//! Callers do not write `notation_events` themselves.

use std::collections::BTreeMap;
use std::sync::Arc;

use cloud::StorageService;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use thiserror::Error;
use uuid::Uuid;

use store::entity::{answer, notation, person, question, question_translation, template};
use store::Db;

use crate::runtime::{StateMachineRuntime, WorkflowRuntimeError};
use crate::spec::{MachineKind, QuestionnaireSpec, StateName, WorkflowSpecError};
use crate::specs::{
    bundled_spec_yaml, prompt_overrides_from_template, prompt_overrides_from_yaml,
    questionnaire_spec_from_template, questionnaire_spec_from_yaml,
};

/// One question presented to the caller — the prompt, the answer
/// shape, and the stable code the caller must echo back on the
/// next `answer_step`. `id` is the row id of the question; the
/// MCP surface ignores it but the admin HTML form uses it to look
/// up any prior answer for the (question, person) pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionDescriptor {
    pub id: Uuid,
    pub code: String,
    pub prompt: String,
    pub answer_type: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct QuestionnaireDefinition {
    spec: QuestionnaireSpec,
    prompts: BTreeMap<String, String>,
}

/// Where the questionnaire is after a `start_notation` /
/// `answer_step` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextStep {
    /// The caller must collect this answer and call `answer_step`.
    NeedsAnswer { question: QuestionDescriptor },
    /// The questionnaire reached `END`. The post-intake workflow
    /// has *not* been started by this module — the caller decides
    /// when and how to kick it off.
    QuestionnaireComplete,
}

/// Output of [`start_notation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartOutcome {
    pub notation_id: Uuid,
    pub next: NextStep,
}

/// Who entered an answer. The notation's bound Person is always the
/// *respondent* (`answers.person_id`); this records who actually typed
/// the value — the staff member filling it in on the client's behalf, or
/// the client themselves through the magic link — and the authorship
/// `source` ([`answer::SOURCE_STAFF`] / [`answer::SOURCE_CLIENT`]) that
/// the data lake groups by.
#[derive(Debug, Clone, Copy)]
pub struct AnswerAuthor<'a> {
    /// FK → persons: who typed the answer. `None` for system/agent
    /// answers with no individual Person row.
    pub authored_by: Option<Uuid>,
    /// `answer::SOURCE_STAFF` or `answer::SOURCE_CLIENT`.
    pub source: &'a str,
}

impl AnswerAuthor<'static> {
    /// A staff-sourced answer typed by `authored_by` (the logged-in
    /// staff/admin person, or `None` for the agent surface).
    #[must_use]
    pub fn staff(authored_by: Option<Uuid>) -> Self {
        Self {
            authored_by,
            source: answer::SOURCE_STAFF,
        }
    }

    /// A client-sourced answer self-entered by `authored_by` through the
    /// magic link.
    #[must_use]
    pub fn client(authored_by: Option<Uuid>) -> Self {
        Self {
            authored_by,
            source: answer::SOURCE_CLIENT,
        }
    }
}

#[derive(Debug, Error)]
pub enum NotationSessionError {
    #[error("template `{0}` not found")]
    TemplateNotFound(String),
    #[error("template `{0}` has no questionnaire frontmatter")]
    TemplateHasNoQuestionnaire(String),
    #[error("notation `{0}` not found")]
    NotationNotFound(Uuid),
    #[error("question `{0}` not seeded in store")]
    QuestionNotSeeded(String),
    #[error("question `{0}` is not a client-facing question on this notation's intake")]
    QuestionNotClientFacing(String),
    #[error("question code mismatch: questionnaire is currently asking `{expected}`, got `{got}`")]
    QuestionMismatch { expected: String, got: String },
    #[error("questionnaire is already complete")]
    AlreadyComplete,
    #[error("workflow runtime: {0}")]
    Runtime(#[from] WorkflowRuntimeError),
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("spec parse: {0}")]
    Spec(#[from] WorkflowSpecError),
    #[error("encoding questionnaire snapshot: {0}")]
    SnapshotEncode(String),
    #[error("decoding questionnaire snapshot: {0}")]
    SnapshotDecode(String),
}

/// Create a fresh Notation for `template_code`, start the
/// questionnaire runtime, and return the first question.
///
/// `person_id`, `project_id`, and `entity_id` are caller-resolved —
/// neither this module nor the runtime invents respondents,
/// matters, or entities. Every Notation belongs to exactly one
/// Project; see `docs/notation.md#notation`.
pub async fn start_notation(
    db: &Db,
    runtime: &dyn StateMachineRuntime,
    storage: Option<&Arc<dyn StorageService>>,
    template_code: &str,
    person_id: Uuid,
    project_id: Uuid,
    entity_id: Option<Uuid>,
) -> Result<StartOutcome, NotationSessionError> {
    // Prefer a Project-scoped template, falling back to the shared one.
    let template_row = store::templates::resolve(db, Some(project_id), template_code)
        .await?
        .ok_or_else(|| NotationSessionError::TemplateNotFound(template_code.into()))?;

    let definition = questionnaire_definition_for(db, storage, &template_row).await?;

    // Freeze the askable set: this exact traversal graph drives every
    // later render/step/fill, so a template edit or binary change can't
    // re-route an in-flight Notation.
    let snapshot = serde_json::to_value(&definition)
        .map_err(|e| NotationSessionError::SnapshotEncode(e.to_string()))?;

    let notation_id = notation::ActiveModel {
        template_id: ActiveValue::Set(template_row.id),
        person_id: ActiveValue::Set(person_id),
        entity_id: ActiveValue::Set(entity_id),
        project_id: ActiveValue::Set(project_id),
        state: ActiveValue::Set(StateName::BEGIN.into()),
        questionnaire_snapshot: ActiveValue::Set(Some(snapshot)),
        ..Default::default()
    }
    .insert(db)
    .await?
    .id;

    runtime
        .start(
            MachineKind::Questionnaire,
            notation_id,
            definition.spec.inner(),
        )
        .await?;

    let locale = resolve_locale(db, person_id).await?;
    let next = first_step(db, &definition, &locale).await?;
    Ok(StartOutcome { notation_id, next })
}

/// Look up the question the questionnaire is *currently* asking,
/// without writing anything. Returns `QuestionnaireComplete` when
/// the questionnaire has already reached END.
pub async fn current_step(
    db: &Db,
    runtime: &dyn StateMachineRuntime,
    storage: Option<&Arc<dyn StorageService>>,
    notation_id: Uuid,
) -> Result<NextStep, NotationSessionError> {
    let (notation_row, definition) = load_notation_and_spec(db, storage, notation_id).await?;
    let locale = resolve_locale(db, notation_row.person_id).await?;
    let current_state = runtime
        .current_state(MachineKind::Questionnaire, notation_id)
        .await
        .unwrap_or_else(StateName::begin);
    next_step_from(db, &definition, &current_state, &locale).await
}

/// Persist one answer, advance the questionnaire, and return the
/// next question — or `QuestionnaireComplete` if that answer
/// landed the machine at END.
///
/// `question_code` MUST match the question the runtime is
/// currently expecting; mismatches return [`NotationSessionError::QuestionMismatch`]
/// so a confused caller fails fast rather than silently writing an
/// answer against the wrong question.
///
/// `author` records who typed the answer and the authorship source (see
/// [`AnswerAuthor`]). The notation's bound Person stays the *respondent*
/// (`answers.person_id`) regardless of who entered it, so a staff-entered
/// and a client-entered answer to the same question share a respondent
/// but differ in authorship.
pub async fn answer_step(
    db: &Db,
    runtime: &dyn StateMachineRuntime,
    storage: Option<&Arc<dyn StorageService>>,
    notation_id: Uuid,
    question_code: &str,
    value: &str,
    author: AnswerAuthor<'_>,
) -> Result<NextStep, NotationSessionError> {
    let (notation_row, definition) = load_notation_and_spec(db, storage, notation_id).await?;
    let person_id = notation_row.person_id;

    let current_state = runtime
        .current_state(MachineKind::Questionnaire, notation_id)
        .await
        .unwrap_or_else(StateName::begin);

    let expected = definition
        .spec
        .transitions_from(&current_state)
        .and_then(|t| t.lookup("_"))
        .cloned()
        .ok_or(NotationSessionError::AlreadyComplete)?;
    if expected == StateName::end() {
        return Err(NotationSessionError::AlreadyComplete);
    }
    if expected.as_str() != question_code {
        return Err(NotationSessionError::QuestionMismatch {
            expected: expected.0,
            got: question_code.into(),
        });
    }

    let canonical_code = question_code_for_state(question_code);
    let question_row = question::Entity::find()
        .filter(question::Column::Code.eq(canonical_code))
        .one(db)
        .await?
        .ok_or_else(|| NotationSessionError::QuestionNotSeeded(question_code.into()))?;

    // The Answer row is application data; the worker doesn't know
    // about it, so we own the write here. Single insert — no txn.
    // `person_id` is the respondent; `authored_by`/`source` record who
    // actually entered it (staff on the client's behalf, or the client).
    answer::ActiveModel {
        question_id: ActiveValue::Set(question_row.id),
        person_id: ActiveValue::Set(person_id),
        notation_id: ActiveValue::Set(Some(notation_id)),
        // The walked state name carries the `<type>__<role>` discriminator
        // (`entity__company`); the question row points at the bare code.
        state_name: ActiveValue::Set(Some(question_code.to_string())),
        value: ActiveValue::Set(answer::primitive(value)),
        source: ActiveValue::Set(author.source.to_string()),
        authored_by_person_id: ActiveValue::Set(author.authored_by),
        ..Default::default()
    }
    .insert(db)
    .await?;

    // `start` is idempotent; subsequent calls are no-ops. `signal`
    // advances state and (in production) triggers the worker's
    // `ctx.run` journal write — including stamping the answer
    // value as `payload`.
    runtime
        .start(
            MachineKind::Questionnaire,
            notation_id,
            definition.spec.inner(),
        )
        .await?;
    runtime
        .signal(MachineKind::Questionnaire, notation_id, "_", Some(value))
        .await?;

    // If the next transition would land at END, fire the final
    // signal so the machine actually reaches END before we report
    // completion.
    let next_after = definition
        .spec
        .transitions_from(&expected)
        .and_then(|t| t.lookup("_"))
        .cloned();
    if matches!(&next_after, Some(s) if s == &StateName::end()) {
        runtime
            .signal(MachineKind::Questionnaire, notation_id, "_", None)
            .await?;
        return Ok(NextStep::QuestionnaireComplete);
    }

    let next_state = next_after.ok_or(NotationSessionError::AlreadyComplete)?;
    let locale = resolve_locale(db, person_id).await?;
    Ok(NextStep::NeedsAnswer {
        question: load_question(db, &next_state, &locale, &definition.prompts).await?,
    })
}

/// The client's place in *their* portion of a notation's intake.
///
/// The client sees only the questions whose `audience` is `client` or
/// `both` ([`store::entity::question::is_client_facing`]), in spec order.
/// Unlike [`answer_step`], the client surface does **not** drive the
/// questionnaire runtime — that pointer is staff's progress toward the
/// post-intake workflow. The client's answers are written straight to the
/// `answers` table ([`record_client_answer`]); the latest answer per code
/// is what the document renders, so a client edit lands without disturbing
/// staff's walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientIntakeStep {
    /// The client should answer (or confirm) this question. `prior_value`
    /// pre-fills any current answer — including one staff entered on the
    /// client's behalf — so the client confirms rather than re-types.
    NeedsAnswer {
        question: QuestionDescriptor,
        prior_value: Option<String>,
        /// 1-based position among this notation's client-facing questions.
        position: usize,
        /// Count of client-facing questions on this notation.
        total: usize,
    },
    /// The client has answered every client-facing question; the rest is
    /// the firm's to finish.
    Complete { total: usize },
}

/// The ordered question codes a questionnaire walks, BEGIN → … → END
/// (following the unconditional `_` edge each step). The same ordering
/// the admin walker's progress indicator uses.
fn ordered_question_codes(spec: &QuestionnaireSpec) -> Vec<String> {
    let mut codes = Vec::new();
    let mut here = StateName::begin();
    while let Some(next) = spec
        .transitions_from(&here)
        .and_then(|t| t.lookup("_"))
        .cloned()
    {
        if next == StateName::end() {
            break;
        }
        codes.push(next.as_str().to_string());
        here = next;
    }
    codes
}

/// Resolve where the client is in their portion of `notation_id`'s
/// intake: the first client-facing question the client has not yet
/// answered (no `client`-sourced answer), pre-filled with any current
/// value, or [`ClientIntakeStep::Complete`] when the client has answered
/// them all. Save-per-step: a drop-off resumes at the first question
/// still missing a client answer.
pub async fn client_intake_step(
    db: &Db,
    storage: Option<&Arc<dyn StorageService>>,
    notation_id: Uuid,
) -> Result<ClientIntakeStep, NotationSessionError> {
    let (notation_row, definition) = load_notation_and_spec(db, storage, notation_id).await?;
    let person_id = notation_row.person_id;
    let locale = resolve_locale(db, person_id).await?;

    let codes = ordered_question_codes(&definition.spec);
    let canonical_codes: Vec<String> = codes
        .iter()
        .map(|code| question_code_for_state(code).to_string())
        .collect();
    let rows = question::Entity::find()
        .filter(question::Column::Code.is_in(canonical_codes))
        .all(db)
        .await?;
    let by_code: BTreeMap<String, question::Model> =
        rows.into_iter().map(|q| (q.code.clone(), q)).collect();
    let id_to_code: BTreeMap<Uuid, String> =
        by_code.values().map(|q| (q.id, q.code.clone())).collect();

    // Client-facing questions, in spec order.
    let client_codes: Vec<String> = codes
        .iter()
        .filter(|c| {
            by_code
                .get(question_code_for_state(c))
                .is_some_and(|q| store::entity::question::is_client_facing(&q.audience))
        })
        .cloned()
        .collect();
    let total = client_codes.len();

    // One pass over the respondent's answers: latest value per code (for
    // pre-fill) and the set of codes the client has answered themselves.
    let answers = answer::Entity::find()
        .filter(answer::Column::PersonId.eq(person_id))
        .order_by_asc(answer::Column::Id)
        .all(db)
        .await?;
    let mut latest_value: BTreeMap<String, String> = BTreeMap::new();
    let mut client_answer_counts: BTreeMap<String, usize> = BTreeMap::new();
    for a in answers {
        let Some(code) = id_to_code.get(&a.question_id) else {
            continue;
        };
        if a.source == answer::SOURCE_CLIENT {
            *client_answer_counts.entry(code.clone()).or_default() += 1;
        }
        latest_value.insert(code.clone(), answer::display_value(&a.value));
    }
    let client_answered = answered_client_states(&client_codes, client_answer_counts);

    for (idx, code) in client_codes.iter().enumerate() {
        if client_answered.contains(code) {
            continue;
        }
        let question = load_question(
            db,
            &StateName::from(code.as_str()),
            &locale,
            &definition.prompts,
        )
        .await?;
        return Ok(ClientIntakeStep::NeedsAnswer {
            question,
            prior_value: latest_value
                .get(code)
                .or_else(|| latest_value.get(question_code_for_state(code)))
                .cloned(),
            position: idx + 1,
            total,
        });
    }
    Ok(ClientIntakeStep::Complete { total })
}

/// Record one client-sourced answer to a client-facing question on
/// `notation_id`, without advancing the staff questionnaire runtime.
/// Rejects a question that is staff-only or outside the notation's
/// questionnaire so a hand-crafted POST can't write an arbitrary answer.
pub async fn record_client_answer(
    db: &Db,
    storage: Option<&Arc<dyn StorageService>>,
    notation_id: Uuid,
    question_code: &str,
    value: &str,
    authored_by: Uuid,
) -> Result<(), NotationSessionError> {
    let (notation_row, definition) = load_notation_and_spec(db, storage, notation_id).await?;
    if !ordered_question_codes(&definition.spec)
        .iter()
        .any(|c| c == question_code)
    {
        return Err(NotationSessionError::QuestionNotClientFacing(
            question_code.into(),
        ));
    }
    let canonical_code = question_code_for_state(question_code);
    let question_row = question::Entity::find()
        .filter(question::Column::Code.eq(canonical_code))
        .one(db)
        .await?
        .ok_or_else(|| NotationSessionError::QuestionNotSeeded(question_code.into()))?;
    if !store::entity::question::is_client_facing(&question_row.audience) {
        return Err(NotationSessionError::QuestionNotClientFacing(
            question_code.into(),
        ));
    }
    answer::ActiveModel {
        question_id: ActiveValue::Set(question_row.id),
        person_id: ActiveValue::Set(notation_row.person_id),
        notation_id: ActiveValue::Set(Some(notation_id)),
        state_name: ActiveValue::Set(Some(question_code.to_string())),
        value: ActiveValue::Set(answer::primitive(value)),
        source: ActiveValue::Set(answer::SOURCE_CLIENT.to_string()),
        authored_by_person_id: ActiveValue::Set(Some(authored_by)),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

async fn load_notation_and_spec(
    db: &Db,
    storage: Option<&Arc<dyn StorageService>>,
    notation_id: Uuid,
) -> Result<(notation::Model, QuestionnaireDefinition), NotationSessionError> {
    let notation_row = notation::Entity::find_by_id(notation_id)
        .one(db)
        .await?
        .ok_or(NotationSessionError::NotationNotFound(notation_id))?;

    // Resolve against the frozen snapshot, immune to later template/binary
    // changes. Only a Notation created before the snapshot column
    // (`questionnaire_snapshot IS NULL`) re-resolves from the template.
    if let Some(snapshot) = &notation_row.questionnaire_snapshot {
        let definition = serde_json::from_value(snapshot.clone())
            .map_err(|e| NotationSessionError::SnapshotDecode(e.to_string()))?;
        return Ok((notation_row, definition));
    }
    let template_row = template::Entity::find_by_id(notation_row.template_id)
        .one(db)
        .await?
        .ok_or(NotationSessionError::NotationNotFound(notation_id))?;
    let definition = questionnaire_definition_for(db, storage, &template_row).await?;
    Ok((notation_row, definition))
}

/// Resolve a template's questionnaire spec. Prefers the bundled
/// standalone YAML (compile-time `include_str!`); for a runtime-loaded
/// template not in the bundle, parses the spec from the template's
/// markdown body fetched from blob storage. A non-bundled template with
/// no body in storage (or no `storage` handle supplied) cannot drive a
/// questionnaire and surfaces
/// [`NotationSessionError::TemplateHasNoQuestionnaire`].
async fn questionnaire_definition_for(
    db: &Db,
    storage: Option<&Arc<dyn StorageService>>,
    template_row: &template::Model,
) -> Result<QuestionnaireDefinition, NotationSessionError> {
    if let Some(yaml) = bundled_spec_yaml(&template_row.code) {
        return Ok(QuestionnaireDefinition {
            spec: questionnaire_spec_from_yaml(yaml)?,
            prompts: prompt_overrides_from_yaml(yaml)?,
        });
    }
    let storage = storage.ok_or_else(|| {
        NotationSessionError::TemplateHasNoQuestionnaire(template_row.code.clone())
    })?;
    let body = store::templates::body(db, storage, template_row)
        .await
        .map_err(|_| NotationSessionError::TemplateHasNoQuestionnaire(template_row.code.clone()))?;
    Ok(QuestionnaireDefinition {
        spec: questionnaire_spec_from_template(&body)?,
        prompts: prompt_overrides_from_template(&body)?,
    })
}

async fn first_step(
    db: &Db,
    definition: &QuestionnaireDefinition,
    locale: &str,
) -> Result<NextStep, NotationSessionError> {
    next_step_from(db, definition, &StateName::begin(), locale).await
}

async fn next_step_from(
    db: &Db,
    definition: &QuestionnaireDefinition,
    current_state: &StateName,
    locale: &str,
) -> Result<NextStep, NotationSessionError> {
    let Some(next) = definition
        .spec
        .transitions_from(current_state)
        .and_then(|t| t.lookup("_"))
        .cloned()
    else {
        return Ok(NextStep::QuestionnaireComplete);
    };
    if next == StateName::end() {
        return Ok(NextStep::QuestionnaireComplete);
    }
    Ok(NextStep::NeedsAnswer {
        question: load_question(db, &next, locale, &definition.prompts).await?,
    })
}

/// The person's questionnaire locale (`persons.preferred_language`),
/// defaulting to `en` if the person row is somehow missing.
async fn resolve_locale(db: &Db, person_id: Uuid) -> Result<String, NotationSessionError> {
    Ok(person::Entity::find_by_id(person_id)
        .one(db)
        .await?
        .map_or_else(|| "en".to_string(), |p| p.preferred_language))
}

async fn load_question(
    db: &Db,
    state: &StateName,
    locale: &str,
    prompts: &BTreeMap<String, String>,
) -> Result<QuestionDescriptor, NotationSessionError> {
    let code = question_code_for_state(state.as_str());
    let row = question::Entity::find()
        .filter(question::Column::Code.eq(code))
        .one(db)
        .await?
        .ok_or_else(|| NotationSessionError::QuestionNotSeeded(state.0.clone()))?;
    let prompt = if let Some(prompt) = prompt_override_for_state(prompts, state.as_str()) {
        prompt.to_string()
    } else {
        localize_prompt_for_state(
            &localized_prompt(db, row.id, &row.prompt, locale).await?,
            state.as_str(),
        )
    };
    Ok(QuestionDescriptor {
        id: row.id,
        code: state.0.clone(),
        prompt,
        answer_type: row.answer_type,
    })
}

fn question_code_for_state(state: &str) -> &str {
    state.split_once("__").map_or(state, |(code, _)| code)
}

fn prompt_override_for_state<'a>(
    prompts: &'a BTreeMap<String, String>,
    state: &str,
) -> Option<&'a str> {
    let (prefix, prompt_key) = state.split_once("__")?;
    if prefix.starts_with("custom_") {
        prompts.get(prompt_key).map(String::as_str)
    } else {
        None
    }
}

fn answered_client_states(
    client_codes: &[String],
    mut answer_counts_by_canonical_code: BTreeMap<String, usize>,
) -> std::collections::BTreeSet<String> {
    let mut answered = std::collections::BTreeSet::new();
    for code in client_codes {
        let canonical_code = question_code_for_state(code);
        let Some(remaining) = answer_counts_by_canonical_code.get_mut(canonical_code) else {
            continue;
        };
        if *remaining == 0 {
            continue;
        }
        answered.insert(code.clone());
        *remaining -= 1;
    }
    answered
}

fn localize_prompt_for_state(prompt: &str, state: &str) -> String {
    let label = state
        .split_once("__")
        .map_or(state, |(_, label)| label)
        .replace('_', " ");
    prompt
        .replace("{{for_label}}", &label)
        .replace("{label}", &label)
}

/// Resolve the prompt for `locale`: the attorney-reviewed
/// `question_translations` variant when one exists, else the English
/// base `questions.prompt`. `en` short-circuits to the base.
async fn localized_prompt(
    db: &Db,
    question_id: Uuid,
    base: &str,
    locale: &str,
) -> Result<String, NotationSessionError> {
    if locale == "en" {
        return Ok(base.to_string());
    }
    let translated = question_translation::Entity::find()
        .filter(question_translation::Column::QuestionId.eq(question_id))
        .filter(question_translation::Column::Locale.eq(locale))
        .one(db)
        .await?
        .map(|t| t.prompt);
    Ok(translated.unwrap_or_else(|| base.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        answer_step, answered_client_states, current_step, start_notation, AnswerAuthor, NextStep,
        NotationSessionError, QuestionDescriptor, QuestionnaireDefinition,
    };
    use crate::questionnaire_spec_from_yaml;
    use crate::runtime::InMemoryRuntime;
    use sea_orm::{
        ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    };
    use std::collections::BTreeMap;
    use store::entity::answer::{SOURCE_CLIENT, SOURCE_STAFF};
    use store::entity::{
        answer, notation, person, project, question, question_translation, template,
    };
    use uuid::Uuid;

    async fn db() -> store::Db {
        store::test_support::pg().await
    }

    async fn seed_person(db: &store::Db, email: &str) -> Uuid {
        person::ActiveModel {
            name: ActiveValue::Set(email.into()),
            email: ActiveValue::Set(email.into()),
            role: ActiveValue::Set(store::entity::person::Role::Client),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_project(db: &store::Db) -> Uuid {
        let __dri = store::test_support::dri_person(db).await;
        project::ActiveModel {
            name: ActiveValue::Set("test project".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(store::test_support::seed_entity(db).await),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_retainer_template(db: &store::Db) {
        // The retainer template body is bundled via include_str!;
        // for tests we only need the row to exist with the
        // matching `code` so the spec lookup hits the bundled YAML.
        seed_template(db, "onboarding__retainer", "Retainer").await;
    }

    async fn seed_template(db: &store::Db, code: &str, title: &str) {
        template::ActiveModel {
            code: ActiveValue::Set(code.into()),
            title: ActiveValue::Set(title.into()),
            respondent_type: ActiveValue::Set("person".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn seed_question(db: &store::Db, code: &str) {
        question::ActiveModel {
            code: ActiveValue::Set(code.into()),
            prompt: ActiveValue::Set(format!("Prompt for {code}")),
            answer_type: ActiveValue::Set("string".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn seed_retainer_questions(db: &store::Db) {
        for code in [
            "client_name",
            "client_email",
            "project_name",
            "product_description",
        ] {
            seed_question(db, code).await;
        }
    }

    async fn seed_question_with_audience(db: &store::Db, code: &str, audience: &str) {
        question::ActiveModel {
            code: ActiveValue::Set(code.into()),
            prompt: ActiveValue::Set(format!("Prompt for {code}")),
            answer_type: ActiveValue::Set("string".into()),
            audience: ActiveValue::Set(audience.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    /// Seed the retainer questions with the shipped audiences:
    /// client_name/email are `both` (the client confirms), project_name /
    /// product_description are `staff`.
    async fn seed_retainer_questions_with_audiences(db: &store::Db) {
        use store::entity::question::{AUDIENCE_BOTH, AUDIENCE_STAFF};
        seed_question_with_audience(db, "client_name", AUDIENCE_BOTH).await;
        seed_question_with_audience(db, "client_email", AUDIENCE_BOTH).await;
        seed_question_with_audience(db, "project_name", AUDIENCE_STAFF).await;
        seed_question_with_audience(db, "product_description", AUDIENCE_STAFF).await;
    }

    #[tokio::test]
    async fn start_notation_creates_row_and_returns_first_question() {
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        let person_id = seed_person(&db, "libra@example.com").await;
        let runtime = InMemoryRuntime::new();

        let outcome = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();

        // Notation row exists, linked to the right person.
        let row = notation::Entity::find_by_id(outcome.notation_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.person_id, person_id);
        assert_eq!(row.entity_id, None);
        assert_eq!(row.state, "BEGIN");

        // First question per retainer questionnaire is client_name.
        match outcome.next {
            NextStep::NeedsAnswer {
                question: QuestionDescriptor { code, .. },
            } => {
                assert_eq!(code, "client_name");
            }
            NextStep::QuestionnaireComplete => {
                panic!("expected NeedsAnswer, got QuestionnaireComplete")
            }
        }
    }

    #[tokio::test]
    async fn start_notation_freezes_the_questionnaire_snapshot() {
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        let person_id = seed_person(&db, "libra@example.com").await;
        let runtime = InMemoryRuntime::new();

        let outcome = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();
        let nid = outcome.notation_id;

        // The snapshot is written at creation.
        let row = notation::Entity::find_by_id(nid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(
            row.questionnaire_snapshot.is_some(),
            "start_notation must freeze the askable set"
        );

        // Overwrite the snapshot with a *different* questionnaire that
        // starts at client_email. The template's bundled spec still starts
        // at client_name, so if resolution re-read the template it would
        // ask client_name; reading the frozen snapshot asks client_email.
        let alt = QuestionnaireDefinition {
            spec: questionnaire_spec_from_yaml(
                "questionnaire:\n  BEGIN:\n    _: client_email\n  \
                 client_email:\n    _: END\n  END: {}\n",
            )
            .unwrap(),
            prompts: BTreeMap::new(),
        };
        let mut active: notation::ActiveModel = row.into();
        active.questionnaire_snapshot = ActiveValue::Set(Some(serde_json::to_value(&alt).unwrap()));
        active.update(&db).await.unwrap();

        let next = current_step(&db, &runtime, None, nid).await.unwrap();
        match next {
            NextStep::NeedsAnswer { question } => assert_eq!(
                question.code, "client_email",
                "resolution must read the frozen snapshot, not the template"
            ),
            NextStep::QuestionnaireComplete => panic!("expected NeedsAnswer"),
        }
    }

    async fn seed_spanish_person(db: &store::Db, email: &str) -> Uuid {
        person::ActiveModel {
            name: ActiveValue::Set(email.into()),
            email: ActiveValue::Set(email.into()),
            role: ActiveValue::Set(store::entity::person::Role::Client),
            preferred_language: ActiveValue::Set("es".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn add_translation(db: &store::Db, code: &str, locale: &str, prompt: &str) {
        let q = question::Entity::find()
            .filter(question::Column::Code.eq(code))
            .one(db)
            .await
            .unwrap()
            .unwrap();
        question_translation::ActiveModel {
            question_id: ActiveValue::Set(q.id),
            locale: ActiveValue::Set(locale.into()),
            prompt: ActiveValue::Set(prompt.into()),
            help_text: ActiveValue::Set(None),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    fn prompt_of(next: &NextStep) -> &str {
        match next {
            NextStep::NeedsAnswer { question } => question.prompt.as_str(),
            NextStep::QuestionnaireComplete => panic!("expected NeedsAnswer"),
        }
    }

    #[tokio::test]
    async fn start_notation_renders_prompt_in_persons_preferred_language() {
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        add_translation(
            &db,
            "client_name",
            "es",
            "¿Cuál es el nombre legal completo del cliente?",
        )
        .await;
        let person_id = seed_spanish_person(&db, "gemini@example.com").await;
        let runtime = InMemoryRuntime::new();

        let outcome = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            prompt_of(&outcome.next),
            "¿Cuál es el nombre legal completo del cliente?"
        );
    }

    #[tokio::test]
    async fn missing_translation_falls_back_to_english_base_prompt() {
        // Spanish person, but no `es` translation seeded for this
        // question → the English base prompt is returned, never an
        // error and never a blank.
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        let person_id = seed_spanish_person(&db, "gemini@example.com").await;
        let runtime = InMemoryRuntime::new();

        let outcome = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();
        // seed_question sets the base prompt to "Prompt for client_name".
        assert_eq!(prompt_of(&outcome.next), "Prompt for client_name");
    }

    #[tokio::test]
    async fn custom_question_uses_template_prompt_override() {
        let db = db().await;
        seed_template(&db, "nv__dissolution", "Dissolution").await;
        seed_question(&db, "custom_text").await;
        let person_id = seed_person(&db, "libra@example.com").await;
        let runtime = InMemoryRuntime::new();

        let outcome = start_notation(
            &db,
            &runtime,
            None,
            "nv__dissolution",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();

        assert_eq!(prompt_of(&outcome.next), "What is the dissolution reason?");
    }

    #[tokio::test]
    async fn answer_step_keeps_rendering_in_the_persons_language() {
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        add_translation(&db, "client_email", "es", "¿Cuál es el correo del cliente?").await;
        let person_id = seed_spanish_person(&db, "gemini@example.com").await;
        let runtime = InMemoryRuntime::new();
        let started = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();
        let next = answer_step(
            &db,
            &runtime,
            None,
            started.notation_id,
            "client_name",
            "Gemini",
            AnswerAuthor::staff(None),
        )
        .await
        .unwrap();
        assert_eq!(prompt_of(&next), "¿Cuál es el correo del cliente?");
    }

    #[tokio::test]
    async fn start_notation_unknown_template_is_template_not_found() {
        let db = db().await;
        let person_id = seed_person(&db, "libra@example.com").await;
        let runtime = InMemoryRuntime::new();
        let err = start_notation(
            &db,
            &runtime,
            None,
            "does_not_exist",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap_err();
        match err {
            NotationSessionError::TemplateNotFound(c) => assert_eq!(c, "does_not_exist"),
            other => panic!("expected TemplateNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn answer_step_walks_to_next_question() {
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        let person_id = seed_person(&db, "libra@example.com").await;
        let runtime = InMemoryRuntime::new();

        let started = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();
        let id = started.notation_id;

        let next = answer_step(
            &db,
            &runtime,
            None,
            id,
            "client_name",
            "Libra",
            AnswerAuthor::staff(None),
        )
        .await
        .unwrap();
        match next {
            NextStep::NeedsAnswer { question } => {
                assert_eq!(question.code, "client_email");
            }
            NextStep::QuestionnaireComplete => {
                panic!("expected NeedsAnswer(client_email), got QuestionnaireComplete");
            }
        }
    }

    #[tokio::test]
    async fn answer_step_with_wrong_code_returns_mismatch() {
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        let person_id = seed_person(&db, "libra@example.com").await;
        let runtime = InMemoryRuntime::new();
        let started = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();

        let err = answer_step(
            &db,
            &runtime,
            None,
            started.notation_id,
            "project_name",
            "anything",
            AnswerAuthor::staff(None),
        )
        .await
        .unwrap_err();
        match err {
            NotationSessionError::QuestionMismatch { expected, got } => {
                assert_eq!(expected, "client_name");
                assert_eq!(got, "project_name");
            }
            other => panic!("expected QuestionMismatch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn full_walk_ends_at_questionnaire_complete() {
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        let person_id = seed_person(&db, "libra@example.com").await;
        let runtime = InMemoryRuntime::new();
        let started = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();
        let id = started.notation_id;

        let walk = [
            ("client_name", "Libra"),
            ("client_email", "libra@example.com"),
            ("project_name", "Apollo"),
            ("product_description", "rocket"),
        ];
        let mut last = NextStep::QuestionnaireComplete;
        for (i, (code, value)) in walk.iter().enumerate() {
            last = answer_step(
                &db,
                &runtime,
                None,
                id,
                code,
                value,
                AnswerAuthor::staff(None),
            )
            .await
            .unwrap();
            if i < walk.len() - 1 {
                let expected_next = walk[i + 1].0;
                match &last {
                    NextStep::NeedsAnswer { question } => {
                        assert_eq!(question.code, expected_next);
                    }
                    NextStep::QuestionnaireComplete => {
                        panic!("step {i}: expected NeedsAnswer, got QuestionnaireComplete");
                    }
                }
            }
        }
        assert!(matches!(last, NextStep::QuestionnaireComplete));
    }

    #[tokio::test]
    async fn answering_after_complete_is_already_complete() {
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        let person_id = seed_person(&db, "libra@example.com").await;
        let runtime = InMemoryRuntime::new();
        let started = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();
        let id = started.notation_id;
        for (code, value) in [
            ("client_name", "Libra"),
            ("client_email", "libra@example.com"),
            ("project_name", "Apollo"),
            ("product_description", "rocket"),
        ] {
            answer_step(
                &db,
                &runtime,
                None,
                id,
                code,
                value,
                AnswerAuthor::staff(None),
            )
            .await
            .unwrap();
        }
        // One more should fail.
        let err = answer_step(
            &db,
            &runtime,
            None,
            id,
            "client_name",
            "again",
            AnswerAuthor::staff(None),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, NotationSessionError::AlreadyComplete));
    }

    #[tokio::test]
    async fn current_step_reports_the_question_about_to_be_asked() {
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        let person_id = seed_person(&db, "libra@example.com").await;
        let runtime = InMemoryRuntime::new();
        let started = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();
        let id = started.notation_id;
        // Before any answer: should be client_name.
        match current_step(&db, &runtime, None, id).await.unwrap() {
            NextStep::NeedsAnswer { question } => assert_eq!(question.code, "client_name"),
            NextStep::QuestionnaireComplete => {
                panic!("expected NeedsAnswer(client_name), got QuestionnaireComplete");
            }
        }
        answer_step(
            &db,
            &runtime,
            None,
            id,
            "client_name",
            "Libra",
            AnswerAuthor::staff(None),
        )
        .await
        .unwrap();
        // After one answer: should be client_email.
        match current_step(&db, &runtime, None, id).await.unwrap() {
            NextStep::NeedsAnswer { question } => assert_eq!(question.code, "client_email"),
            NextStep::QuestionnaireComplete => {
                panic!("expected NeedsAnswer(client_email), got QuestionnaireComplete");
            }
        }
    }

    #[tokio::test]
    async fn current_step_for_unknown_notation_is_notation_not_found() {
        let db = db().await;
        let runtime = InMemoryRuntime::new();
        let err = current_step(&db, &runtime, None, Uuid::nil())
            .await
            .unwrap_err();
        assert!(matches!(err, NotationSessionError::NotationNotFound(_)));
    }

    #[tokio::test]
    async fn answer_step_persists_the_answer_row() {
        use sea_orm::{ColumnTrait, QueryFilter};
        use store::entity::answer;
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        let person_id = seed_person(&db, "libra@example.com").await;
        let runtime = InMemoryRuntime::new();
        let started = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();
        // The client self-answered this one through the magic link, so
        // the row must record both the source and the typist.
        answer_step(
            &db,
            &runtime,
            None,
            started.notation_id,
            "client_name",
            "Libra",
            AnswerAuthor::client(Some(person_id)),
        )
        .await
        .unwrap();

        let q = question::Entity::find()
            .filter(question::Column::Code.eq("client_name"))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let rows = answer::Entity::find()
            .filter(answer::Column::QuestionId.eq(q.id))
            .filter(answer::Column::PersonId.eq(person_id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(answer::display_value(&rows[0].value), "Libra");
        assert_eq!(rows[0].notation_id, Some(started.notation_id));
        assert_eq!(rows[0].state_name.as_deref(), Some("client_name"));
        // person_id is the respondent; source + authored_by record who
        // actually entered it.
        assert_eq!(rows[0].source, SOURCE_CLIENT);
        assert_eq!(rows[0].authored_by_person_id, Some(person_id));
    }

    #[tokio::test]
    async fn staff_entered_answer_records_staff_source() {
        use sea_orm::{ColumnTrait, QueryFilter};
        use store::entity::answer;
        let db = db().await;
        seed_retainer_template(&db).await;
        seed_retainer_questions(&db).await;
        let client_id = seed_person(&db, "libra@example.com").await;
        let staff_id = seed_person(&db, "staff@neonlaw.com").await;
        let runtime = InMemoryRuntime::new();
        let started = start_notation(
            &db,
            &runtime,
            None,
            "onboarding__retainer",
            client_id,
            seed_project(&db).await,
            None,
        )
        .await
        .unwrap();
        // Staff types the client's answer on their behalf: the respondent
        // is the client, the typist is staff, the source is `staff`.
        answer_step(
            &db,
            &runtime,
            None,
            started.notation_id,
            "client_name",
            "Libra",
            AnswerAuthor::staff(Some(staff_id)),
        )
        .await
        .unwrap();
        let row = answer::Entity::find()
            .filter(answer::Column::PersonId.eq(client_id))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.person_id, client_id);
        assert_eq!(row.source, SOURCE_STAFF);
        assert_eq!(row.authored_by_person_id, Some(staff_id));
    }

    use super::{client_intake_step, record_client_answer, ClientIntakeStep};

    /// Start a retainer notation whose questions carry the shipped
    /// audiences, returning `(notation_id, respondent_id)`.
    async fn start_audienced_retainer(db: &store::Db, runtime: &InMemoryRuntime) -> (Uuid, Uuid) {
        seed_retainer_template(db).await;
        seed_retainer_questions_with_audiences(db).await;
        let person_id = seed_person(db, "libra@example.com").await;
        let started = start_notation(
            db,
            runtime,
            None,
            "onboarding__retainer",
            person_id,
            seed_project(db).await,
            None,
        )
        .await
        .unwrap();
        (started.notation_id, person_id)
    }

    #[tokio::test]
    async fn client_intake_walks_only_client_facing_questions() {
        let db = db().await;
        let runtime = InMemoryRuntime::new();
        let (id, person) = start_audienced_retainer(&db, &runtime).await;

        // Only the two client-facing questions are offered, in order.
        let step = client_intake_step(&db, None, id).await.unwrap();
        let ClientIntakeStep::NeedsAnswer {
            question,
            position,
            total,
            ..
        } = step
        else {
            panic!("expected NeedsAnswer(client_name)");
        };
        assert_eq!(question.code, "client_name");
        assert_eq!((position, total), (1, 2));

        record_client_answer(&db, None, id, "client_name", "Libra", person)
            .await
            .unwrap();
        let step = client_intake_step(&db, None, id).await.unwrap();
        let ClientIntakeStep::NeedsAnswer {
            question, position, ..
        } = step
        else {
            panic!("expected NeedsAnswer(client_email)");
        };
        assert_eq!(question.code, "client_email");
        assert_eq!(position, 2);

        record_client_answer(&db, None, id, "client_email", "libra@example.com", person)
            .await
            .unwrap();
        // The staff-only project_name / product_description are never
        // offered to the client; their part is done.
        assert!(matches!(
            client_intake_step(&db, None, id).await.unwrap(),
            ClientIntakeStep::Complete { total: 2 }
        ));
    }

    #[tokio::test]
    async fn staff_prefilled_answer_shows_and_is_editable() {
        let db = db().await;
        let runtime = InMemoryRuntime::new();
        let (id, person) = start_audienced_retainer(&db, &runtime).await;

        // Staff fills client_name on the client's behalf first.
        answer_step(
            &db,
            &runtime,
            None,
            id,
            "client_name",
            "Staff-typed Libra",
            AnswerAuthor::staff(None),
        )
        .await
        .unwrap();

        // The client sees that staff answer pre-filled and editable —
        // client_name is still *their* step because they haven't answered
        // it themselves yet.
        let step = client_intake_step(&db, None, id).await.unwrap();
        let ClientIntakeStep::NeedsAnswer {
            question,
            prior_value,
            ..
        } = step
        else {
            panic!("expected NeedsAnswer(client_name) pre-filled");
        };
        assert_eq!(question.code, "client_name");
        assert_eq!(prior_value.as_deref(), Some("Staff-typed Libra"));

        // The client corrects it; the latest answer (client-sourced) wins.
        record_client_answer(&db, None, id, "client_name", "Libra Prime", person)
            .await
            .unwrap();
        let latest = answer::Entity::find()
            .filter(answer::Column::PersonId.eq(person))
            .order_by_desc(answer::Column::Id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(answer::display_value(&latest.value), "Libra Prime");
        assert_eq!(latest.notation_id, Some(id));
        assert_eq!(latest.source, SOURCE_CLIENT);
    }

    #[tokio::test]
    async fn record_client_answer_rejects_staff_only_question() {
        let db = db().await;
        let runtime = InMemoryRuntime::new();
        let (id, person) = start_audienced_retainer(&db, &runtime).await;
        let err = record_client_answer(&db, None, id, "project_name", "sneaky", person)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            NotationSessionError::QuestionNotClientFacing(c) if c == "project_name"
        ));
    }

    #[test]
    fn answered_client_states_do_not_collapse_duplicate_typed_prefixes() {
        let codes = vec![
            "custom_text__mission_statement".to_string(),
            "custom_text__revenue_strategy".to_string(),
        ];
        let answered = answered_client_states(&codes, BTreeMap::from([("custom_text".into(), 1)]));

        assert!(answered.contains("custom_text__mission_statement"));
        assert!(!answered.contains("custom_text__revenue_strategy"));
    }
}
