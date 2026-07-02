//! Project (matter) lifecycle helpers.
//!
//! A matter's `status` walks `open` → `closed` → `archived`
//! (`entity::project::Model::status`). Opening is done at retainer
//! intake; this module owns the *close* — flipping a matter to `closed`
//! when the firm signs its closing letter. Archival (the Drive cold
//! store) is a separate downstream step and is left untouched here.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};
use uuid::Uuid;

use crate::entity::{notation, project};
use crate::Db;

/// Client-facing message when a matter cannot be opened because its git
/// repository is not ready. Keep this deliberately plain: the client/staff
/// action is to retry or ask support, not to reason about git.
pub const REPO_PROVISIONING_FAILURE_MESSAGE: &str =
    "We couldn't open this matter because its secure document workspace was not ready. Please try again in a moment.";

/// How long matter creation waits for the bare repo to become ready before
/// rolling back the database transaction.
pub const REPO_PROVISIONING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Env var naming the base URL of the **single mounted writer** — the `web`
/// deployment that mounts the repo volume (`git-serving` in prod). Processes
/// without the volume provision matter repos through it.
pub const GIT_WRITER_URL_ENV: &str = "NAVIGATOR_GIT_WRITER_URL";

/// Env var carrying the bearer token the writer's ensure endpoint requires.
/// Both sides read it from the same secret (`navigator-web-secrets` in prod).
pub const GIT_WRITER_TOKEN_ENV: &str = "NAVIGATOR_GIT_WRITER_TOKEN";

/// Path of the repo-ensure endpoint the writer serves (`web::git_writer`).
pub const GIT_WRITER_ENSURE_PATH: &str = "/git-writer/ensure";

#[derive(Debug, thiserror::Error)]
pub enum ProvisionRepoError {
    #[error(transparent)]
    Repo(#[from] repos::RepoError),
    #[error("project {project_id} was not found while provisioning git repo")]
    NotFound { project_id: Uuid },
    #[error("timed out provisioning git repo for project {project_id}")]
    Timeout { project_id: Uuid },
    #[error(
        "git repo provisioning is not configured: set {} (this process mounts the repo volume) \
         or {GIT_WRITER_URL_ENV} + {GIT_WRITER_TOKEN_ENV} (provision through the single mounted \
         writer)",
        repos::REPO_ROOT_ENV
    )]
    NotConfigured,
    #[error("git writer request failed: {0}")]
    RemoteWriter(String),
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
}

/// Wire request of the writer's ensure endpoint. Carries only the project id
/// — never client content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnsureRepoRequest {
    pub project_id: Uuid,
}

/// Wire response of the writer's ensure endpoint: the bare repo's path on
/// the writer's volume (informational — callers treat success as the signal).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnsureRepoResponse {
    pub path: String,
}

/// Where a matter's bare repo gets created — the env-selected seam between
/// "this process mounts the repo volume" and "this process asks the single
/// mounted writer to create it" (docs/git-project-repos.md §6), the same
/// shape as `cloud::StorageService`'s backend selection.
///
/// Exactly one deployment per environment mounts the RWO repo volume
/// (`git-serving` in prod; the sole in-cluster `web` pod in KIND; host-side
/// `web` and the CLI in dev). That process resolves `Local` from
/// [`repos::REPO_ROOT_ENV`] and runs `git init` itself. Every other process
/// resolves `Remote` and drives the writer's [`GIT_WRITER_ENSURE_PATH`]
/// endpoint — the filesystem half only. In **both** variants the
/// `git_initialized_at` stamp happens on the caller's own open transaction,
/// so a rollback can never commit a Project row without its repo.
#[derive(Clone, Debug)]
pub enum RepoEnsurer {
    /// This process mounts the repo volume; create the bare repo in-process.
    Local(repos::RepoStore),
    /// Ask the single mounted writer over HTTP.
    Remote(RemoteWriter),
}

