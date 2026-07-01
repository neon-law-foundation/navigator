//! Canonical seed loader: insert the workspace-bundled YAML fixtures
//! (`store/seeds/*.yaml`) into every entity table the schema knows
//! about. Re-running is a no-op on the natural keys of each table.
//!
//! The YAML files use a `lookup_fields: + records:` shape (see
//! `store/seeds/`); this loader resolves nested foreign references
//! (e.g., `entity.entity_type.name`) by looking up rows by their
//! natural key.
//!
//! Both binaries in the workspace go through this module:
//! - `navigator list ...` calls [`seed_canonical`] before reading.
//! - `web` calls [`seed_canonical`] after migrations on startup.

use std::collections::BTreeMap;

use crate::entity::{
    address, answer, credential, entity as entities, entity_billing_profile, entity_type,
    git_repository, invoice, invoice_line_item, jurisdiction, letter, mailroom, person,
    person_entity_role, person_project_role, product, project, question, template, testimonial,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel,
    QueryFilter,
};
use serde::Deserialize;
use uuid::Uuid;

/// Per-entity insert counts for one seed pass.
#[derive(Debug, Default, Clone, Copy)]
pub struct SeedReport {
    pub jurisdictions_inserted: usize,
    pub entity_types_inserted: usize,
    pub entities_inserted: usize,
    pub persons_inserted: usize,
    pub persons_updated: usize,
    pub projects_inserted: usize,
    pub git_repositories_inserted: usize,
    pub questions_inserted: usize,
    pub question_translations_inserted: usize,
    pub mailrooms_inserted: usize,
    pub addresses_inserted: usize,
    pub letters_inserted: usize,
    pub answers_inserted: usize,
    pub person_entity_roles_inserted: usize,
    pub person_project_roles_inserted: usize,
    pub entity_billing_profiles_inserted: usize,
    pub invoices_inserted: usize,
    pub invoice_line_items_inserted: usize,
    pub credentials_inserted: usize,
    pub templates_inserted: usize,
    pub products_inserted: usize,
    pub testimonials_inserted: usize,
}

impl SeedReport {
    /// One-line summary suitable for CLI output. Reports every entity
    /// even when zero so re-runs make it visible that the pass was
    /// a no-op.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "Seeded: {} jurisdictions, {} entity_types, {} entities, {} persons \
             (+{} role updates), {} projects, {} git_repos, {} questions, \
             {} mailrooms, {} addresses, {} letters, {} answers, \
             {} person_entity_roles, {} person_project_roles, \
             {} billing_profiles, {} invoices, {} invoice_line_items, {} credentials, \
             {} templates, {} products, {} testimonials.",
            self.jurisdictions_inserted,
            self.entity_types_inserted,
            self.entities_inserted,
            self.persons_inserted,
            self.persons_updated,
            self.projects_inserted,
            self.git_repositories_inserted,
            self.questions_inserted,
            self.mailrooms_inserted,
            self.addresses_inserted,
            self.letters_inserted,
            self.answers_inserted,
            self.person_entity_roles_inserted,
            self.person_project_roles_inserted,
            self.entity_billing_profiles_inserted,
            self.invoices_inserted,
            self.invoice_line_items_inserted,
            self.credentials_inserted,
            self.templates_inserted,
            self.products_inserted,
            self.testimonials_inserted,
        )
    }
}

// ---------- Embedded canonical YAMLs ----------
//
// Bundled at compile time so the installed `navigator` binary is
// self-contained — no runtime lookup of `store/seeds/`.

/// The canonical jurisdiction reference data, embedded at compile time.
/// Exposed so cross-crate reconciliation tests (e.g. `cli`) can assert the
/// path vocabulary in `rules::f110` stays in sync with the seeded rows
/// without reaching into `store`'s private modules.
pub const JURISDICTION_SEED_YAML: &str = canonical::JURISDICTION;

mod canonical {
    pub const JURISDICTION: &str = include_str!("../seeds/Jurisdiction.yaml");
    pub const ENTITY_TYPE: &str = include_str!("../seeds/EntityType.yaml");
    pub const ENTITY: &str = include_str!("../seeds/Entity.yaml");
    pub const PERSON: &str = include_str!("../seeds/Person.yaml");
    pub const USER: &str = include_str!("../seeds/User.yaml");
    pub const PROJECT: &str = include_str!("../seeds/Project.yaml");
    pub const GIT_REPOSITORY: &str = include_str!("../seeds/GitRepository.yaml");
    pub const QUESTION: &str = include_str!("../seeds/Question.yaml");
    pub const QUESTION_TRANSLATION: &str = include_str!("../seeds/QuestionTranslation.yaml");
    pub const MAILROOM: &str = include_str!("../seeds/Mailroom.yaml");
    pub const ADDRESS: &str = include_str!("../seeds/Address.yaml");
    pub const LETTER: &str = include_str!("../seeds/Letter.yaml");
    pub const ANSWER: &str = include_str!("../seeds/Answer.yaml");
    pub const PERSON_ENTITY_ROLE: &str = include_str!("../seeds/PersonEntityRole.yaml");
    pub const PERSON_PROJECT_ROLE: &str = include_str!("../seeds/PersonProjectRole.yaml");
    pub const ENTITY_BILLING_PROFILE: &str = include_str!("../seeds/EntityBillingProfile.yaml");
    pub const INVOICE: &str = include_str!("../seeds/Invoice.yaml");
    pub const INVOICE_LINE_ITEM: &str = include_str!("../seeds/InvoiceLineItem.yaml");
    pub const CREDENTIAL: &str = include_str!("../seeds/Credential.yaml");
    pub const PRODUCT: &str = include_str!("../seeds/Product.yaml");
    pub const TESTIMONIAL: &str = include_str!("../seeds/Testimonial.yaml");

