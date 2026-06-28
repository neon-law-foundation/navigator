//! Import: walk a directory of validated template markdown files and
//! persist each as a `templates` row, registering every question code
//! referenced by the file's `questionnaire:` and `workflow:` maps as a
//! `questions` row (creating each on first sight).
//!
//! The import is intentionally idempotent: re-importing the same
//! directory must not produce duplicate rows. `template.code` and
//! `question.code` both carry unique indexes, so we look up by code
//! before inserting and skip whenever a matching row already exists.
//!
//! The CLI calls this both from the `import` subcommand (file-backed
//! `SQLite`) and from integration tests (in-memory `SQLite`). The
//! fixture repository lives at `notation_templates/<category>/<name>.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rules::{navigator_default_rules_with_codes, DefaultFileFilter, FileFilter, Violation};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use serde::Deserialize;
use store::entity::{question, template};
use walkdir::WalkDir;

/// Load every known question code from the `questions` table and
/// return them as a strict registry suitable for passing to
/// `F104FlowQuestionCodes::new`. Used by `navigator validate` after a
/// directory has been imported so N104 can flag unknown codes.
pub async fn load_question_codes(db: &DatabaseConnection) -> anyhow::Result<Vec<String>> {
    let rows = question::Entity::find().all(db).await?;
    Ok(rows.into_iter().map(|q| q.code).collect())
}

/// Outcome of a single import run: how many templates and questions
/// were created, plus any rule violations keyed by path.
#[derive(Debug, Default)]
pub struct ImportReport {
    pub templates_created: usize,
    pub questions_created: usize,
    pub files_skipped_due_to_violations: usize,
    pub violations: Vec<Violation>,
}

#[derive(Debug, Deserialize)]
struct TemplateFrontmatter {
    title: Option<String>,
    respondent_type: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    questionnaire: Option<BTreeMap<String, BTreeMap<String, String>>>,
    #[serde(default)]
    workflow: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

/// Walk `dir`, validate every `*.md` (with the default Neon Law Navigator rule
/// set minus N104 question-code validation since we're populating the
/// registry as we go), and insert one `templates` row + one `questions`
/// row per referenced question code. Files with any rule violation
/// are skipped — they're recorded in [`ImportReport::violations`] for
/// the caller to report.
pub async fn import_directory(
    db: &DatabaseConnection,
    storage: &std::sync::Arc<dyn cloud::StorageService>,
    dir: &Path,
) -> anyhow::Result<ImportReport> {
    let mut report = ImportReport::default();
    let validation_rules = navigator_default_rules_with_codes(&[]);
    let filter = DefaultFileFilter::default();
    for entry in WalkDir::new(dir).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !filter.include_file(path) {
            continue;
        }
        let contents = std::fs::read_to_string(path)?;
        let file = rules::SourceFile {
            path: PathBuf::from(path),
            contents: contents.clone(),
        };
        let file_violations: Vec<Violation> = validation_rules
            .iter()
            .flat_map(|r| r.lint(&file))
            .collect();
        // Only blocking (Error-severity) violations skip a file. Yellow
        // advisories like N112 ("step allowed but not built yet") apply
        // to nearly every template's staff_review gate and must not stop
        // it from importing.
        let has_errors = file_violations
            .iter()
            .any(|v| rules::severity_for_code(v.code) == rules::Severity::Error);
        if has_errors {
            report.files_skipped_due_to_violations += 1;
            report.violations.extend(file_violations);
            continue;
        }
        if let Some(parsed) = parse_frontmatter(&contents) {
            persist_template(db, storage, path, &parsed, &mut report).await?;
        }
    }
    Ok(report)
}

fn parse_frontmatter(contents: &str) -> Option<TemplateFrontmatter> {
    let fm = rules::frontmatter::extract(contents)?;
    serde_yaml::from_str(fm).ok()
}

/// Derive the template code from frontmatter or fall back to the
/// filename stem.
fn template_code(path: &Path, fm: &TemplateFrontmatter) -> String {
    fm.code.clone().unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string()
    })
}

async fn persist_template(
    db: &DatabaseConnection,
    storage: &std::sync::Arc<dyn cloud::StorageService>,
    path: &Path,
    fm: &TemplateFrontmatter,
    report: &mut ImportReport,
) -> anyhow::Result<()> {
    let code = template_code(path, fm);
    let existing = store::templates::resolve(db, None, &code).await?;
    if existing.is_none() {
        // The body lives in a content-addressed blob, not an inline
        // column. Ingest the file contents and reference the blob.
        let body = std::fs::read_to_string(path)?;
        let blob_id = store::blobs::ingest(db, storage, body.as_bytes(), "text/markdown").await?;
        template::ActiveModel {
            code: ActiveValue::Set(code.clone()),
            title: ActiveValue::Set(fm.title.clone().unwrap_or_else(|| code.clone())),
            respondent_type: ActiveValue::Set(
                fm.respondent_type
                    .clone()
                    .unwrap_or_else(|| "entity".into()),
            ),
            project_id: ActiveValue::Set(None),
            blob_id: ActiveValue::Set(Some(blob_id)),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.templates_created += 1;
    }

    // Collect every question code referenced by either map. State keys
    // may carry a `__label` suffix; the question code is the prefix.
    // `BEGIN` and `END` are control states and never registered;
    // `staff_review` is a workflow state but we register it so a later
    // `N104` pass keyed on the populated `questions` table doesn't
    // flag it as unknown.
    let mut codes: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for map in [&fm.questionnaire, &fm.workflow].into_iter().flatten() {
        for state in map.keys() {
            if state == "BEGIN" || state == "END" {
                continue;
            }
            let prefix = state.split_once("__").map_or(state.as_str(), |(p, _)| p);
            codes.insert(prefix.to_string());
        }
    }
    for q_code in codes {
        let existing = question::Entity::find()
            .filter(question::Column::Code.eq(q_code.clone()))
            .one(db)
            .await?;
        if existing.is_some() {
            continue;
        }
        question::ActiveModel {
            code: ActiveValue::Set(q_code.clone()),
            prompt: ActiveValue::Set(format!("(auto-imported) {q_code}")),
            answer_type: ActiveValue::Set("string".into()),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.questions_created += 1;
    }
    Ok(())
}