/// HTTP client half of [`RepoEnsurer::Remote`].
#[derive(Clone, Debug)]
pub struct RemoteWriter {
    base_url: String,
    token: String,
    client: reqwest::Client,
}

impl RemoteWriter {
    /// Build a writer client from its base URL + shared bearer token.
    #[must_use]
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: token.into(),
            client: reqwest::Client::new(),
        }
    }

    async fn ensure(&self, project_id: Uuid) -> Result<std::path::PathBuf, ProvisionRepoError> {
        let url = format!(
            "{}{GIT_WRITER_ENSURE_PATH}",
            self.base_url.trim_end_matches('/')
        );
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&EnsureRepoRequest { project_id })
            .send()
            .await
            .map_err(|e| ProvisionRepoError::RemoteWriter(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ProvisionRepoError::RemoteWriter(format!(
                "{} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        let body: EnsureRepoResponse = resp
            .json()
            .await
            .map_err(|e| ProvisionRepoError::RemoteWriter(e.to_string()))?;
        Ok(std::path::PathBuf::from(body.path))
    }
}

impl RepoEnsurer {
    /// Resolve the ensurer from the process environment. [`RepoEnsurer::Local`]
    /// wins when [`repos::REPO_ROOT_ENV`] is set (a process with the volume
    /// never needs the network); otherwise [`GIT_WRITER_URL_ENV`] +
    /// [`GIT_WRITER_TOKEN_ENV`] select [`RepoEnsurer::Remote`].
    ///
    /// # Errors
    /// [`ProvisionRepoError::NotConfigured`] when neither is set — matter
    /// creation surfaces turn this into the shared 503 +
    /// [`REPO_PROVISIONING_FAILURE_MESSAGE`].
    pub fn from_env() -> Result<Self, ProvisionRepoError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// [`RepoEnsurer::from_env`] against any `key -> Option<value>` lookup —
    /// the seam tests use to avoid mutating process env vars.
    ///
    /// # Errors
    /// See [`RepoEnsurer::from_env`].
    pub fn from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> Result<Self, ProvisionRepoError> {
        if let Some(root) = get(repos::REPO_ROOT_ENV).filter(|s| !s.is_empty()) {
            return Ok(Self::Local(repos::RepoStore::new(root)));
        }
        let url = get(GIT_WRITER_URL_ENV).filter(|s| !s.is_empty());
        let token = get(GIT_WRITER_TOKEN_ENV).filter(|s| !s.is_empty());
        match (url, token) {
            (Some(url), Some(token)) => Ok(Self::Remote(RemoteWriter::new(url, token))),
            _ => Err(ProvisionRepoError::NotConfigured),
        }
    }

    /// Create the bare repo for `project_id` (idempotent), locally or through
    /// the mounted writer. Filesystem only — the caller stamps
    /// `git_initialized_at` on its own transaction.
    ///
    /// # Errors
    /// [`ProvisionRepoError::Repo`] for local git/filesystem failures,
    /// [`ProvisionRepoError::RemoteWriter`] for writer transport/HTTP
    /// failures.
    pub async fn ensure(&self, project_id: Uuid) -> Result<std::path::PathBuf, ProvisionRepoError> {
        match self {
            // `ensure` shells to `git` (blocking); keep it off the async runtime.
            Self::Local(store) => {
                let store = store.clone();
                tokio::task::spawn_blocking(move || store.ensure(project_id))
                    .await
                    .map_err(|e| repos::RepoError::Io(std::io::Error::other(e.to_string())))?
                    .map_err(Into::into)
            }
            Self::Remote(writer) => writer.ensure(project_id).await,
        }
    }
}

/// The notation id of the person's **sole open matter**, for auto-routing an
/// inbound message to a matter without manual triage. Returns `Some` only
/// when the person is the client (`notations.person_id`) on exactly one
/// matter whose project is still `open`; `None` when they have none, or more
/// than one (the ambiguous case — fall back to manual `@link`).
///
/// This is the seam the email loop uses so a known client's reply lands on
/// their matter's conversation log on its own.
///
/// # Errors
///
/// Propagates any database error.
pub async fn sole_open_matter_for_person(
    db: &Db,
    person_id: Uuid,
) -> Result<Option<Uuid>, sea_orm::DbErr> {
    let notations = notation::Entity::find()
        .filter(notation::Column::PersonId.eq(person_id))
        .all(db)
        .await?;

    let mut open: Vec<Uuid> = Vec::new();
    for n in notations {
        if let Some(p) = project::Entity::find_by_id(n.project_id).one(db).await? {
            if p.status == "open" {
                open.push(n.id);
            }
        }
    }
    Ok((open.len() == 1).then(|| open[0]))
}

/// Flip the matter that `notation_id` belongs to from `open` to
/// `closed`. Returns the closed project's id, or `None` if the notation
/// (or its project) no longer exists.
///
/// Idempotent and monotonic: a matter already `closed` or `archived` is
/// left as-is — re-running never re-opens it, and a replay of the
/// firm-signature side effect is a no-op. `inserted_at`/`updated_at` are
/// maintained by the entity's active-model behavior.
pub async fn close_for_notation(
    db: &Db,
    notation_id: Uuid,
) -> Result<Option<Uuid>, sea_orm::DbErr> {
    let Some(n) = notation::Entity::find_by_id(notation_id).one(db).await? else {
        return Ok(None);
    };
    let Some(p) = project::Entity::find_by_id(n.project_id).one(db).await? else {
        return Ok(None);
    };
    let project_id = p.id;
    // Monotonic: don't walk backwards out of `archived`, and don't
    // churn an already-`closed` row.
    if p.status == "closed" || p.status == "archived" {
        return Ok(Some(project_id));
    }
    let mut active: project::ActiveModel = p.into();
    active.status = ActiveValue::Set("closed".into());
    // Stamp the close date — the start of the 10-year retention window.
    active.closed_at = ActiveValue::Set(Some(chrono::Utc::now().to_rfc3339()));
    active.update(db).await?;
    Ok(Some(project_id))
}

/// Stamp `git_initialized_at` the first time a matter's bare repo is
/// created. Returns the effective timestamp (`Some`), or `None` if the
/// Project no longer exists.
///
/// Idempotent and monotonic, enforced *atomically*: the write is a single
/// conditional `UPDATE ... WHERE git_initialized_at IS NULL`, so only the
/// first writer sets the column and a concurrent provisioning replay can
/// never overwrite the original first-creation timestamp. An already-stamped
/// Project is left untouched and its existing stamp is returned. Because the
/// bulk update bypasses the entity's active-model behavior, `updated_at` is
/// bumped in the same statement.
///
/// # Errors
///
/// Propagates any database error.
pub async fn mark_git_initialized<C>(
    db: &C,
    project_id: Uuid,
    when: chrono::DateTime<chrono::Utc>,
) -> Result<Option<String>, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    use sea_orm::sea_query::Expr;

    let stamp = when.to_rfc3339();
    let res = project::Entity::update_many()
        .col_expr(
            project::Column::GitInitializedAt,
            Expr::value(stamp.clone()),
        )
        .col_expr(project::Column::UpdatedAt, Expr::value(stamp.clone()))
        .filter(project::Column::Id.eq(project_id))
        .filter(project::Column::GitInitializedAt.is_null())
        .exec(db)
        .await?;
    if res.rows_affected == 1 {
        return Ok(Some(stamp));
    }
    // No row stamped: the Project is either already stamped (return its
    // existing first-creation timestamp) or gone (`None`).
    Ok(project::Entity::find_by_id(project_id)
        .one(db)
        .await?
        .and_then(|p| p.git_initialized_at))
}

/// Provision a Project's append-only bare git repo and stamp
/// `git_initialized_at` — the lazy half of the provisioning story, used by
/// the git smart-HTTP transport (first authorized clone/push of a row whose
/// repo predates hard provisioning). The bare repo is created by
/// [`repos::RepoStore::ensure`] (no second `git init`) and the column is
/// stamped by [`mark_git_initialized`]; both halves are idempotent, so a
/// repeat call is a no-op that preserves the original first-creation
/// timestamp.
///
/// Returns the bare repo's on-disk path. The repo lives on the git volume
/// named by [`repos::REPO_ROOT_ENV`]; [`repos::RepoError::RootUnset`]
/// surfaces when that volume is not configured (a CLI/seed/remote-DB
/// context with no git mount).
///
/// # Errors
/// [`repos::RepoError`] when the git volume is unconfigured or a `git`
/// invocation fails. A failure to *stamp* the column is logged, not
/// returned — the repo is what the caller needs, and the stamp reconciles
/// on the next call.
pub async fn provision_repo(
    db: &Db,
    project_id: Uuid,
) -> Result<std::path::PathBuf, repos::RepoError> {
    provision_repo_in(db, repos::RepoStore::from_env()?, project_id).await
}

/// [`provision_repo`] against an explicit [`repos::RepoStore`] — the seam
/// tests use to root the repo in a temp dir without touching the
/// process-global env var.
async fn provision_repo_in(
    db: &Db,
    store: repos::RepoStore,
    project_id: Uuid,
) -> Result<std::path::PathBuf, repos::RepoError> {
    // `ensure` shells to `git` (blocking); keep it off the async runtime.
    let path = tokio::task::spawn_blocking(move || store.ensure(project_id))
        .await
        .map_err(|e| repos::RepoError::Io(std::io::Error::other(e.to_string())))??;
    if let Err(e) = mark_git_initialized(db, project_id, chrono::Utc::now()).await {
        tracing::error!(error = %e, %project_id, "provision_repo: stamping git_initialized_at failed");
    }
    Ok(path)
}

/// Provision a Project repo as a hard dependency of matter creation.
///
/// Call this while the surrounding create transaction is still open. If repo
/// creation or the `git_initialized_at` stamp fails, return the error so the
/// caller can roll the transaction back and avoid committing a Project row
/// whose document workspace is missing.
///
/// # Errors
/// Returns [`ProvisionRepoError`] when repo provisioning is not configured,
/// git / filesystem / writer-transport setup fails, the timeout elapses, or
/// the stamp write fails.
pub async fn provision_repo_hard<C>(
    db: &C,
    ensurer: &RepoEnsurer,
    project_id: Uuid,
    timeout: std::time::Duration,
) -> Result<std::path::PathBuf, ProvisionRepoError>
where
    C: ConnectionTrait,
{
    let path = tokio::time::timeout(timeout, ensurer.ensure(project_id))
        .await
        .map_err(|_| ProvisionRepoError::Timeout { project_id })??;
    if mark_git_initialized(db, project_id, chrono::Utc::now())
        .await?
        .is_none()
    {
        return Err(ProvisionRepoError::NotFound { project_id });
    }
    Ok(path)
}

/// [`provision_repo_hard`] using the env-resolved [`RepoEnsurer`] and
/// workspace timeout.
///
/// # Errors
/// See [`provision_repo_hard`].
pub async fn provision_repo_hard_from_env<C>(
    db: &C,
    project_id: Uuid,
) -> Result<std::path::PathBuf, ProvisionRepoError>
where
    C: ConnectionTrait,
{
    provision_repo_hard(
        db,
        &RepoEnsurer::from_env()?,
        project_id,
        REPO_PROVISIONING_TIMEOUT,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        close_for_notation, mark_git_initialized, provision_repo_hard, provision_repo_in,
        sole_open_matter_for_person, ProvisionRepoError, RepoEnsurer,
    };
    use crate::entity::{notation, person, project, template};
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};

    async fn seed_open_matter(db: &crate::Db) -> (uuid::Uuid, uuid::Uuid) {
        let tmpl = template::ActiveModel {
            code: ActiveValue::Set("closing__letter".into()),
            title: ActiveValue::Set("Closing Letter".into()),
            respondent_type: ActiveValue::Set("person_and_entity".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let person = person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let __dri = crate::test_support::dri_person(db).await;
        let proj = project::ActiveModel {
            name: ActiveValue::Set("matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let notation_id = notation::ActiveModel {
            template_id: ActiveValue::Set(tmpl.id),
            person_id: ActiveValue::Set(person.id),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(proj.id),
            state: ActiveValue::Set("BEGIN".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id;
        (notation_id, proj.id)
    }

    #[tokio::test]
    async fn close_for_notation_flips_open_to_closed() {
        let db = crate::test_support::pg().await;
        let (notation_id, project_id) = seed_open_matter(&db).await;

        let closed = close_for_notation(&db, notation_id).await.unwrap();
        assert_eq!(closed, Some(project_id));

        let row = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "closed");
    }

    #[tokio::test]
    async fn close_for_notation_is_idempotent_and_does_not_unarchive() {
        let db = crate::test_support::pg().await;
        let (notation_id, project_id) = seed_open_matter(&db).await;

        // First close: open -> closed.
        close_for_notation(&db, notation_id).await.unwrap();
        // Manually archive, then re-run: must stay archived (monotonic).
        let row = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut active: project::ActiveModel = row.into();
        active.status = ActiveValue::Set("archived".into());
        active.update(&db).await.unwrap();

        let again = close_for_notation(&db, notation_id).await.unwrap();
        assert_eq!(again, Some(project_id));
        let row = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.status, "archived",
            "close must not walk back from archived"
        );
    }

    #[tokio::test]
    async fn mark_git_initialized_stamps_once_and_is_idempotent() {
        let db = crate::test_support::pg().await;
        let (_notation_id, project_id) = seed_open_matter(&db).await;

        // A freshly-opened matter carries no repo stamp.
        let before = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(before.git_initialized_at.is_none());

        let t1 = chrono::Utc::now();
        let first = mark_git_initialized(&db, project_id, t1).await.unwrap();
        assert_eq!(first, Some(t1.to_rfc3339()));
        let row = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.git_initialized_at.as_deref(),
            Some(t1.to_rfc3339().as_str())
        );

        // A later call (a replay of create-time provisioning, or a lazy init on
        // first clone) must NOT rewrite the original first-creation stamp.
        let t2 = t1 + chrono::Duration::hours(1);
        let second = mark_git_initialized(&db, project_id, t2).await.unwrap();
        assert_eq!(
            second,
            Some(t1.to_rfc3339()),
            "stamp records first creation, not last touch"
        );
    }

    #[tokio::test]
    async fn provision_repo_creates_the_bare_repo_and_stamps_then_is_idempotent() {
        let db = crate::test_support::pg().await;
        let (_notation_id, project_id) = seed_open_matter(&db).await;
        let root = tempfile::TempDir::new().unwrap();
        let store = repos::RepoStore::new(root.path());

        // Fresh matter: no repo on disk, no stamp.
        assert!(!store.exists(project_id));

        let path = provision_repo_in(&db, store.clone(), project_id)
            .await
            .unwrap();

        // The bare repo now exists, and the column is stamped.
        assert!(store.exists(project_id));
        assert!(path.join("HEAD").is_file());
        let stamp = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .git_initialized_at
            .expect("git_initialized_at stamped");
        chrono::DateTime::parse_from_rfc3339(&stamp).expect("RFC 3339");

        // A second call (the lazy transport hitting an already-created repo)
        // is a no-op that preserves the first-creation stamp.
        provision_repo_in(&db, store.clone(), project_id)
            .await
            .unwrap();
        let stamp2 = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .git_initialized_at
            .unwrap();
        assert_eq!(stamp, stamp2, "first-creation stamp must not be rewritten");
    }

    #[tokio::test]
    async fn hard_provision_returns_error_instead_of_swallowing_repo_failures() {
        let db = crate::test_support::pg().await;
        let (_notation_id, project_id) = seed_open_matter(&db).await;
        let file_root = tempfile::NamedTempFile::new().unwrap();
        let ensurer = RepoEnsurer::Local(repos::RepoStore::new(file_root.path()));

        let err = provision_repo_hard(
            &db,
            &ensurer,
            project_id,
            std::time::Duration::from_secs(10),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(err, ProvisionRepoError::Repo(repos::RepoError::Io(_))),
            "expected filesystem error from invalid repo root, got {err:?}",
        );
        let row = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(
            row.git_initialized_at.is_none(),
            "failed hard provision must not stamp the project",
        );
    }

    #[tokio::test]
    async fn hard_provision_fails_when_project_row_is_missing() {
        let db = crate::test_support::pg().await;
        let missing_project_id = uuid::Uuid::now_v7();
        let root = tempfile::tempdir().unwrap();
        let ensurer = RepoEnsurer::Local(repos::RepoStore::new(root.path()));

        let err = provision_repo_hard(
            &db,
            &ensurer,
            missing_project_id,
            std::time::Duration::from_secs(10),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(
                err,
                ProvisionRepoError::NotFound { project_id }
                    if project_id == missing_project_id
            ),
            "expected missing project row to fail provisioning, got {err:?}",
        );
    }

    /// Open one more matter for `person_id` so a person can have several.
    async fn seed_open_matter_for(db: &crate::Db, person_id: uuid::Uuid) -> uuid::Uuid {
        let tmpl = template::ActiveModel {
            code: ActiveValue::Set(format!("onboarding__{}", uuid::Uuid::now_v7())),
            title: ActiveValue::Set("Matter".into()),
            respondent_type: ActiveValue::Set("person".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let __dri = crate::test_support::dri_person(db).await;
        let proj = project::ActiveModel {
            name: ActiveValue::Set("another matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        notation::ActiveModel {
            template_id: ActiveValue::Set(tmpl.id),
            person_id: ActiveValue::Set(person_id),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(proj.id),
            state: ActiveValue::Set("BEGIN".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn sole_open_matter_routes_only_when_unambiguous() {
        let db = crate::test_support::pg().await;
        let (notation_id, project_id) = seed_open_matter(&db).await;
        let person_id = notation::Entity::find_by_id(notation_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .person_id;

        // Exactly one open matter → routes to it.
        assert_eq!(
            sole_open_matter_for_person(&db, person_id).await.unwrap(),
            Some(notation_id)
        );

        // Close it → no open matter → no routing.
        close_for_notation(&db, notation_id).await.unwrap();
        let _ = project_id;
        assert_eq!(
            sole_open_matter_for_person(&db, person_id).await.unwrap(),
            None
        );

        // Two open matters → ambiguous → no routing (manual @link instead).
        let a = seed_open_matter_for(&db, person_id).await;
        let _b = seed_open_matter_for(&db, person_id).await;
        assert_eq!(
            sole_open_matter_for_person(&db, person_id).await.unwrap(),
            None,
            "two open matters must not auto-route"
        );
        let _ = a;
    }

    #[tokio::test]
    async fn close_for_notation_returns_none_for_unknown_notation() {
        let db = crate::test_support::pg().await;
        let missing = close_for_notation(&db, uuid::Uuid::from_u128(9999))
            .await
            .unwrap();
        assert_eq!(missing, None);
    }

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn ensurer_resolves_local_when_repo_root_is_set() {
        let ensurer = RepoEnsurer::from_lookup(lookup(&[(repos::REPO_ROOT_ENV, "/var/repos")]));
        assert!(matches!(ensurer, Ok(RepoEnsurer::Local(_))));
    }

    #[test]
    fn ensurer_prefers_local_when_both_are_configured() {
        // A process with the volume never needs the network.
        let ensurer = RepoEnsurer::from_lookup(lookup(&[
            (repos::REPO_ROOT_ENV, "/var/repos"),
            (super::GIT_WRITER_URL_ENV, "http://navigator-git:3001"),
            (super::GIT_WRITER_TOKEN_ENV, "t0ken"),
        ]));
        assert!(matches!(ensurer, Ok(RepoEnsurer::Local(_))));
    }

    #[test]
    fn ensurer_resolves_remote_from_writer_url_and_token() {
        let ensurer = RepoEnsurer::from_lookup(lookup(&[
            (super::GIT_WRITER_URL_ENV, "http://navigator-git:3001"),
            (super::GIT_WRITER_TOKEN_ENV, "t0ken"),
        ]));
        assert!(matches!(ensurer, Ok(RepoEnsurer::Remote(_))));
    }

    #[test]
    fn ensurer_requires_both_writer_url_and_token() {
        for partial in [
            vec![(super::GIT_WRITER_URL_ENV, "http://navigator-git:3001")],
            vec![(super::GIT_WRITER_TOKEN_ENV, "t0ken")],
            vec![],
            // Empty strings are a *present but useless* deploy bug — treat
            // them as absent rather than building a client that can't work.
            vec![
                (super::GIT_WRITER_URL_ENV, ""),
                (super::GIT_WRITER_TOKEN_ENV, "t0ken"),
            ],
            vec![(repos::REPO_ROOT_ENV, "")],
        ] {
            let err = RepoEnsurer::from_lookup(lookup(&partial)).unwrap_err();
            assert!(
                matches!(err, ProvisionRepoError::NotConfigured),
                "expected NotConfigured for {partial:?}, got {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn remote_ensure_posts_bearer_and_project_id_to_the_writer() {
        use wiremock::matchers::{body_partial_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let project_id = uuid::Uuid::now_v7();
        let writer = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(super::GIT_WRITER_ENSURE_PATH))
            .and(header("authorization", "Bearer t0ken"))
            .and(body_partial_json(
                serde_json::json!({ "project_id": project_id }),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "path": "/var/lib/navigator/git-repos/x.git" }),
                ),
            )
            .expect(1)
            .mount(&writer)
            .await;

        let ensurer = RepoEnsurer::Remote(super::RemoteWriter::new(writer.uri(), "t0ken"));
        let path = ensurer.ensure(project_id).await.unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("/var/lib/navigator/git-repos/x.git")
        );
    }

    #[tokio::test]
    async fn remote_ensure_failure_surfaces_as_remote_writer_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let writer = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(super::GIT_WRITER_ENSURE_PATH))
            .respond_with(ResponseTemplate::new(500).set_body_string("volume gone"))
            .mount(&writer)
            .await;

        let ensurer = RepoEnsurer::Remote(super::RemoteWriter::new(writer.uri(), "t0ken"));
        let err = ensurer.ensure(uuid::Uuid::now_v7()).await.unwrap_err();
        assert!(matches!(err, ProvisionRepoError::RemoteWriter(_)));
    }

    #[tokio::test]
    async fn hard_provision_through_the_remote_writer_stamps_in_the_caller_txn() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let db = crate::test_support::pg().await;
        let (_notation_id, project_id) = seed_open_matter(&db).await;
        let writer = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(super::GIT_WRITER_ENSURE_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "path": "/repos/remote.git" })),
            )
            .expect(1)
            .mount(&writer)
            .await;

        let ensurer = RepoEnsurer::Remote(super::RemoteWriter::new(writer.uri(), "t0ken"));
        provision_repo_hard(
            &db,
            &ensurer,
            project_id,
            std::time::Duration::from_secs(10),
        )
        .await
        .unwrap();

        let stamp = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .git_initialized_at;
        assert!(
            stamp.is_some(),
            "the caller-side stamp must happen even when the filesystem half is remote"
        );
    }
}