    /// Bundled notation templates. Each entry is `(path, full_md)`
    /// where `path` exists only as a label in the seed report.
    /// Adding a template here lets the cluster's Postgres carry
    /// it without a separate `navigator import` step. The full
    /// shipped catalog is bundled so a fresh cluster carries every
    /// template without an import pass.
    pub const TEMPLATE_RETAINER: &str = include_str!("../../templates/neon_law/shared/retainer.md");
    pub const TEMPLATE_CLOSING_LETTER: &str =
        include_str!("../../templates/neon_law/shared/closing_letter.md");
    pub const TEMPLATE_ANNUAL_REPORT_NV: &str =
        include_str!("../../templates/forms/united_states/nevada/state/nv__annual_report.md");
    pub const TEMPLATE_DISSOLUTION_NV: &str =
        include_str!("../../templates/forms/united_states/nevada/state/nv__dissolution.md");
    pub const TEMPLATE_LLC_CA: &str =
        include_str!("../../templates/neon_law/nest/ca__llc_operating_agreement.md");
    pub const TEMPLATE_FORM990: &str =
        include_str!("../../templates/forms/united_states/federal/irs/us__form_990.md");
    pub const TEMPLATE_NONPROFIT_501C3_NV: &str = include_str!(
        "../../templates/forms/united_states/nevada/state/nv__nonprofit_501c3_formation.md"
    );
    pub const TEMPLATE_CHARITABLE_SOLICITATION_NV: &str = include_str!(
        "../../templates/forms/united_states/nevada/state/nv__charitable_solicitation_registration.md"
    );
    pub const TEMPLATE_NV_MBT: &str = include_str!(
        "../../templates/forms/united_states/nevada/state/nv__modified_business_tax.md"
    );
    pub const TEMPLATE_TRUST_NV: &str =
        include_str!("../../templates/neon_law/northstar/nv__generic_trust.md");
    pub const TEMPLATE_WILL_SIMPLE: &str =
        include_str!("../../templates/neon_law/northstar/nv__simple_will.md");
    pub const TEMPLATE_ESTATE: &str =
        include_str!("../../templates/neon_law/northstar/estate_plan.md");
    // Northstar estate instrument stubs — the will, trust, and the two
    // directives the `document_drafts__estate` step renders from the
    // sitting's answers into one `review_documents` row each.
    pub const TEMPLATE_NORTHSTAR_WILL: &str =
        include_str!("../../templates/neon_law/northstar/nv__will.md");
    pub const TEMPLATE_NORTHSTAR_TRUST: &str =
        include_str!("../../templates/neon_law/northstar/nv__trust.md");
    pub const TEMPLATE_NORTHSTAR_DIRECTIVE_HEALTH: &str =
        include_str!("../../templates/neon_law/northstar/nv__directive_health.md");
    pub const TEMPLATE_NORTHSTAR_DIRECTIVE_FINANCIAL: &str =
        include_str!("../../templates/neon_law/northstar/nv__directive_financial.md");
    pub const TEMPLATE_NEST_NV: &str =
        include_str!("../../templates/forms/united_states/nevada/state/nv__llc_formation.md");
    pub const TEMPLATE_NEST_CORP_NV: &str = include_str!(
        "../../templates/forms/united_states/nevada/state/nv__profit_corp_formation.md"
    );
    pub const TEMPLATE_NEST_BUSINESS_TRUST_NV: &str = include_str!(
        "../../templates/forms/united_states/nevada/state/nv__business_trust_formation.md"
    );
    pub const TEMPLATE_NEXUS: &str =
        include_str!("../../templates/neon_law/nexus/fractional_gc.md");
    pub const TEMPLATE_EMPLOYMENT_W2: &str =
        include_str!("../../templates/neon_law/nexus/nv__employment_agreement.md");
    pub const TEMPLATE_CONTRACTOR_1099: &str =
        include_str!("../../templates/neon_law/nexus/nv__contractor_agreement.md");
    pub const TEMPLATE_CONTRACT_REVIEW: &str =
        include_str!("../../templates/neon_law/nexus/contract_review.md");
    pub const TEMPLATE_NAUTILUS_CEASE: &str =
        include_str!("../../templates/neon_law/nautilus/cease_communication.md");
    pub const TEMPLATE_NAUTILUS_DEBT_VALIDATION: &str =
        include_str!("../../templates/neon_law/nautilus/debt_validation.md");
    pub const TEMPLATE_NAUTILUS_FCRA: &str =
        include_str!("../../templates/neon_law/nautilus/fcra_dispute.md");
    pub const TEMPLATE_NAUTILUS_NOTICE: &str =
        include_str!("../../templates/neon_law/nautilus/notice_of_representation.md");
    pub const TEMPLATE_NAUTILUS_SETTLEMENT: &str =
        include_str!("../../templates/neon_law/nautilus/settlement_letter.md");
    pub const TEMPLATE_NATURALIZATION: &str =
        include_str!("../../templates/forms/united_states/federal/uscis/us__naturalization.md");
    // Service-specific retainers — one engagement agreement per product.
    // Each carries the shared JAMS arbitration + `support@` clauses
    // (byte-identical across all six, guarded by a body test) and a
    // practice-area-specific ethics reading naming the RPC(s) that bite
    // for that service.
    pub const TEMPLATE_RETAINER_NEST: &str =
        include_str!("../../templates/neon_law/nest/retainer.md");
    pub const TEMPLATE_RETAINER_NEXUS: &str =
        include_str!("../../templates/neon_law/nexus/retainer.md");
    pub const TEMPLATE_RETAINER_NORTHSTAR: &str =
        include_str!("../../templates/neon_law/northstar/retainer.md");
    pub const TEMPLATE_RETAINER_NAUTILUS: &str =
        include_str!("../../templates/neon_law/nautilus/retainer.md");
    pub const TEMPLATE_RETAINER_NOOK: &str =
        include_str!("../../templates/neon_law/nook/retainer.md");
    pub const TEMPLATE_RETAINER_LITIGATION: &str =
        include_str!("../../templates/neon_law/litigation/retainer.md");
    pub const TEMPLATE_RETAINER_NERD: &str =
        include_str!("../../templates/neon_law/nerd/retainer.md");
    pub const TEMPLATE_RETAINER_NODE: &str =
        include_str!("../../templates/neon_law/node/retainer.md");
    pub const TEMPLATE_RETAINER_NEWLEAF: &str =
        include_str!("../../templates/neon_law/newleaf/retainer.md");
    pub const TEMPLATE_RETAINER_NAMESAKE: &str =
        include_str!("../../templates/neon_law/namesake/retainer.md");
    pub const TEMPLATE_RETAINER_NUCLEUS: &str =
        include_str!("../../templates/neon_law/nucleus/retainer.md");
}

/// Wrap a list of records under the YAML's `records:` key. Every seed
/// YAML in `store/seeds/` has the same outer shape.
#[derive(Debug, Deserialize)]
struct Records<T> {
    #[serde(default = "Vec::new")]
    records: Vec<T>,
}

fn parse<T>(yaml: &str, file: &str) -> anyhow::Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let r: Records<T> =
        serde_yaml::from_str(yaml).map_err(|e| anyhow::anyhow!("parse {file}: {e}"))?;
    Ok(r.records)
}

/// Run the full canonical seed pass against `db`. Each entity table
/// is populated from its corresponding `store/seeds/*.yaml` file.
/// Idempotent: re-running inserts no new rows.
pub async fn seed_canonical(
    db: &DatabaseConnection,
    storage: &std::sync::Arc<dyn cloud::StorageService>,
) -> anyhow::Result<SeedReport> {
    let mut r = SeedReport::default();
    seed_jurisdictions(db, &mut r).await?;
    seed_entity_types(db, &mut r).await?;
    seed_entities(db, &mut r).await?;
    seed_persons(db, &mut r).await?;
    seed_user_roles(db, &mut r).await?;
    seed_projects(db, &mut r).await?;
    seed_git_repositories(db, &mut r).await?;
    seed_questions(db, &mut r).await?;
    seed_question_translations(db, &mut r).await?;
    seed_mailrooms(db, &mut r).await?;
    seed_addresses(db, &mut r).await?;
    seed_letters(db, &mut r).await?;
    seed_answers(db, &mut r).await?;
    seed_person_entity_roles(db, &mut r).await?;
    seed_person_project_roles(db, &mut r).await?;
    let billing = seed_billing_profiles(db, &mut r).await?;
    let by_invoice = seed_invoices(db, &mut r, &billing).await?;
    seed_invoice_line_items(db, &mut r, &by_invoice).await?;
    seed_credentials(db, &mut r).await?;
    seed_templates(db, storage, &mut r).await?;
    seed_products(db, &mut r).await?;
    seed_testimonials(db, &mut r).await?;
    Ok(r)
}

#[derive(Debug, Deserialize)]
struct TemplateFrontmatter {
    code: String,
    title: String,
    respondent_type: String,
    /// forms-registry code of the government form this template fills
    /// (`form: nv__llc_formation`); absent for Typst-rendered
    /// templates.
    #[serde(default)]
    form: Option<String>,
}

/// Split a notation template's markdown into `(frontmatter, body)`.
/// The frontmatter is the YAML between the opening and closing
/// `---\n` markers; the body is everything after.
fn split_template(md: &str) -> Option<(&str, &str)> {
    let after_open = md.strip_prefix("---\n")?;
    let end = after_open.find("\n---\n")?;
    let fm = &after_open[..end];
    let body = &after_open[end + "\n---\n".len()..];
    Some((fm, body))
}

/// Seed the workspace-bundled notation templates into the
/// `templates` table. Idempotent on `code` — re-running is a
/// no-op. The full shipped catalog is bundled; add more
/// `include_str!` entries in `canonical` above and a row here to
/// extend.
#[allow(clippy::too_many_lines)]
async fn seed_templates(
    db: &DatabaseConnection,
    storage: &std::sync::Arc<dyn cloud::StorageService>,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    for (label, md) in [
        ("neon_law/shared/retainer.md", canonical::TEMPLATE_RETAINER),
        (
            "neon_law/shared/closing_letter.md",
            canonical::TEMPLATE_CLOSING_LETTER,
        ),
        (
            "forms/united_states/nevada/state/nv__annual_report.md",
            canonical::TEMPLATE_ANNUAL_REPORT_NV,
        ),
        (
            "forms/united_states/nevada/state/nv__dissolution.md",
            canonical::TEMPLATE_DISSOLUTION_NV,
        ),
        (
            "neon_law/nest/ca__llc_operating_agreement.md",
            canonical::TEMPLATE_LLC_CA,
        ),
        (
            "forms/united_states/federal/irs/us__form_990.md",
            canonical::TEMPLATE_FORM990,
        ),
        (
            "forms/united_states/nevada/state/nv__nonprofit_501c3_formation.md",
            canonical::TEMPLATE_NONPROFIT_501C3_NV,
        ),
        (
            "forms/united_states/nevada/state/nv__charitable_solicitation_registration.md",
            canonical::TEMPLATE_CHARITABLE_SOLICITATION_NV,
        ),
        (
            "forms/united_states/nevada/state/nv__modified_business_tax.md",
            canonical::TEMPLATE_NV_MBT,
        ),
        (
            "neon_law/northstar/nv__generic_trust.md",
            canonical::TEMPLATE_TRUST_NV,
        ),
        (
            "neon_law/northstar/nv__simple_will.md",
            canonical::TEMPLATE_WILL_SIMPLE,
        ),
        (
            "neon_law/northstar/estate_plan.md",
            canonical::TEMPLATE_ESTATE,
        ),
        (
            "neon_law/northstar/nv__will.md",
            canonical::TEMPLATE_NORTHSTAR_WILL,
        ),
        (
            "neon_law/northstar/nv__trust.md",
            canonical::TEMPLATE_NORTHSTAR_TRUST,
        ),
        (
            "neon_law/northstar/nv__directive_health.md",
            canonical::TEMPLATE_NORTHSTAR_DIRECTIVE_HEALTH,
        ),
        (
            "neon_law/northstar/nv__directive_financial.md",
            canonical::TEMPLATE_NORTHSTAR_DIRECTIVE_FINANCIAL,
        ),
        (
            "forms/united_states/nevada/state/nv__llc_formation.md",
            canonical::TEMPLATE_NEST_NV,
        ),
        (
            "forms/united_states/nevada/state/nv__profit_corp_formation.md",
            canonical::TEMPLATE_NEST_CORP_NV,
        ),
        (
            "forms/united_states/nevada/state/nv__business_trust_formation.md",
            canonical::TEMPLATE_NEST_BUSINESS_TRUST_NV,
        ),
        ("neon_law/nexus/fractional_gc.md", canonical::TEMPLATE_NEXUS),
        (
            "neon_law/nexus/nv__employment_agreement.md",
            canonical::TEMPLATE_EMPLOYMENT_W2,
        ),
        (
            "neon_law/nexus/nv__contractor_agreement.md",
            canonical::TEMPLATE_CONTRACTOR_1099,
        ),
        (
            "neon_law/nexus/contract_review.md",
            canonical::TEMPLATE_CONTRACT_REVIEW,
        ),
        (
            "neon_law/nautilus/cease_communication.md",
            canonical::TEMPLATE_NAUTILUS_CEASE,
        ),
        (
            "neon_law/nautilus/debt_validation.md",
            canonical::TEMPLATE_NAUTILUS_DEBT_VALIDATION,
        ),
        (
            "neon_law/nautilus/fcra_dispute.md",
            canonical::TEMPLATE_NAUTILUS_FCRA,
        ),
        (
            "neon_law/nautilus/notice_of_representation.md",
            canonical::TEMPLATE_NAUTILUS_NOTICE,
        ),
        (
            "neon_law/nautilus/settlement_letter.md",
            canonical::TEMPLATE_NAUTILUS_SETTLEMENT,
        ),
        (
            "forms/united_states/federal/uscis/us__naturalization.md",
            canonical::TEMPLATE_NATURALIZATION,
        ),
        (
            "neon_law/nest/retainer.md",
            canonical::TEMPLATE_RETAINER_NEST,
        ),
        (
            "neon_law/nexus/retainer.md",
            canonical::TEMPLATE_RETAINER_NEXUS,
        ),
        (
            "neon_law/northstar/retainer.md",
            canonical::TEMPLATE_RETAINER_NORTHSTAR,
        ),
        (
            "neon_law/nautilus/retainer.md",
            canonical::TEMPLATE_RETAINER_NAUTILUS,
        ),
        (
            "neon_law/nook/retainer.md",
            canonical::TEMPLATE_RETAINER_NOOK,
        ),
        (
            "neon_law/litigation/retainer.md",
            canonical::TEMPLATE_RETAINER_LITIGATION,
        ),
        (
            "neon_law/nerd/retainer.md",
            canonical::TEMPLATE_RETAINER_NERD,
        ),
        (
            "neon_law/node/retainer.md",
            canonical::TEMPLATE_RETAINER_NODE,
        ),
        (
            "neon_law/newleaf/retainer.md",
            canonical::TEMPLATE_RETAINER_NEWLEAF,
        ),
        (
            "neon_law/namesake/retainer.md",
            canonical::TEMPLATE_RETAINER_NAMESAKE,
        ),
        (
            "neon_law/nucleus/retainer.md",
            canonical::TEMPLATE_RETAINER_NUCLEUS,
        ),
    ] {
        let (fm_str, body) = split_template(md)
            .ok_or_else(|| anyhow::anyhow!("{label}: missing YAML frontmatter"))?;
        let fm: TemplateFrontmatter = serde_yaml::from_str(fm_str)
            .map_err(|e| anyhow::anyhow!("{label}: parse frontmatter: {e}"))?;

        // The body lives in a content-addressed blob; ingest it (sha
        // dedup) and reference it by `blob_id`.
        let body_bytes = body.trim_start().as_bytes();
        let blob_id = crate::blobs::ingest(db, storage, body_bytes, "text/markdown")
            .await
            .map_err(|e| anyhow::anyhow!("{label}: ingest body blob: {e}"))?;

        // Idempotent on the shared (project_id IS NULL) code. A fresh
        // cluster inserts the row; an existing row gets `blob_id` (from
        // before the body→blob move) or a changed `form_code` binding
        // backfilled.
        if let Some(existing) = template::Entity::find()
            .filter(template::Column::Code.eq(fm.code.clone()))
            .filter(template::Column::ProjectId.is_null())
            .one(db)
            .await?
        {
            backfill_template(db, existing, blob_id, fm.form.clone()).await?;
            continue;
        }

        template::ActiveModel {
            code: ActiveValue::Set(fm.code),
            title: ActiveValue::Set(fm.title),
            respondent_type: ActiveValue::Set(fm.respondent_type),
            project_id: ActiveValue::Set(None),
            blob_id: ActiveValue::Set(Some(blob_id)),
            form_code: ActiveValue::Set(fm.form),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.templates_inserted += 1;
    }
    Ok(())
}

/// Backfill an already-seeded template row: `blob_id` (rows from before
/// the body→blob move) and a changed `form_code` binding. No-op when
/// both already match.
async fn backfill_template(
    db: &DatabaseConnection,
    existing: template::Model,
    blob_id: Uuid,
    form: Option<String>,
) -> anyhow::Result<()> {
    let needs_blob = existing.blob_id.is_none();
    let needs_form = existing.form_code != form;
    if needs_blob || needs_form {
        let mut active: template::ActiveModel = existing.into();
        if needs_blob {
            active.blob_id = ActiveValue::Set(Some(blob_id));
        }
        if needs_form {
            active.form_code = ActiveValue::Set(form);
        }
        active.update(db).await?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct CredentialRec {
    person: PersonEmailRef,
    jurisdiction: JurisdictionRef,
    license_number: String,
}

async fn seed_credentials(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<CredentialRec>(canonical::CREDENTIAL, "Credential.yaml")? {
        let Some(p) = person::Entity::find()
            .filter(person::Column::Email.eq(rec.person.email.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        let Some(j) = jurisdiction::Entity::find()
            .filter(jurisdiction::Column::Name.eq(rec.jurisdiction.name.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        if credential::Entity::find()
            .filter(credential::Column::PersonId.eq(p.id))
            .filter(credential::Column::JurisdictionId.eq(j.id))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        credential::ActiveModel {
            person_id: ActiveValue::Set(p.id),
            jurisdiction_id: ActiveValue::Set(j.id),
            license_number: ActiveValue::Set(rec.license_number),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.credentials_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ProductRec {
    code: String,
    display_name: String,
    list_price_cents: i64,
    currency: String,
    cadence: String,
    billing_kind: String,
    #[serde(default = "default_true")]
    active: bool,
    #[serde(default)]
    xero_item_code: Option<String>,
    #[serde(default = "default_account_code")]
    account_code: String,
    #[serde(default)]
    matter_close_template_code: Option<String>,
    #[serde(default)]
    retainer_template_code: Option<String>,
}

fn default_account_code() -> String {
    "200".to_string()
}

fn default_true() -> bool {
    true
}

/// Seed the firm's product catalog. Idempotent on `code` — re-running is
/// a no-op. This is the single source of truth for each product's list
/// price; `web::retainer_walk::flat_fee_cents` reads it to resolve a
/// matter-close fee, so a cents edit here changes what a client is billed.
async fn seed_products(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<ProductRec>(canonical::PRODUCT, "Product.yaml")? {
        if product::Entity::find()
            .filter(product::Column::Code.eq(rec.code.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        product::ActiveModel {
            code: ActiveValue::Set(rec.code),
            display_name: ActiveValue::Set(rec.display_name),
            list_price_cents: ActiveValue::Set(rec.list_price_cents),
            currency: ActiveValue::Set(rec.currency),
            cadence: ActiveValue::Set(rec.cadence),
            billing_kind: ActiveValue::Set(rec.billing_kind),
            active: ActiveValue::Set(rec.active),
            xero_item_code: ActiveValue::Set(rec.xero_item_code),
            account_code: ActiveValue::Set(rec.account_code),
            matter_close_template_code: ActiveValue::Set(rec.matter_close_template_code),
            retainer_template_code: ActiveValue::Set(rec.retainer_template_code),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.products_inserted += 1;
    }
    Ok(())
}

// ---------- Per-entity loaders ----------

#[derive(Debug, Deserialize)]
struct JurisdictionRec {
    name: String,
    code: String,
    jurisdiction_type: String,
}

async fn seed_jurisdictions(
    db: &DatabaseConnection,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    for rec in parse::<JurisdictionRec>(canonical::JURISDICTION, "Jurisdiction.yaml")? {
        if jurisdiction::Entity::find()
            .filter(jurisdiction::Column::Code.eq(rec.code.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        jurisdiction::ActiveModel {
            name: ActiveValue::Set(rec.name),
            code: ActiveValue::Set(rec.code),
            jurisdiction_type: ActiveValue::Set(rec.jurisdiction_type),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.jurisdictions_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct EntityTypeRec {
    name: String,
}

async fn seed_entity_types(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for rec in parse::<EntityTypeRec>(canonical::ENTITY_TYPE, "EntityType.yaml")? {
        if !seen.insert(rec.name.clone()) {
            continue;
        }
        if entity_type::Entity::find()
            .filter(entity_type::Column::Name.eq(rec.name.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        entity_type::ActiveModel {
            name: ActiveValue::Set(rec.name),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.entity_types_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct EntityRec {
    name: String,
    entity_type: EntityTypeRef,
}

#[derive(Debug, Deserialize)]
struct EntityTypeRef {
    name: String,
    #[serde(default)]
    jurisdiction: Option<JurisdictionRef>,
}

#[derive(Debug, Deserialize)]
struct JurisdictionRef {
    name: String,
}

async fn seed_entities(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<EntityRec>(canonical::ENTITY, "Entity.yaml")? {
        let et = entity_type::Entity::find()
            .filter(entity_type::Column::Name.eq(rec.entity_type.name.clone()))
            .one(db)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Entity.yaml references unknown entity_type {name:?}",
                    name = rec.entity_type.name
                )
            })?;
        let jurisdiction_name = rec
            .entity_type
            .jurisdiction
            .as_ref()
            .map_or("Nevada", |j| j.name.as_str());
        let jur = jurisdiction::Entity::find()
            .filter(jurisdiction::Column::Name.eq(jurisdiction_name))
            .one(db)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("Entity.yaml references unknown jurisdiction {jurisdiction_name:?}")
            })?;
        if entities::Entity::find()
            .filter(entities::Column::Name.eq(rec.name.clone()))
            .filter(entities::Column::EntityTypeId.eq(et.id))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        entities::ActiveModel {
            name: ActiveValue::Set(rec.name),
            entity_type_id: ActiveValue::Set(et.id),
            jurisdiction_id: ActiveValue::Set(jur.id),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.entities_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct PersonRec {
    email: String,
    name: String,
    #[serde(default)]
    profile_image_url: Option<String>,
}

async fn seed_persons(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<PersonRec>(canonical::PERSON, "Person.yaml")? {
        if person::Entity::find()
            .filter(person::Column::Email.eq(rec.email.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        person::ActiveModel {
            name: ActiveValue::Set(rec.name),
            email: ActiveValue::Set(rec.email),
            oidc_subject: ActiveValue::Set(None),
            role: ActiveValue::Set(person::Role::Client),
            profile_image_url: ActiveValue::Set(rec.profile_image_url),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.persons_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct TestimonialRec {
    project: ProjectCodenameRef,
    person: PersonEmailRef,
    #[serde(default)]
    product_code: Option<String>,
    quote: String,
    #[serde(default)]
    attribution_label: Option<String>,
    #[serde(default)]
    consented_at: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    display_order: i32,
}

async fn seed_testimonials(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<TestimonialRec>(canonical::TESTIMONIAL, "Testimonial.yaml")? {
        let Some(project) = project::Entity::find()
            .filter(project::Column::Name.eq(rec.project.codename.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        let Some(person) = person::Entity::find()
            .filter(person::Column::Email.eq(rec.person.email.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        if testimonial::Entity::find()
            .filter(testimonial::Column::ProjectId.eq(project.id))
            .filter(testimonial::Column::PersonId.eq(person.id))
            .filter(testimonial::Column::Quote.eq(rec.quote.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        testimonial::ActiveModel {
            project_id: ActiveValue::Set(project.id),
            person_id: ActiveValue::Set(person.id),
            product_code: ActiveValue::Set(rec.product_code),
            quote: ActiveValue::Set(rec.quote),
            attribution_label: ActiveValue::Set(rec.attribution_label),
            consented_at: ActiveValue::Set(rec.consented_at),
            published_at: ActiveValue::Set(rec.published_at),
            display_order: ActiveValue::Set(rec.display_order),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.testimonials_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct UserRec {
    person: PersonEmailRef,
    role: String,
}

#[derive(Debug, Deserialize)]
struct PersonEmailRef {
    email: String,
}

/// Firm-domain convention for seeded role assignments: any
/// `staff` or `admin` row must use a **lowercase** `@neonlaw.com`
/// email. Personal-domain emails belong on `client` rows. The
/// lowercase requirement is exact-match — the seed is the canonical
/// source of truth and mixed-case ("Nick@NeonLaw.com") breaks the
/// email-as-foreign-key pattern used throughout the workspace.
/// See `docs/access-model.md`.
fn require_firm_domain(email: &str, role: person::Role) -> anyhow::Result<()> {
    use person::Role;
    if !matches!(role, Role::Staff | Role::Admin) {
        return Ok(());
    }
    if email != email.to_ascii_lowercase() {
        anyhow::bail!(
            "User.yaml: {role:?} seed for {email:?} must be lowercase \
             (see docs/access-model.md)",
        );
    }
    if !email.ends_with("@neonlaw.com") {
        anyhow::bail!(
            "User.yaml: {role:?} seed for {email:?} violates the firm-domain \
             convention — staff/admin records must use an @neonlaw.com email \
             (see docs/access-model.md)",
        );
    }
    Ok(())
}

/// User.yaml carries a `role` per person; the `users` table doesn't
/// exist as its own entity here — the system-wide tier lives on
/// `persons.role`. Resolve each user record by email, parse the role
/// token, and update the row if the requested tier is higher than
/// what's already stored. The ladder is admin > staff > client.
async fn seed_user_roles(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    use person::Role;

    fn parse_role_token(s: &str) -> Role {
        match s {
            "admin" => Role::Admin,
            "staff" => Role::Staff,
            _ => Role::Client,
        }
    }
    fn rank(r: Role) -> u8 {
        match r {
            Role::Client => 0,
            Role::Staff => 1,
            Role::Admin => 2,
        }
    }

    for rec in parse::<UserRec>(canonical::USER, "User.yaml")? {
        let requested = parse_role_token(&rec.role);
        require_firm_domain(&rec.person.email, requested)?;
        let Some(p) = person::Entity::find()
            .filter(person::Column::Email.eq(rec.person.email.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        if rank(p.role) >= rank(requested) {
            continue;
        }
        let mut am = p.into_active_model();
        am.role = ActiveValue::Set(requested);
        am.update(db).await?;
        report.persons_updated += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ProjectRec {
    codename: String,
}

async fn seed_projects(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    // Every project is opened against a pre-existing entity and carries
    // two required DRIs (see docs/glossary.md). The firm's own examples
    // matter tracks the firm's entity and names Nick on both sides — it is
    // the firm's internal demo, so the firm is its own client. The DRI
    // columns are authoritative; we do not mirror them into the
    // `person_project_roles` participation ledger.
    let entity_id = entities::Entity::find()
        .filter(entities::Column::Name.eq("Shook Law PLLC"))
        .one(db)
        .await?
        .map(|e| e.id)
        .ok_or_else(|| anyhow::anyhow!("seed: entity `Shook Law PLLC` must be seeded first"))?;
    let nick_id = person::Entity::find()
        .filter(person::Column::Email.eq("nick@neonlaw.com"))
        .one(db)
        .await?
        .map(|p| p.id)
        .ok_or_else(|| anyhow::anyhow!("seed: person `nick@neonlaw.com` must be seeded first"))?;

    for rec in parse::<ProjectRec>(canonical::PROJECT, "Project.yaml")? {
        if project::Entity::find()
            .filter(project::Column::Name.eq(rec.codename.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        project::ActiveModel {
            name: ActiveValue::Set(rec.codename),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(entity_id),
            staff_dri_person_id: ActiveValue::Set(Some(nick_id)),
            client_dri_person_id: ActiveValue::Set(Some(nick_id)),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.projects_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct GitRepoRec {
    repository_name: String,
}

async fn seed_git_repositories(
    db: &DatabaseConnection,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    for rec in parse::<GitRepoRec>(canonical::GIT_REPOSITORY, "GitRepository.yaml")? {
        let remote_hash = remote_hash(&rec.repository_name);
        if git_repository::Entity::find()
            .filter(git_repository::Column::RemoteHash.eq(remote_hash.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        git_repository::ActiveModel {
            remote_hash: ActiveValue::Set(remote_hash),
            last_commit_sha: ActiveValue::Set("0".repeat(40)),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.git_repositories_inserted += 1;
    }
    Ok(())
}

fn remote_hash(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(name.as_bytes());
    h.finalize().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[derive(Debug, Deserialize)]
struct QuestionRec {
    code: String,
    prompt: String,
    #[serde(default)]
    question_type: Option<String>,
    /// `staff` | `client` | `both` — which side of the intake sees this
    /// question. Defaults `both` when the YAML omits it.
    #[serde(default)]
    audience: Option<String>,
    // `help_text` / `choices` exist in the YAML but the schema has
    // no column for them — silently dropped.
}

async fn seed_questions(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<QuestionRec>(canonical::QUESTION, "Question.yaml")? {
        if question::Entity::find()
            .filter(question::Column::Code.eq(rec.code.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        question::ActiveModel {
            code: ActiveValue::Set(rec.code),
            prompt: ActiveValue::Set(rec.prompt),
            answer_type: ActiveValue::Set(rec.question_type.unwrap_or_else(|| "string".into())),
            audience: ActiveValue::Set(
                rec.audience
                    .unwrap_or_else(|| question::AUDIENCE_BOTH.to_string()),
            ),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.questions_inserted += 1;
    }
    Ok(())
}

/// A question's canonical definition narrowed to its `code` and the
/// optional `choices:` block — the slice of `Question.yaml` the
/// [`question_choices`] reader needs. Every other field (prompt,
/// help_text, audience, …) is ignored.
#[derive(Debug, Deserialize)]
struct ChoiceQuestionRec {
    code: String,
    #[serde(default)]
    choices: Option<serde_yaml::Mapping>,
}

/// The attorney-reviewed answer choices for a `radio` question, as
/// ordered `(value, label)` pairs read from the canonical
/// `Question.yaml`. Returns an empty vec for a question with no
/// `choices:` block (every non-`radio` question) or an unknown code.
///
/// Choices live in the question's canonical seed definition but have no
/// column on the `questions` table — they are presentational, dropped at
/// seed time (see [`QuestionRec`]). The one surface that needs them at
/// runtime, the CLI questionnaire walker's machine-readable step
/// (`GET …/step?format=json`), reads them here rather than from the row,
/// so the choices a terminal shows are the same bytes the seed defines.
#[must_use]
pub fn question_choices(code: &str) -> Vec<(String, String)> {
    let code = code.split_once("__").map_or(code, |(prefix, _)| prefix);
    let Ok(parsed) = serde_yaml::from_str::<Records<ChoiceQuestionRec>>(canonical::QUESTION) else {
        return Vec::new();
    };
    parsed
        .records
        .into_iter()
        .find(|r| r.code == code)
        .and_then(|r| r.choices)
        .map(|m| {
            m.into_iter()
                .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct QuestionTranslationRec {
    code: String,
    locale: String,
    prompt: String,
    #[serde(default)]
    help_text: Option<String>,
}

/// Seed attorney-reviewed localized question prompts. Each record is
/// resolved to its Question by `code`; idempotent on `(question, locale)`.
async fn seed_question_translations(
    db: &DatabaseConnection,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    use crate::entity::question_translation;
    for rec in parse::<QuestionTranslationRec>(
        canonical::QUESTION_TRANSLATION,
        "QuestionTranslation.yaml",
    )? {
        let Some(q) = question::Entity::find()
            .filter(question::Column::Code.eq(rec.code.clone()))
            .one(db)
            .await?
        else {
            // The base question must be seeded first; skip orphans
            // rather than failing the whole seed.
            continue;
        };
        if question_translation::Entity::find()
            .filter(question_translation::Column::QuestionId.eq(q.id))
            .filter(question_translation::Column::Locale.eq(rec.locale.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        question_translation::ActiveModel {
            question_id: ActiveValue::Set(q.id),
            locale: ActiveValue::Set(rec.locale),
            prompt: ActiveValue::Set(rec.prompt),
            help_text: ActiveValue::Set(rec.help_text),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.question_translations_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct MailroomRec {
    name: String,
}

/// `mailrooms.address_id` is NOT NULL; the YAML carries no separate
/// address for the mailroom itself. We synthesize a placeholder
/// address per mailroom so the FK satisfies — flagged with a
/// `(via mailroom)` line1 so it's obvious in row dumps.
async fn seed_mailrooms(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<MailroomRec>(canonical::MAILROOM, "Mailroom.yaml")? {
        if mailroom::Entity::find()
            .filter(mailroom::Column::Name.eq(rec.name.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        let addr = address::ActiveModel {
            person_id: ActiveValue::Set(None),
            entity_id: ActiveValue::Set(None),
            line1: ActiveValue::Set(format!("(via mailroom: {})", rec.name)),
            line2: ActiveValue::Set(None),
            city: ActiveValue::Set(String::new()),
            region: ActiveValue::Set(String::new()),
            postal_code: ActiveValue::Set(String::new()),
            country: ActiveValue::Set(String::new()),
            ..Default::default()
        }
        .insert(db)
        .await?;
        mailroom::ActiveModel {
            name: ActiveValue::Set(rec.name),
            address_id: ActiveValue::Set(addr.id),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.mailrooms_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct AddressRec {
    entity: EntityNameRef,
    street: String,
    city: String,
    state: String,
    country: String,
    zip: String,
}

#[derive(Debug, Deserialize)]
struct EntityNameRef {
    name: String,
}

async fn seed_addresses(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<AddressRec>(canonical::ADDRESS, "Address.yaml")? {
        let Some(ent) = entities::Entity::find()
            .filter(entities::Column::Name.eq(rec.entity.name.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        if address::Entity::find()
            .filter(address::Column::EntityId.eq(ent.id))
            .filter(address::Column::PostalCode.eq(rec.zip.clone()))
            .filter(address::Column::Line1.eq(rec.street.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        address::ActiveModel {
            person_id: ActiveValue::Set(None),
            entity_id: ActiveValue::Set(Some(ent.id)),
            line1: ActiveValue::Set(rec.street),
            line2: ActiveValue::Set(None),
            city: ActiveValue::Set(rec.city),
            region: ActiveValue::Set(rec.state),
            postal_code: ActiveValue::Set(rec.zip),
            country: ActiveValue::Set(rec.country),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.addresses_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct LetterRec {
    subject: String,
    sender: String,
    mailroom: MailroomNameRef,
}

#[derive(Debug, Deserialize)]
struct MailroomNameRef {
    name: String,
}

async fn seed_letters(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<LetterRec>(canonical::LETTER, "Letter.yaml")? {
        let Some(mr) = mailroom::Entity::find()
            .filter(mailroom::Column::Name.eq(rec.mailroom.name.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        if letter::Entity::find()
            .filter(letter::Column::MailroomId.eq(mr.id))
            .filter(letter::Column::Summary.eq(rec.subject.clone()))
            .filter(letter::Column::Sender.eq(rec.sender.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        letter::ActiveModel {
            mailroom_id: ActiveValue::Set(mr.id),
            direction: ActiveValue::Set("incoming".into()),
            sender: ActiveValue::Set(rec.sender),
            recipient: ActiveValue::Set(rec.mailroom.name.clone()),
            summary: ActiveValue::Set(rec.subject),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.letters_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct AnswerRec {
    question_code: String,
    person_email: String,
    value: String,
}

async fn seed_answers(db: &DatabaseConnection, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<AnswerRec>(canonical::ANSWER, "Answer.yaml")? {
        let Some(q) = question::Entity::find()
            .filter(question::Column::Code.eq(rec.question_code.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        let Some(p) = person::Entity::find()
            .filter(person::Column::Email.eq(rec.person_email.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        // Answers are append-only at every walker write site; this guard
        // is only the seed pass's own idempotency (`seed_canonical`
        // re-running inserts no new rows), keyed on the fixture's identity
        // — not a domain dedup. These fixtures are person-scoped (no
        // Notation behind them), so `notation_id` and `state_name` stay
        // null.
        if answer::Entity::find()
            .filter(answer::Column::QuestionId.eq(q.id))
            .filter(answer::Column::PersonId.eq(p.id))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        answer::ActiveModel {
            question_id: ActiveValue::Set(q.id),
            person_id: ActiveValue::Set(p.id),
            value: ActiveValue::Set(answer::primitive(&rec.value)),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.answers_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct PersonEntityRoleRec {
    person: PersonEmailRef,
    entity: EntityNameRef,
    role: String,
}

async fn seed_person_entity_roles(
    db: &DatabaseConnection,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    for rec in parse::<PersonEntityRoleRec>(canonical::PERSON_ENTITY_ROLE, "PersonEntityRole.yaml")?
    {
        let Some(p) = person::Entity::find()
            .filter(person::Column::Email.eq(rec.person.email.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        let Some(e) = entities::Entity::find()
            .filter(entities::Column::Name.eq(rec.entity.name.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        if person_entity_role::Entity::find()
            .filter(person_entity_role::Column::PersonId.eq(p.id))
            .filter(person_entity_role::Column::EntityId.eq(e.id))
            .filter(person_entity_role::Column::Role.eq(rec.role.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        person_entity_role::ActiveModel {
            person_id: ActiveValue::Set(p.id),
            entity_id: ActiveValue::Set(e.id),
            role: ActiveValue::Set(rec.role),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.person_entity_roles_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct PersonProjectRoleRec {
    person: PersonEmailRef,
    project: ProjectCodenameRef,
    role: String,
}

#[derive(Debug, Deserialize)]
struct ProjectCodenameRef {
    codename: String,
}

async fn seed_person_project_roles(
    db: &DatabaseConnection,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    for rec in
        parse::<PersonProjectRoleRec>(canonical::PERSON_PROJECT_ROLE, "PersonProjectRole.yaml")?
    {
        let Some(p) = person::Entity::find()
            .filter(person::Column::Email.eq(rec.person.email.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        let Some(pr) = project::Entity::find()
            .filter(project::Column::Name.eq(rec.project.codename.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        if person_project_role::Entity::find()
            .filter(person_project_role::Column::PersonId.eq(p.id))
            .filter(person_project_role::Column::ProjectId.eq(pr.id))
            .filter(person_project_role::Column::Participation.eq(rec.role.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        person_project_role::ActiveModel {
            person_id: ActiveValue::Set(p.id),
            project_id: ActiveValue::Set(pr.id),
            participation: ActiveValue::Set(rec.role),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.person_project_roles_inserted += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct BillingProfileRec {
    #[serde(default)]
    provider: Option<String>,
    external_contact_id: String,
    entity: EntityNameRef,
}

/// Returns external_contact_id → billing_profile_id so the invoice
/// loader can resolve invoices without re-walking the YAML.
async fn seed_billing_profiles(
    db: &DatabaseConnection,
    report: &mut SeedReport,
) -> anyhow::Result<BTreeMap<String, Uuid>> {
    let mut by_external: BTreeMap<String, Uuid> = BTreeMap::new();
    for rec in parse::<BillingProfileRec>(
        canonical::ENTITY_BILLING_PROFILE,
        "EntityBillingProfile.yaml",
    )? {
        let Some(e) = entities::Entity::find()
            .filter(entities::Column::Name.eq(rec.entity.name.clone()))
            .one(db)
            .await?
        else {
            continue;
        };
        let provider = rec.provider.unwrap_or_else(|| "unknown".into());
        let billing_email = format!("billing+{provider}@{}.example", e.id);
        let existing = entity_billing_profile::Entity::find()
            .filter(entity_billing_profile::Column::EntityId.eq(e.id))
            .one(db)
            .await?;
        let profile_id = if let Some(p) = existing {
            p.id
        } else {
            let row = entity_billing_profile::ActiveModel {
                entity_id: ActiveValue::Set(e.id),
                billing_email: ActiveValue::Set(billing_email),
                billing_address_id: ActiveValue::Set(None),
                ..Default::default()
            }
            .insert(db)
            .await?;
            report.entity_billing_profiles_inserted += 1;
            row.id
        };
        by_external.insert(rec.external_contact_id, profile_id);
    }
    Ok(by_external)
}

#[derive(Debug, Deserialize)]
struct InvoiceRec {
    billing_profile_external_contact_id: String,
    external_invoice_id: String,
    invoice_number: String,
    status: String,
    currency_code: String,
    total: String,
}

/// Returns external_invoice_id → invoice_id so line items can resolve.
async fn seed_invoices(
    db: &DatabaseConnection,
    report: &mut SeedReport,
    by_external_contact: &BTreeMap<String, Uuid>,
) -> anyhow::Result<BTreeMap<String, Uuid>> {
    let mut by_external: BTreeMap<String, Uuid> = BTreeMap::new();
    for rec in parse::<InvoiceRec>(canonical::INVOICE, "Invoice.yaml")? {
        let Some(&profile_id) = by_external_contact.get(&rec.billing_profile_external_contact_id)
        else {
            continue;
        };
        let invoice_id = if let Some(existing) = invoice::Entity::find()
            .filter(invoice::Column::Number.eq(rec.invoice_number.clone()))
            .one(db)
            .await?
        {
            existing.id
        } else {
            let total_cents = parse_decimal_to_cents(&rec.total);
            let row = invoice::ActiveModel {
                entity_billing_profile_id: ActiveValue::Set(profile_id),
                number: ActiveValue::Set(rec.invoice_number),
                status: ActiveValue::Set(rec.status),
                total_cents: ActiveValue::Set(total_cents),
                currency: ActiveValue::Set(rec.currency_code),
                ..Default::default()
            }
            .insert(db)
            .await?;
            report.invoices_inserted += 1;
            row.id
        };
        by_external.insert(rec.external_invoice_id, invoice_id);
    }
    Ok(by_external)
}

#[derive(Debug, Deserialize)]
struct InvoiceLineItemRec {
    external_invoice_id: String,
    description: String,
    quantity: String,
    unit_amount: String,
}

async fn seed_invoice_line_items(
    db: &DatabaseConnection,
    report: &mut SeedReport,
    by_external_invoice: &BTreeMap<String, Uuid>,
) -> anyhow::Result<()> {
    for rec in parse::<InvoiceLineItemRec>(canonical::INVOICE_LINE_ITEM, "InvoiceLineItem.yaml")? {
        let Some(&invoice_id) = by_external_invoice.get(&rec.external_invoice_id) else {
            continue;
        };
        if invoice_line_item::Entity::find()
            .filter(invoice_line_item::Column::InvoiceId.eq(invoice_id))
            .filter(invoice_line_item::Column::Description.eq(rec.description.clone()))
            .one(db)
            .await?
            .is_some()
        {
            continue;
        }
        let qty_cents = parse_decimal_to_cents(&rec.quantity);
        let quantity = i32::try_from(qty_cents / 100).unwrap_or(1).max(1);
        invoice_line_item::ActiveModel {
            invoice_id: ActiveValue::Set(invoice_id),
            description: ActiveValue::Set(rec.description),
            quantity: ActiveValue::Set(quantity),
            unit_price_cents: ActiveValue::Set(parse_decimal_to_cents(&rec.unit_amount)),
            ..Default::default()
        }
        .insert(db)
        .await?;
        report.invoice_line_items_inserted += 1;
    }
    Ok(())
}

/// Convert a non-negative decimal string like `"1000.0000"` to cents
/// (`100_000`). Truncates beyond two fractional digits — the seed
/// YAMLs are exact to the cent so this loses no information.
fn parse_decimal_to_cents(s: &str) -> i64 {
    let cleaned = s.trim().trim_start_matches('+');
    let (sign, body): (i64, &str) = match cleaned.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, cleaned),
    };
    let (whole, frac) = body.split_once('.').unwrap_or((body, ""));
    let whole: i64 = whole.parse().unwrap_or(0);
    let frac_padded: String = frac.chars().chain(std::iter::repeat('0')).take(2).collect();
    let frac_val: i64 = frac_padded[..2].parse().unwrap_or(0);
    sign * (whole * 100 + frac_val)
}

#[cfg(test)]
mod tests {
    use super::seed_canonical;
    use crate::entity::{jurisdiction, person, question, template};
    use crate::test_support::pg;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    /// A filesystem-backed storage at a fixed path so the bytes a seed
    /// writes are readable by a later `templates::body` call in the same
    /// test — blobs are content-addressed, so sharing the dir across
    /// tests is safe (identical bytes dedup).
    async fn fs_storage() -> std::sync::Arc<dyn cloud::StorageService> {
        std::sync::Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join("navigator-seed-test-storage"))
                .await
                .expect("temp FsStorage"),
        )
    }

    #[tokio::test]
    async fn seeds_full_question_set() {
        let db = pg().await;
        let report = seed_canonical(&db, &fs_storage().await)
            .await
            .expect("seed");
        // The canonical question catalog grows whenever a bundled
        // template adds a questionnaire prompt (the retainer + closing
        // walkers, then Nautilus, Northstar, the i18n set, …), so this
        // is a *floor*, not an exact count — 41 distinct codes at the
        // time of writing. A floor tolerates that growth while still
        // catching a regression that drops a question; the code-presence
        // checks below pin the load-bearing prompts by name.
        let qs = question::Entity::find().all(&db).await.unwrap();
        assert!(
            qs.len() >= 41,
            "expected at least 41 distinct questions, found {}",
            qs.len()
        );
        assert!(qs.iter().any(|q| q.code == "personal_name"));
        assert!(qs.iter().any(|q| q.code == "staff_review"));
        assert!(qs.iter().any(|q| q.code == "client_name"));
        assert!(qs.iter().any(|q| q.code == "product_description"));
        assert!(qs.iter().any(|q| q.code == "matter_summary"));
        assert!(report.questions_inserted >= 41);
    }

    #[tokio::test]
    async fn seeds_full_jurisdiction_set() {
        let db = pg().await;
        seed_canonical(&db, &fs_storage().await)
            .await
            .expect("seed");
        let js = jurisdiction::Entity::find().all(&db).await.unwrap();
        // 50 states + DC + United States + Germany = 53 rows.
        assert_eq!(js.len(), 53);
        let codes: Vec<&str> = js.iter().map(|j| j.code.as_str()).collect();
        for code in ["NV", "CA", "NY", "TX", "WY", "DC", "US", "GMBH"] {
            assert!(codes.contains(&code), "expected `{code}` in jurisdictions");
        }
        // `jurisdiction_type` is reconciled with the seed: states are
        // `state`, the federal sovereigns are `country`.
        let by_code = |c: &str| js.iter().find(|j| j.code == c).unwrap();
        assert_eq!(by_code("NV").jurisdiction_type, "state");
        assert_eq!(by_code("US").jurisdiction_type, "country");
        assert_eq!(by_code("GMBH").jurisdiction_type, "country");
    }

    #[tokio::test]
    async fn seeds_the_bundled_template_catalog() {
        let db = pg().await;
        let report = seed_canonical(&db, &fs_storage().await)
            .await
            .expect("seed");
        assert_eq!(
            report.templates_inserted, 40,
            "expected the full bundled template catalog to be inserted on first pass"
        );
        // Spot-check templates from across the catalog so a dropped
        // `include_str!` entry is caught, not just the retainer.
        for code in [
            "onboarding__retainer",
            "closing__letter",
            "trusts__nevada",
            "will__simple",
            "ca__llc_operating_agreement",
            "us__form_990",
            "services__contract_review",
            "employment__nonprofit_w2",
            "contractor__nonprofit_1099",
        ] {
            assert!(
                template::Entity::find()
                    .filter(template::Column::Code.eq(code))
                    .one(&db)
                    .await
                    .unwrap()
                    .is_some(),
                "expected bundled template `{code}` to be seeded"
            );
        }
        let tmpl = template::Entity::find()
            .filter(template::Column::Code.eq("onboarding__retainer"))
            .one(&db)
            .await
            .unwrap()
            .expect("template row");
        assert_eq!(tmpl.title, "Retainer Agreement");
        assert_eq!(tmpl.respondent_type, "person_and_entity");
        assert!(tmpl.project_id.is_none(), "bundled templates are shared");
        // The body now lives in a blob — fetch it via the storage
        // accessor. Just the markdown body, no frontmatter, so the
        // renderer's `{{client_name}}` interpolation finds its targets.
        let body = crate::templates::body(&db, &fs_storage().await, &tmpl)
            .await
            .expect("template body in storage");
        assert!(
            !body.starts_with("---"),
            "body should not include the YAML frontmatter; got {:?}",
            &body[..body.len().min(20)]
        );
        assert!(body.contains("{{client_name}}"));
        assert!(body.contains("{{client_email}}"));
        assert!(body.contains("{{project_name}}"));
        assert!(body.contains("{{product_description}}"));
    }

    #[tokio::test]
    async fn template_seeder_is_idempotent_on_second_pass() {
        let db = pg().await;
        let first = seed_canonical(&db, &fs_storage().await).await.unwrap();
        let second = seed_canonical(&db, &fs_storage().await).await.unwrap();
        assert_eq!(first.templates_inserted, 40);
        assert_eq!(
            second.templates_inserted, 0,
            "second pass must skip every existing template"
        );
        let count = template::Entity::find()
            .filter(template::Column::Code.eq("onboarding__retainer"))
            .all(&db)
            .await
            .unwrap()
            .len();
        assert_eq!(count, 1, "exactly one retainer template row");
    }

    /// Every service-specific retainer carries the three load-bearing
    /// elements the firm requires: the JAMS arbitration clause (forum
    /// selection only, with the non-waivable fee-arbitration carve-out and
    /// the independent-counsel sentence — never a liability limitation),
    /// the `support@neonlaw.com` acknowledgement, and the practice-area
    /// ethics reading naming the right RPC(s). The shared phrases are
    /// asserted byte-identically across every retainer so the once-reviewed
    /// arbitration wording cannot drift between retainers.
    #[tokio::test]
    async fn service_retainers_carry_arbitration_support_and_ethics() {
        let db = pg().await;
        seed_canonical(&db, &fs_storage().await).await.unwrap();
        let storage = fs_storage().await;

        // Distinctive phrases from the two shared clauses — identical in
        // every retainer. Each sits within a single wrapped line so a
        // literal `contains` is exact.
        let shared = [
            "binding arbitration administered by **JAMS**",
            "seated in **Reno, Nevada**",
            "limit, cap, or waive the Firm's responsibility for its own work",
            "right to consult independent counsel of your own choosing before you agree to it",
            "Mandatory Fee Arbitration Act",
            "Washington State Bar Association",
            "Email to **support@neonlaw.com** is the best and primary way to reach the Firm",
            "{{custom_clauses}}",
            "{{client.signature}}",
            "{{firm.signature}}",
        ];

        // Each retainer's practice-area ethics reading must name its RPCs.
        let retainers = [
            ("onboarding__retainer_nest", vec!["RPC 1.13", "RPC 1.7"]),
            (
                "onboarding__retainer_nexus",
                vec!["RPC 1.13", "RPC 1.6", "RPC 1.8(a)"],
            ),
            (
                "onboarding__retainer_northstar",
                vec!["RPC 1.7", "RPC 1.6", "RPC 1.14", "RPC 1.8(c)"],
            ),
            (
                "onboarding__retainer_nautilus",
                vec!["RPC 1.2(c)", "RPC 1.5", "RPC 7.1"],
            ),
            ("onboarding__retainer_nook", vec!["RPC 1.7", "RPC 4.3"]),
            (
                "onboarding__retainer_litigation",
                vec!["RPC 1.5(c)", "RPC 1.8(i)", "RPC 3.1", "RPC 1.7"],
            ),
            (
                "onboarding__retainer_nerd",
                vec!["RPC 2.3", "RPC 3.3", "RPC 3.4", "RPC 1.2(c)", "RPC 1.6"],
            ),
            (
                "onboarding__retainer_node",
                vec!["RPC 2.3", "RPC 1.6", "RPC 1.2(c)"],
            ),
            (
                "onboarding__retainer_newleaf",
                vec!["RPC 1.7", "RPC 1.6", "RPC 1.2(c)"],
            ),
            (
                "onboarding__retainer_namesake",
                vec!["RPC 1.1", "RPC 1.2(c)", "RPC 1.4"],
            ),
            (
                "onboarding__retainer_nucleus",
                vec!["RPC 1.13", "RPC 1.7", "RPC 1.8(a)", "RPC 1.6"],
            ),
        ];

        for (code, rpcs) in retainers {
            let tmpl = template::Entity::find()
                .filter(template::Column::Code.eq(code))
                .one(&db)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("{code} seeded"));
            let body = crate::templates::body(&db, &storage, &tmpl)
                .await
                .expect("retainer body");
            for phrase in shared {
                assert!(
                    body.contains(phrase),
                    "{code} must carry the shared clause phrase {phrase:?}"
                );
            }
            for rpc in &rpcs {
                assert!(
                    body.contains(rpc),
                    "{code}'s ethics reading must name {rpc}"
                );
            }
            // The arbitration clause must not read as a liability waiver
            // (RPC 1.8(h)). Guard against a regression that re-introduces
            // limiting language.
            for forbidden in ["limit our liability", "waive any claim against the Firm"] {
                assert!(
                    !body.contains(forbidden),
                    "{code} must not limit malpractice liability ({forbidden:?})"
                );
            }
        }
    }

    #[tokio::test]
    async fn seed_is_idempotent() {
        let db = pg().await;
        let first = seed_canonical(&db, &fs_storage().await)
            .await
            .expect("seed 1");
        let second = seed_canonical(&db, &fs_storage().await)
            .await
            .expect("seed 2");
        assert_eq!(second.questions_inserted, 0);
        assert_eq!(second.jurisdictions_inserted, 0);
        assert_eq!(second.persons_inserted, 0);
        assert_eq!(second.products_inserted, 0);
        assert!(first.questions_inserted > 0);
        assert_eq!(
            first.products_inserted, 11,
            "the eleven-product catalog seeds on the first pass"
        );
    }

    #[tokio::test]
    async fn seeds_attorney_credentials_with_correct_numbers() {
        use crate::entity::credential;
        let db = pg().await;
        let report = seed_canonical(&db, &fs_storage().await)
            .await
            .expect("seed");
        let nick = person::Entity::find()
            .filter(person::Column::Email.eq("nick@neonlaw.com"))
            .one(&db)
            .await
            .unwrap()
            .expect("nick exists");
        let creds = credential::Entity::find()
            .filter(credential::Column::PersonId.eq(nick.id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(creds.len(), 3, "expected NV + CA + WA admissions");
        // The state bar numbers are public-record disclosures; pin them
        // explicitly so a seed YAML edit can't silently change the
        // attorney advertising disclosure rendered on the firm site.
        let mut by_juris: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for c in &creds {
            let j = jurisdiction::Entity::find_by_id(c.jurisdiction_id)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
            by_juris.insert(j.code, c.license_number.clone());
        }
        assert_eq!(by_juris.get("NV").map(String::as_str), Some("13400"));
        assert_eq!(by_juris.get("CA").map(String::as_str), Some("337252"));
        assert_eq!(by_juris.get("WA").map(String::as_str), Some("63446"));
        assert_eq!(report.credentials_inserted, 3);
    }

    #[tokio::test]
    async fn user_role_lifts_persons_to_admin() {
        let db = pg().await;
        seed_canonical(&db, &fs_storage().await)
            .await
            .expect("seed");
        let nick = person::Entity::find()
            .filter(person::Column::Email.eq("nick@neonlaw.com"))
            .one(&db)
            .await
            .unwrap()
            .expect("nick exists");
        assert_eq!(nick.role, person::Role::Admin);
    }

    #[test]
    fn firm_domain_convention_accepts_lowercase_neonlaw_com_for_staff_and_admin() {
        use super::require_firm_domain;
        use person::Role;
        assert!(require_firm_domain("nick@neonlaw.com", Role::Admin).is_ok());
        assert!(require_firm_domain("staff@neonlaw.com", Role::Staff).is_ok());
    }

    #[test]
    fn firm_domain_convention_rejects_mixed_case_staff_and_admin_emails() {
        use super::require_firm_domain;
        use person::Role;
        let err = require_firm_domain("Nick@NeonLaw.com", Role::Admin).unwrap_err();
        assert!(
            err.to_string().contains("lowercase"),
            "error should call out lowercase, got: {err}",
        );
        assert!(require_firm_domain("nick@NEONLAW.COM", Role::Admin).is_err());
    }

    #[test]
    fn firm_domain_convention_allows_any_domain_for_client() {
        use super::require_firm_domain;
        use person::Role;
        assert!(require_firm_domain("libra@example.com", Role::Client).is_ok());
        // Client rows aren't held to lowercase here; that's a normalization
        // concern for the persons table, not the seed convention.
        assert!(require_firm_domain("Libra@Example.com", Role::Client).is_ok());
    }

    #[test]
    fn question_choices_reads_ordered_radio_choices_from_the_canonical_seed() {
        use super::question_choices;
        // `management_structure` is the `nv__llc_formation` radio; its
        // choices must come back in YAML order (members before managers),
        // value-keyed, so a terminal renders them like the web walker would.
        let choices = question_choices("management_structure");
        assert_eq!(
            choices,
            vec![
                (
                    "members".to_string(),
                    "Managed by its members — the owners".to_string(),
                ),
                (
                    "managers".to_string(),
                    "Managed by appointed managers".to_string(),
                ),
            ],
        );
        // A free-text question carries no choices; neither does an unknown
        // code. Both answer with an empty vec rather than panicking.
        assert!(question_choices("entity_name").is_empty());
        assert!(question_choices("no_such_question_code").is_empty());
    }

    #[test]
    fn firm_domain_convention_rejects_off_domain_staff_and_admin_seeds() {
        use super::require_firm_domain;
        use person::Role;
        let err = require_firm_domain("libra@example.com", Role::Staff).unwrap_err();
        assert!(
            err.to_string().contains("@neonlaw.com"),
            "error should mention the firm domain, got: {err}",
        );
        assert!(require_firm_domain("nick@gmail.com", Role::Admin).is_err());
    }
}
