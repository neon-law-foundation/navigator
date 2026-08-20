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
//! - a brand binary calls [`seed_environment`] after migrations on startup,
//!   naming the brand it serves so that brand's own seeds apply too.
//!
//! Seeds come in three layers — canonical, brand, and the simulated-matter
//! fixture. [`seed_environment`] documents which reaches a deployment holding
//! real client files and why.

use anyhow::Context as _;

use crate::jurisdictions::{self, NewJurisdiction};
use crate::surreal::SurrealDb;
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
    pub notations_inserted: usize,
    pub assets_inserted: usize,
    pub communications_inserted: usize,
    pub git_repositories_inserted: usize,
    pub questions_inserted: usize,
    pub mailrooms_inserted: usize,
    pub addresses_inserted: usize,
    pub letters_inserted: usize,
    pub answers_inserted: usize,
    pub person_entity_roles_inserted: usize,
    pub person_project_roles_inserted: usize,
    pub credentials_inserted: usize,
    pub templates_inserted: usize,
    pub testimonials_inserted: usize,
    /// Glossary terms materialized from `docs/glossary.md`. Reference
    /// data: environment-blind, upserted by slug on every boot.
    pub glossary_terms_written: usize,
}

impl SeedReport {
    /// One-line summary suitable for CLI output. Reports every entity
    /// even when zero so re-runs make it visible that the pass was
    /// a no-op.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "Seeded: {} jurisdictions, {} entity_types, {} entities, {} persons \
             (+{} role updates), {} projects, {} notations, {} assets, {} communications, \
             {} git_repos, {} questions, \
             {} mailrooms, {} addresses, {} letters, {} answers, \
             {} person_entity_roles, {} person_project_roles, {} credentials, \
             {} templates, {} testimonials, {} glossary_terms.",
            self.jurisdictions_inserted,
            self.entity_types_inserted,
            self.entities_inserted,
            self.persons_inserted,
            self.persons_updated,
            self.projects_inserted,
            self.notations_inserted,
            self.assets_inserted,
            self.communications_inserted,
            self.git_repositories_inserted,
            self.questions_inserted,
            self.mailrooms_inserted,
            self.addresses_inserted,
            self.letters_inserted,
            self.answers_inserted,
            self.person_entity_roles_inserted,
            self.person_project_roles_inserted,
            self.credentials_inserted,
            self.templates_inserted,
            self.testimonials_inserted,
            self.glossary_terms_written,
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

/// The firm Entity that anchors the canonical seed. `Entity.yaml` re-creates
/// this row by exact name on every boot, so every deployment carries it.
/// `web` reads this to keep the row's delete and rename guards aligned with
/// the name the seed looks up.
///
/// This is the professional LLC a client engages — the entity of record behind
/// the Neon Law mark, which is why it is the row the application refuses to
/// delete. `Neon Law` is what the site is signed with; `Shook Law PLLC` is the
/// legal person that renders the services and owns the mark, and only a legal
/// person can anchor a client relationship. It is not the copyright holder —
/// the software is the Neon Law Foundation's, and the two are deliberately
/// different organizations. Moving this name is a data
/// change as well as a code one: `seed_entities` reconciles
/// `entities.firm_anchor_key` on every boot, because the delete guard reads
/// that column and not the name.
pub const FIRM_ENTITY_NAME: &str = "Shook Law PLLC";

/// Which brand's own seeds a boot applies.
///
/// This is the third seed layer, and the only one besides the canonical set
/// that reaches production. The canonical layer is what every deployment
/// shares; the simulated-matter fixture is disposable; this layer
/// is the data one brand owns and the other must not carry. The Firm's postal
/// identities and the Foundation's are the founding case: both are real, both
/// belong in production, and neither belongs in the other's database.
///
/// A brand binary declares its own value in [`hosting::Brand`], so adding a
/// brand is a new variant plus its seed directory rather than a branch in
/// this module.
///
/// [`hosting::Brand`]: https://docs.rs/portal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrandSeed {
    /// `neonlaw.com` — Neon Law: the firm at the site root and the Neon Law
    /// Foundation beneath `/foundation`, one binary serving both faces.
    ///
    /// One variant, not two. The firm and the Foundation are still separate
    /// legal entities and their rows stay keyed to those entities, but there
    /// is one deployment applying them, so there is one seed to apply.
    Neon,
    /// A white-label tenant deployment, which carries none of our own
    /// entities' data. This is a real value rather than an absent one: a
    /// tenant boot must be a deliberate "seed nothing", not a brand someone
    /// forgot to name.
    Tenant,
}

impl BrandSeed {
    /// The brand's short key, matching `hosting::Brand::key`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Neon => "neon",
            Self::Tenant => "tenant",
        }
    }

    /// The brand's own `Entity.yaml` as `(contents, path)`, or `None` for a
    /// brand that owns no entities beyond the shared registry.
    const fn entities(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Neon => Some((brand_seeds::NEON_ENTITY, "neon/Entity.yaml")),
            Self::Tenant => None,
        }
    }

    /// The brand's own `Mailroom.yaml` as `(contents, path)`, or `None` for a
    /// brand that rents no mail facility of its own.
    const fn mailrooms(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Neon => Some((brand_seeds::NEON_MAILROOM, "neon/Mailroom.yaml")),
            Self::Tenant => None,
        }
    }

    /// The brand's own `Address.yaml` as `(contents, path)`, embedded at
    /// compile time, or `None` for a brand that owns no addresses of ours.
    /// The path travels with the contents so a parse error names the file a
    /// reader can open rather than the brand that loaded it.
    const fn addresses(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Neon => Some((brand_seeds::NEON_ADDRESS, "neon/Address.yaml")),
            Self::Tenant => None,
        }
    }
}

/// Per-brand seeds, embedded at compile time exactly like the canonical set
/// so an installed binary carries its own brand's data with no runtime
/// lookup of `store/seeds/`.
mod brand_seeds {
    pub const NEON_ENTITY: &str = include_str!("../seeds/neon/Entity.yaml");
    pub const NEON_MAILROOM: &str = include_str!("../seeds/neon/Mailroom.yaml");
    pub const NEON_ADDRESS: &str = include_str!("../seeds/neon/Address.yaml");
}

mod canonical {
    pub const JURISDICTION: &str = include_str!("../seeds/Jurisdiction.yaml");
    pub const ENTITY_TYPE: &str = include_str!("../seeds/EntityType.yaml");
    pub const ENTITY: &str = include_str!("../seeds/Entity.yaml");
    pub const PERSON: &str = include_str!("../seeds/Person.yaml");
    pub const USER: &str = include_str!("../seeds/User.yaml");
    pub const GIT_REPOSITORY: &str = include_str!("../seeds/GitRepository.yaml");
    pub const QUESTION: &str = include_str!("../seeds/Question.yaml");
    pub const LETTER: &str = include_str!("../seeds/Letter.yaml");
    pub const ANSWER: &str = include_str!("../seeds/Answer.yaml");
    pub const PERSON_ENTITY_ROLE: &str = include_str!("../seeds/PersonEntityRole.yaml");
    pub const PERSON_PROJECT_ROLE: &str = include_str!("../seeds/PersonProjectRole.yaml");
    pub const CREDENTIAL: &str = include_str!("../seeds/Credential.yaml");
    pub const TESTIMONIAL: &str = include_str!("../seeds/Testimonial.yaml");

    /// Bundled notation templates. Each entry is `(path, full_md)`
    /// where `path` exists only as a label in the seed report.
    /// Adding a template here lets the cluster carry
    /// it without a separate `navigator catalog-seed` step. The full
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
    pub const TEMPLATE_NAUTILUS_FCRA: &str =
        include_str!("../../templates/neon_law/nautilus/fcra_dispute.md");
    pub const TEMPLATE_NATURALIZATION: &str =
        include_str!("../../templates/forms/united_states/federal/uscis/us__naturalization.md");
}

/// One bundled notation template that the canonical seed inserts into the
/// shared catalog.
#[derive(Debug, Clone, Copy)]
pub struct SeededTemplate {
    pub label: &'static str,
    pub markdown: &'static str,
}

/// The full bundled notation-template catalog, in seed insertion order.
///
/// This is the canonical list consumed by both the database seeder and
/// cross-crate catalog/spec drift guards. Adding a template here makes it a
/// shared seeded template, so its code must also resolve to a questionnaire
/// through the workflow catalog or through an intentionally carried template
/// body.
pub const SEEDED_TEMPLATES: &[SeededTemplate] = &[
    SeededTemplate {
        label: "neon_law/shared/retainer.md",
        markdown: canonical::TEMPLATE_RETAINER,
    },
    SeededTemplate {
        label: "neon_law/shared/closing_letter.md",
        markdown: canonical::TEMPLATE_CLOSING_LETTER,
    },
    SeededTemplate {
        label: "forms/united_states/nevada/state/nv__annual_report.md",
        markdown: canonical::TEMPLATE_ANNUAL_REPORT_NV,
    },
    SeededTemplate {
        label: "forms/united_states/nevada/state/nv__dissolution.md",
        markdown: canonical::TEMPLATE_DISSOLUTION_NV,
    },
    SeededTemplate {
        label: "neon_law/nest/ca__llc_operating_agreement.md",
        markdown: canonical::TEMPLATE_LLC_CA,
    },
    SeededTemplate {
        label: "forms/united_states/federal/irs/us__form_990.md",
        markdown: canonical::TEMPLATE_FORM990,
    },
    SeededTemplate {
        label: "forms/united_states/nevada/state/nv__nonprofit_501c3_formation.md",
        markdown: canonical::TEMPLATE_NONPROFIT_501C3_NV,
    },
    SeededTemplate {
        label: "forms/united_states/nevada/state/nv__charitable_solicitation_registration.md",
        markdown: canonical::TEMPLATE_CHARITABLE_SOLICITATION_NV,
    },
    SeededTemplate {
        label: "forms/united_states/nevada/state/nv__modified_business_tax.md",
        markdown: canonical::TEMPLATE_NV_MBT,
    },
    SeededTemplate {
        label: "neon_law/northstar/nv__generic_trust.md",
        markdown: canonical::TEMPLATE_TRUST_NV,
    },
    SeededTemplate {
        label: "neon_law/northstar/nv__simple_will.md",
        markdown: canonical::TEMPLATE_WILL_SIMPLE,
    },
    SeededTemplate {
        label: "neon_law/northstar/estate_plan.md",
        markdown: canonical::TEMPLATE_ESTATE,
    },
    SeededTemplate {
        label: "neon_law/northstar/nv__will.md",
        markdown: canonical::TEMPLATE_NORTHSTAR_WILL,
    },
    SeededTemplate {
        label: "neon_law/northstar/nv__trust.md",
        markdown: canonical::TEMPLATE_NORTHSTAR_TRUST,
    },
    SeededTemplate {
        label: "neon_law/northstar/nv__directive_health.md",
        markdown: canonical::TEMPLATE_NORTHSTAR_DIRECTIVE_HEALTH,
    },
    SeededTemplate {
        label: "neon_law/northstar/nv__directive_financial.md",
        markdown: canonical::TEMPLATE_NORTHSTAR_DIRECTIVE_FINANCIAL,
    },
    SeededTemplate {
        label: "forms/united_states/nevada/state/nv__llc_formation.md",
        markdown: canonical::TEMPLATE_NEST_NV,
    },
    SeededTemplate {
        label: "forms/united_states/nevada/state/nv__profit_corp_formation.md",
        markdown: canonical::TEMPLATE_NEST_CORP_NV,
    },
    SeededTemplate {
        label: "forms/united_states/nevada/state/nv__business_trust_formation.md",
        markdown: canonical::TEMPLATE_NEST_BUSINESS_TRUST_NV,
    },
    SeededTemplate {
        label: "neon_law/nexus/fractional_gc.md",
        markdown: canonical::TEMPLATE_NEXUS,
    },
    SeededTemplate {
        label: "neon_law/nexus/nv__employment_agreement.md",
        markdown: canonical::TEMPLATE_EMPLOYMENT_W2,
    },
    SeededTemplate {
        label: "neon_law/nexus/nv__contractor_agreement.md",
        markdown: canonical::TEMPLATE_CONTRACTOR_1099,
    },
    SeededTemplate {
        label: "neon_law/nexus/contract_review.md",
        markdown: canonical::TEMPLATE_CONTRACT_REVIEW,
    },
    SeededTemplate {
        label: "neon_law/nautilus/fcra_dispute.md",
        markdown: canonical::TEMPLATE_NAUTILUS_FCRA,
    },
    SeededTemplate {
        label: "forms/united_states/federal/uscis/us__naturalization.md",
        markdown: canonical::TEMPLATE_NATURALIZATION,
    },
];

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
/// Apply the production-safe canonical seed: reference data plus the
/// firm-owned baseline (jurisdictions, entity types, the protected firm
/// [`FIRM_ENTITY_NAME`] Entity and its people, questions, credentials,
/// templates, products, testimonials). It is **environment-blind** — it
/// runs identically in production, so it must never insert a disposable
/// Project, mailroom, letter, or answer row. Those live in
/// [`seed_simulated_portfolio`] and are applied only where matters are
/// simulated.
pub async fn seed_canonical(
    surreal: &SurrealDb,
    storage: &std::sync::Arc<dyn cloud::StorageService>,
) -> anyhow::Result<SeedReport> {
    let mut r = SeedReport::default();
    seed_canonical_into(surreal, storage, &mut r).await?;
    Ok(r)
}

/// Apply the compiled simulated-matter fixture on top of the canonical seed.
/// It gives an environment the three synthetic matters, their participants, and
/// the reference rows the portal walkthrough needs. Applied only where
/// [`crate::config::simulated_matters`] says the data plane is synthetic.
/// Idempotent: a second run inserts zero duplicates.
pub async fn seed_simulated_portfolio(
    surreal: &SurrealDb,
    storage: &std::sync::Arc<dyn cloud::StorageService>,
) -> anyhow::Result<SeedReport> {
    let mut r = SeedReport::default();
    seed_simulated_portfolio_into(surreal, storage, &mut r).await?;
    Ok(r)
}

/// Apply one brand's own seeds. This is production data, deliberately: the
/// Firm's postal identities and the Foundation's are real, and each belongs
/// only in the deployment that serves that brand. Idempotent on the same
/// natural keys as every other layer.
///
/// # Errors
///
/// Propagates any store error from the underlying writes.
pub async fn seed_brand(surreal: &SurrealDb, brand: BrandSeed) -> anyhow::Result<SeedReport> {
    let mut r = SeedReport::default();
    seed_brand_into(surreal, brand, &mut r).await?;
    Ok(r)
}

/// The single environment-aware orchestration call, and the three layers it
/// composes.
///
/// 1. The **canonical** seed, on every boot of every brand in every
///    environment: the shared identities, reference data, and catalog.
/// 2. The booting **brand's own** seed, likewise in every environment
///    including production. This is the layer that carries data one brand
///    owns and the other must not: the brand layer seeds the Firm's mailboxes,
///    `neon` the Foundation's, and neither sees the other's.
/// 3. The disposable **simulated-matter fixture**, only where
///    [`crate::config::simulated_matters`] resolves true, so synthetic
///    Project, mail, or answer rows never reach a deployment holding real
///    files. That is every `dev` boot, plus any deployment that says so
///    explicitly — which is how the persistent staging deployment carries
///    them while running the production runtime profile.
///
/// Every layer is idempotent, so a reset/recreate that runs this again
/// restores the exact same baseline.
///
/// # Errors
///
/// Propagates any store error from the underlying writes.
pub async fn seed_environment(
    surreal: &SurrealDb,
    storage: &std::sync::Arc<dyn cloud::StorageService>,
    environment: crate::DeploymentEnvironment,
    brand: BrandSeed,
) -> anyhow::Result<SeedReport> {
    let simulated = crate::config::simulated_matters(environment)?;
    seed_environment_with(surreal, storage, brand, simulated).await
}

/// [`seed_environment`] with the simulated-matter decision already made, so a
/// test can drive both answers without mutating process environment.
///
/// # Errors
///
/// Propagates any store error from the underlying writes.
pub async fn seed_environment_with(
    surreal: &SurrealDb,
    storage: &std::sync::Arc<dyn cloud::StorageService>,
    brand: BrandSeed,
    simulated_matters: bool,
) -> anyhow::Result<SeedReport> {
    let mut r = SeedReport::default();
    seed_canonical_into(surreal, storage, &mut r).await?;
    seed_brand_into(surreal, brand, &mut r).await?;
    if simulated_matters {
        seed_simulated_portfolio_into(surreal, storage, &mut r).await?;
    }
    Ok(r)
}

/// The brand layer runs after [`seed_canonical_into`] because it leans on it
/// twice: its Entities resolve an entity type and a jurisdiction the canonical
/// layer seeds, and its addresses hang off Entities. Entities therefore come
/// first here too — `seed_addresses` *skips* a record whose Entity it cannot
/// resolve, so the wrong order would be a silent no-op rather than an error.
async fn seed_brand_into(
    surreal: &SurrealDb,
    brand: BrandSeed,
    r: &mut SeedReport,
) -> anyhow::Result<()> {
    if let Some((yaml, path)) = brand.entities() {
        seed_entities(surreal, yaml, path, r).await?;
    }
    if let Some((yaml, path)) = brand.mailrooms() {
        seed_mailrooms(surreal, yaml, path, r).await?;
    }
    seed_addresses(surreal, brand, r).await
}

async fn seed_canonical_into(
    surreal: &SurrealDb,
    storage: &std::sync::Arc<dyn cloud::StorageService>,
    r: &mut SeedReport,
) -> anyhow::Result<()> {
    seed_jurisdictions(surreal, r).await?;
    seed_entity_types(surreal, r).await?;
    seed_entities(surreal, canonical::ENTITY, "Entity.yaml", r).await?;
    seed_persons(surreal, r).await?;
    seed_user_roles(surreal, r).await?;
    seed_questions(surreal, r).await?;
    seed_person_entity_roles(surreal, r).await?;
    seed_credentials(surreal, r).await?;
    seed_templates(surreal, storage, r).await?;
    seed_testimonials(surreal, r).await?;
    seed_glossary_terms(surreal, r).await?;
    Ok(())
}

async fn seed_simulated_portfolio_into(
    surreal: &SurrealDb,
    _storage: &std::sync::Arc<dyn cloud::StorageService>,
    r: &mut SeedReport,
) -> anyhow::Result<()> {
    seed_role_matrix_simulated(surreal, r).await?;
    seed_git_repositories(surreal, r).await?;
    seed_letters(surreal, r).await?;
    seed_answers(surreal, r).await?;
    seed_person_project_roles(surreal, r).await?;
    Ok(())
}

/// One simulated matter the development and staging fixture carries.
///
/// Three of these ship. Each is a whole matter — a client Entity, a Project
/// code that is also its portal's URL segment, the practice it demonstrates,
/// and the public repository whose Vite bundle mounts on it — so adding a
/// fourth is a row here rather than a fourth branch through the seed.
///
/// **Everything in this table is invented.** No client, matter, dispute, or
/// estate named here corresponds to a real one, which is the whole reason the
/// fixture may be published to a deployment anyone can reach.
struct SimulatedMatter {
    /// The Project code, which is also the URL segment its portal mounts under
    /// at `/app/projects/{code}/portal/`.
    code: &'static str,
    /// The matter as a reader sees it named.
    name: &'static str,
    /// The client Entity this matter is opened for.
    client_entity: &'static str,
    /// Whether that client is a person or a company. The two `Entity` shapes
    /// the fixture needs, resolved against the seeded entity types.
    client_kind: SimulatedClientKind,
    /// The practice the matter demonstrates, rendered as the Project's
    /// description.
    description: &'static str,
    /// The public repository whose built bundle mounts on this matter.
    ///
    /// Recorded on the Project rather than compiled into the command that
    /// clones it, so each simulated matter demonstrates the real mechanism: a
    /// Project names its own repository, on whatever forge hosts it.
    repository_url: &'static str,
    /// The deterministic document published when no built bundle is staged.
    portal_index: &'static str,
}

/// Which seeded entity type a simulated matter's client resolves to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SimulatedClientKind {
    /// A natural person — the individual plaintiff and the individual testator.
    Human,
    /// A Nevada C-Corp — the company that engages the firm as outside counsel.
    Company,
}

impl SimulatedClientKind {
    /// The seeded `entity_types.name` this kind resolves to.
    const fn entity_type(self) -> &'static str {
        match self {
            Self::Human => "Human",
            Self::Company => "C-Corp",
        }
    }
}

/// The three simulated matters, in the order a reader meets them.
///
/// They are three deliberately different shapes of legal work, because one
/// matter could only ever demonstrate one: a dispute in front of a court, a
/// company on a monthly retainer, and an estate plan. A person signing in as
/// the fixture Client sees all three at once, which is what makes the
/// participation-scoped list worth looking at.
const SIMULATED_MATTERS: &[SimulatedMatter] = &[
    SimulatedMatter {
        code: DONUT_LITIGATION_CODE,
        name: "Cruller v. Prine",
        client_entity: "Dermot Cruller",
        client_kind: SimulatedClientKind::Human,
        description: "trespass to land, and rescission of the doughnut instrument",
        repository_url:
            "https://github.com/neon-law-foundation/navigator-sample-project-litigation",
        portal_index: DONUT_LITIGATION_PORTAL_INDEX,
    },
    SimulatedMatter {
        code: WIDGET_WORKS_CODE,
        name: "Widget Works — Outside Counsel",
        client_entity: "Widget Works, Inc.",
        client_kind: SimulatedClientKind::Company,
        description: "employment agreements and contract review on a monthly retainer",
        repository_url:
            "https://github.com/neon-law-foundation/navigator-sample-project-transactional",
        portal_index: WIDGET_WORKS_PORTAL_INDEX,
    },
    SimulatedMatter {
        code: MONTGOMERY_ESTATE_CODE,
        name: "Estate of Cornelius Montgomery",
        client_entity: "Cornelius Montgomery",
        client_kind: SimulatedClientKind::Human,
        description: "estate plan dividing the residue among nieces and nephews",
        repository_url: "https://github.com/neon-law-foundation/navigator-sample-project-estate",
        portal_index: MONTGOMERY_ESTATE_PORTAL_INDEX,
    },
];

/// The disputes matter's Project code, and therefore its portal's URL segment.
///
/// Exposed because the browser walkthrough and the `dev sample-project` command
/// both name the matter they drive, and a code that drifted between the seed
/// and either of them would 404 at the mount rather than fail a test.
pub const DONUT_LITIGATION_CODE: &str = "donut-litigation";

/// The transactional matter's Project code.
pub const WIDGET_WORKS_CODE: &str = "widget-works";

/// The estate matter's Project code.
pub const MONTGOMERY_ESTATE_CODE: &str = "montgomery-estate";

/// Look up one simulated matter by its Project code.
#[must_use]
pub fn simulated_matter_codes() -> Vec<&'static str> {
    SIMULATED_MATTERS.iter().map(|matter| matter.code).collect()
}

/// Every simulated matter's caption, in table order.
///
/// Exposed so a test asserting what a participant sees can derive the list
/// rather than restate it: a fourth matter would otherwise silently narrow
/// what those tests check to the three they were written against.
#[must_use]
pub fn simulated_matter_names() -> Vec<&'static str> {
    SIMULATED_MATTERS.iter().map(|matter| matter.name).collect()
}

/// The public repository whose bundle mounts on one simulated matter, or
/// `None` for a code that names no simulated matter.
#[must_use]
pub fn simulated_matter_repository(code: &str) -> Option<&'static str> {
    SIMULATED_MATTERS
        .iter()
        .find(|matter| matter.code == code)
        .map(|matter| matter.repository_url)
}

/// Build one simulated matter's deterministic portal document.
///
/// A self-contained static page — no inline `<script>`, because the portal
/// serve CSP is `script-src 'self'` — so it renders under the same
/// participation-gated stream a real Vite bundle would. `id="{code}-portal-ready"`
/// is the hook the browser walkthrough looks for, which is why the code is
/// interpolated rather than written out per matter.
macro_rules! portal_index {
    ($code:expr, $title:expr, $subtitle:expr, $steps:expr) => {
        concat!(
            r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>"#,
            $title,
            r#" — Client Portal</title>
<style>
  :root { color-scheme: light dark; }
  body { margin: 0; font-family: -apple-system, system-ui, sans-serif; background: #0f1216; color: #e8eef5; }
  header { padding: 2.5rem 2rem 2rem; background: linear-gradient(135deg, #1b2430, #0f1216); border-bottom: 1px solid #263140; }
  .pill { display: inline-block; background: #12351f; color: #57d98a; border-radius: 999px; padding: .2rem .7rem; font-size: .8rem; }
  h1 { margin: .5rem 0 .25rem; font-size: 1.7rem; }
  .sub { color: #8aa0b6; }
  main { padding: 2rem; max-width: 720px; }
  .card { background: #151b22; border: 1px solid #263140; border-radius: 12px; padding: 1.25rem 1.5rem; margin-bottom: 1rem; }
  .card h2 { margin: .1rem 0 .6rem; font-size: 1.05rem; }
  ul { margin: .4rem 0; padding-left: 1.2rem; } li { margin: .3rem 0; }
  footer { padding: 1.5rem 2rem; color: #5f7085; font-size: .85rem; }
</style>
</head>
<body>
<header>
  <div class="pill" id=""#,
            $code,
            r#"-portal-ready">Client portal · live</div>
  <h1>"#,
            $title,
            r#"</h1>
  <div class="sub">"#,
            $subtitle,
            r#"</div>
</header>
<main>
  <div class="card">
    <h2>Where things stand</h2>
    <p>This is the client portal application served for your matter, streamed from Navigator's per-deployment applications bucket.</p>
  </div>
  <div class="card">
    <h2>Next steps</h2>
    <ul>
"#,
            $steps,
            r#"
    </ul>
  </div>
</main>
<footer>Fixture data only — "#,
            $title,
            r#" is a simulated matter.</footer>
</body>
</html>
"#
        )
    };
}

const DONUT_LITIGATION_PORTAL_INDEX: &str = portal_index!(
    "donut-litigation",
    "Cruller v. Prine",
    "Trespass and rescission — your matter workspace",
    "      <li>Review the complaint draft</li>
      <li>Confirm the discovery timeline</li>
      <li>Message your legal team with questions</li>"
);

const WIDGET_WORKS_PORTAL_INDEX: &str = portal_index!(
    "widget-works",
    "Widget Works — Outside Counsel",
    "Employment agreements and contract review — your matter workspace",
    "      <li>Review this month's employment agreement queue</li>
      <li>Send a Redline contract for one-business-day turnaround</li>
      <li>Message your legal team with questions</li>"
);

const MONTGOMERY_ESTATE_PORTAL_INDEX: &str = portal_index!(
    "montgomery-estate",
    "Estate of Cornelius Montgomery",
    "Estate plan — your matter workspace",
    "      <li>Confirm the list of nieces and nephews</li>
      <li>Review the draft will and trust</li>
      <li>Message your legal team with questions</li>"
);

/// Seed the simulated matters every fixture login lands on, with a participant
/// for each firm and client tier so the same projects can be opened through
/// every lens the KIND Rauthy fixture signs in as — including Owner, who
/// carries a firm-side row so they appear in an Owner's participation-scoped
/// `/app/projects` list.
///
/// Idempotent, and it publishes each matter's portal "little app" to the
/// applications bucket so `/app/projects/{code}/portal/` streams rather than
/// 404s. The publish is best-effort: a tier without an applications bucket
/// configured logs and skips rather than failing the whole seed.
async fn seed_role_matrix_simulated(
    surreal: &SurrealDb,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    let nevada = jurisdictions::find_by_name(surreal, "Nevada")
        .await?
        .ok_or_else(|| anyhow::anyhow!("seed: jurisdiction `Nevada` must be seeded first"))?;

    // One person per tier, matching the KIND Rauthy fixture's five accounts.
    let owner_id = ensure_dev_person(
        surreal,
        report,
        "Olive Owner",
        "owner@neonlaw.com",
        crate::persons::Role::Owner,
    )
    .await?;
    // The Admin Person exists so the fixture account can sign in; it is
    // deliberately given no participation on any of these matters (see below),
    // so its id is not bound.
    ensure_dev_person(
        surreal,
        report,
        "Ada Admin",
        "admin@neonlaw.com",
        crate::persons::Role::Admin,
    )
    .await?;
    let lawyer_id = ensure_dev_person(
        surreal,
        report,
        "Lawrence Lawyer",
        "lawyer@neonlaw.com",
        crate::persons::Role::Lawyer,
    )
    .await?;
    let clerk_id = ensure_dev_person(
        surreal,
        report,
        "Clara Clerk",
        "clerk@neonlaw.com",
        crate::persons::Role::Clerk,
    )
    .await?;
    let client_id = ensure_dev_person(
        surreal,
        report,
        "Cleo Client",
        "client@neonlaw.com",
        crate::persons::Role::Client,
    )
    .await?;

    let cast = FixtureCast {
        owner: owner_id,
        lawyer: lawyer_id,
        clerk: clerk_id,
        client: client_id,
    };
    let applications = match cloud::applications_from_env().await {
        Ok(applications) => Some(applications),
        Err(error) => {
            tracing::warn!(%error, "seed: no applications bucket; skipping the simulated portals");
            None
        }
    };

    for matter in SIMULATED_MATTERS {
        open_simulated_matter(
            surreal,
            report,
            matter,
            &cast,
            nevada.id,
            applications.as_ref(),
        )
        .await?;
    }
    Ok(())
}

/// The four fixture accounts that get a participation row on every simulated
/// matter, resolved once.
///
/// Admin is deliberately absent, and its absence is the point — see
/// [`open_simulated_matter`]. Grouping the four into one value is what keeps the
/// per-matter half a readable argument list rather than seven positional ids.
struct FixtureCast {
    owner: Uuid,
    lawyer: Uuid,
    clerk: Uuid,
    client: Uuid,
}

/// Open one simulated matter: its client Entity, its Project, its repository
/// pointer, its participation rows, its two DRIs, and its portal bundle.
///
/// Split from [`seed_role_matrix_simulated`] because the two halves answer
/// different questions — who the fixture accounts are, and what one matter is —
/// and only the second one repeats.
async fn open_simulated_matter(
    surreal: &SurrealDb,
    report: &mut SeedReport,
    matter: &SimulatedMatter,
    cast: &FixtureCast,
    jurisdiction_id: Uuid,
    applications: Option<&std::sync::Arc<dyn cloud::StorageService>>,
) -> anyhow::Result<()> {
    let FixtureCast {
        owner: owner_id,
        lawyer: lawyer_id,
        clerk: clerk_id,
        client: client_id,
    } = *cast;
    {
        let entity_type =
            crate::entity_types::find_by_name(surreal, matter.client_kind.entity_type())
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "seed: entity_type `{}` must be seeded first",
                        matter.client_kind.entity_type()
                    )
                })?;
        let entity_id = ensure_dev_entity(
            surreal,
            report,
            matter.client_entity,
            entity_type.id,
            jurisdiction_id,
        )
        .await?;
        let project_id = ensure_dev_project(
            surreal,
            report,
            matter.code,
            matter.name,
            entity_id,
            matter.description,
        )
        .await?;

        crate::projects::set_repository_url(surreal, project_id, Some(matter.repository_url))
            .await?;

        // Client side and firm side. The lawyer is the licensed lawyer DRI,
        // which is also what lets the supervised Clerk resolve the matter.
        ensure_participation(surreal, report, project_id, client_id, "client").await?;
        ensure_participation(surreal, report, project_id, lawyer_id, "attorney").await?;
        ensure_participation(surreal, report, project_id, clerk_id, "clerk").await?;
        // Owner gets a firm-side row so the demo matters appear in the Owner's
        // own list: since ENG-81 the whole matter surface — `/app/projects` and
        // `/app/projects/{code}` alike — is participation-scoped for every tier,
        // with no privileged bypass.
        ensure_participation(surreal, report, project_id, owner_id, "owner").await?;
        // Admin deliberately gets **no** row. These matters are what an
        // administrator who was never assigned to a matter looks like, which is
        // the ENG-81 decision made visible: privileged reach is a place you
        // navigate to (`/app/admin`, which reads and writes the participation
        // ledger), not a silent widening of a shared route. Because one row
        // gates both the list and the detail view, this is also why none of
        // these appear in an admin's `/app/projects` list — the absence is the
        // point, not an oversight.
        crate::projects::designate_dri_in_surreal(
            surreal,
            project_id,
            lawyer_id,
            crate::projects::DriSide::Lawyer,
        )
        .await?;
        crate::projects::designate_dri_in_surreal(
            surreal,
            project_id,
            client_id,
            crate::projects::DriSide::Client,
        )
        .await?;

        if let Some(applications) = applications {
            publish_simulated_portal(applications, matter).await?;
        }
    }
    Ok(())
}

/// Publish one simulated matter's client portal, preferring a locally built
/// bundle.
///
/// Local `dev` boot clones and builds each matter's repository and stages it,
/// naming the staged directories in [`crate::sample_project::STAGE_ENV`]. Boot
/// publishes the bundle whose manifest names this matter; the generated
/// environment points the web process at the staged directories.
///
/// A staged bundle must declare its Project in `navigator.yml`. Publishing a
/// bundle that names another matter would put one client's application on
/// another client's portal, so a mismatch — like an unbuilt or unparsable
/// staging directory — leaves the deterministic document in place and is
/// reported as a failed local application build.
async fn publish_simulated_portal(
    applications: &std::sync::Arc<dyn cloud::StorageService>,
    matter: &SimulatedMatter,
) -> anyhow::Result<()> {
    if let Some(staged) = crate::sample_project::staged_for(matter.code) {
        match publish_staged_portal(applications, &staged, matter.code).await {
            Ok(count) => {
                tracing::info!(
                    code = matter.code,
                    root = %staged.root.display(),
                    objects = count,
                    "seed: published the built sample project portal"
                );
                return Ok(());
            }
            Err(error) => {
                // Keep the deterministic portal document available while the
                // local application build is repaired.
                tracing::warn!(
                    code = matter.code,
                    root = %staged.root.display(),
                    %error,
                    "seed: staged sample project unusable; keeping the portal document"
                );
            }
        }
    }

    applications
        .put_cached(
            &format!(
                "{}/{}",
                crate::sample_project::portal_prefix(matter.code),
                crate::sample_project::ENTRY_DOCUMENT
            ),
            matter.portal_index.as_bytes(),
            "text/html; charset=utf-8",
            crate::sample_project::ENTRY_CACHE_CONTROL,
        )
        .await?;
    Ok(())
}

/// Publish one staged bundle, returning how many objects landed. Every failure
/// mode — missing manifest, wrong Project, or unbuilt `dist/` — is returned as
/// an error for the local boot to report.
async fn publish_staged_portal(
    applications: &std::sync::Arc<dyn cloud::StorageService>,
    staged: &crate::sample_project::StagedProject,
    expected_code: &str,
) -> anyhow::Result<usize> {
    let manifest = std::fs::read_to_string(staged.manifest()).with_context(|| {
        format!(
            "reading {} — a project application must declare its Project",
            staged.manifest().display()
        )
    })?;
    let code = crate::sample_project::project_code_for(&manifest, expected_code)?;

    let plan = crate::sample_project::publish_plan(&staged.dist, &code)?;
    anyhow::ensure!(
        !plan.is_empty(),
        "{} has no {} — that is a failed build, not a bundle",
        staged.dist.display(),
        crate::sample_project::ENTRY_DOCUMENT
    );

    let count = plan.len();
    for object in plan {
        let bytes = std::fs::read(&object.source)?;
        applications
            .put_cached(
                &object.key,
                &bytes,
                object.content_type,
                object.cache_control,
            )
            .await?;
    }
    Ok(count)
}

async fn ensure_dev_person(
    surreal: &SurrealDb,
    report: &mut SeedReport,
    name: &str,
    email: &str,
    role: crate::persons::Role,
) -> anyhow::Result<Uuid> {
    let existing = crate::persons::find_by_email_ci(surreal, email).await?;
    let row = crate::persons::find_or_create(
        surreal,
        &crate::persons::NewPerson::with_role(name, email, role),
    )
    .await?;
    if existing.is_none() {
        report.persons_inserted += 1;
        return Ok(row.id);
    }
    if row.name != name || row.role != role {
        crate::persons::edit(
            surreal,
            row.id,
            &crate::persons::PersonEdit {
                name: Some(name.into()),
                role: Some(role),
                ..crate::persons::PersonEdit::default()
            },
        )
        .await?;
        report.persons_updated += 1;
    }
    Ok(row.id)
}

/// Idempotently ensure one simulated matter's client Entity, of whatever type
/// that matter's client is. The type is a parameter rather than fixed to
/// `Human` because the fixture carries a company client as well as two people,
/// and a company engaging outside counsel is a different Entity shape from an
/// individual plaintiff.
async fn ensure_dev_entity(
    surreal: &SurrealDb,
    report: &mut SeedReport,
    name: &str,
    entity_type_id: Uuid,
    jurisdiction_id: Uuid,
) -> anyhow::Result<Uuid> {
    if let Some(row) = crate::entities::find_by_name_and_type(surreal, name, entity_type_id).await?
    {
        // Same repair as `seed_entities`: a persisted row's
        // `jurisdiction_id` may name a jurisdiction the reset local
        // engine no longer holds, and the engine never validated it.
        if row.jurisdiction_id != jurisdiction_id {
            crate::entities::repoint_jurisdiction(surreal, row.id, jurisdiction_id).await?;
        }
        return Ok(row.id);
    }
    let row = crate::entities::create(surreal, &new_entity(name, entity_type_id, jurisdiction_id))
        .await?;
    report.entities_inserted += 1;
    Ok(row.id)
}

/// The write shape every seeded Entity shares.
///
/// The firm's own row is the reason this is a helper rather than a
/// literal: it must carry `firm_anchor_key`, or the seeded firm would be
/// forkable through `entity_commands` — the guard reads the key, and a
/// row seeded without one leaves the `entity_firm_anchor` index free.
/// The shipped default stands in for the configured anchor here because
/// `is_firm_anchor` protects it under every configuration, and the
/// canonical seed ships no other firm.
fn new_entity(
    name: &str,
    entity_type_id: Uuid,
    jurisdiction_id: Uuid,
) -> crate::entities::NewEntity {
    crate::entities::NewEntity {
        name: name.to_string(),
        entity_type_id,
        jurisdiction_id,
        phone: None,
        url: None,
        firm_anchor_key: crate::entity_commands::firm_anchor_key(FIRM_ENTITY_NAME, name),
    }
}

async fn ensure_dev_project(
    surreal: &SurrealDb,
    report: &mut SeedReport,
    code: &str,
    name: &str,
    entity_id: Uuid,
    description: &str,
) -> anyhow::Result<Uuid> {
    let input = crate::projects::NewProject {
        code: code.to_string(),
        name: name.to_string(),
        status: "open".to_string(),
        entity_id,
        description: Some(description.to_string()),
    };
    let row = match crate::projects::find_by_code(surreal, code).await? {
        Some(existing) => crate::projects::upsert_with_id(surreal, existing.id, &input).await?,
        None => crate::projects::find_or_create_by_code(surreal, Uuid::now_v7(), &input).await?,
    };
    report.projects_inserted += 1;
    Ok(row.id)
}

/// Idempotently record one person's participation on a project.
async fn ensure_participation(
    surreal: &SurrealDb,
    report: &mut SeedReport,
    project_id: Uuid,
    person_id: Uuid,
    participation: &str,
) -> anyhow::Result<()> {
    if let Some(row) =
        crate::projects::participation_for_person(surreal, person_id, project_id).await?
    {
        // Restore the seeded participation if a persisted database drifted it:
        // the client/lawyer visibility this fixture demonstrates depends on the
        // exact value, so a stale `paralegal` or blank row must be reconciled.
        if row.participation != participation {
            crate::projects::update_participation(surreal, row.id, person_id, participation)
                .await?;
        }
        return Ok(());
    }
    ensure_participation_in_surreal(surreal, project_id, person_id, participation).await?;
    report.person_project_roles_inserted += 1;
    Ok(())
}

/// Reconcile one participation row in the Surreal ledger. Split out because
/// the memory-backed local engine resets with its pod, so this must
/// converge whether the row is already there or not.
async fn ensure_participation_in_surreal(
    surreal: &SurrealDb,
    project_id: Uuid,
    person_id: Uuid,
    participation: &str,
) -> anyhow::Result<()> {
    match crate::projects::participation_for_person(surreal, person_id, project_id).await? {
        Some(existing) if existing.participation == participation => Ok(()),
        Some(existing) => {
            crate::projects::update_participation(surreal, existing.id, person_id, participation)
                .await?;
            Ok(())
        }
        None => {
            match crate::projects::add_participation(surreal, project_id, person_id, participation)
                .await
            {
                Ok(_) => Ok(()),
                // `person_project_role_pair` is UNIQUE, so a concurrent seed
                // may have filed this pair between the read above and this
                // write. Losing that race is not an error — adopt the winner's
                // row and reconcile its value, the same settle-on-one-row
                // contract `find_or_create_by_code` gives the project itself.
                Err(err) if err.to_string().contains("person_project_role_pair") => {
                    if let Some(existing) =
                        crate::projects::participation_for_person(surreal, person_id, project_id)
                            .await?
                    {
                        if existing.participation != participation {
                            crate::projects::update_participation(
                                surreal,
                                existing.id,
                                person_id,
                                participation,
                            )
                            .await?;
                        }
                    }
                    Ok(())
                }
                Err(err) => Err(err.into()),
            }
        }
    }
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
    /// Declared notation kind (`retainer`/`letter`/`filing`) from the
    /// `kind:` key; `None` until declared.
    #[serde(default)]
    kind: Option<String>,
}

/// Template codes in the shared seeded catalog.
///
/// The codes are parsed from the same frontmatter the seeder uses, so a
/// cross-crate guard can derive its coverage from the actual catalog instead
/// of maintaining a second list by hand.
pub fn seeded_template_codes() -> anyhow::Result<Vec<String>> {
    SEEDED_TEMPLATES
        .iter()
        .map(|template| {
            let (fm_str, _) = split_template(template.markdown)
                .ok_or_else(|| anyhow::anyhow!("{}: missing YAML frontmatter", template.label))?;
            let fm: TemplateFrontmatter = serde_yaml::from_str(fm_str)
                .map_err(|e| anyhow::anyhow!("{}: parse frontmatter: {e}", template.label))?;
            Ok(fm.code)
        })
        .collect()
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
    surreal: &SurrealDb,
    storage: &std::sync::Arc<dyn cloud::StorageService>,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    for template in SEEDED_TEMPLATES {
        let label = template.label;
        let md = template.markdown;
        let (fm_str, body) = split_template(md)
            .ok_or_else(|| anyhow::anyhow!("{label}: missing YAML frontmatter"))?;
        let fm: TemplateFrontmatter = serde_yaml::from_str(fm_str)
            .map_err(|e| anyhow::anyhow!("{label}: parse frontmatter: {e}"))?;

        // The body lives in a content-addressed asset; ingest it (sha
        // dedup) and reference it by `asset_id`.
        let body_bytes = body.trim_start().as_bytes();
        let asset_id = crate::assets::ingest_content(surreal, storage, body_bytes, "text/markdown")
            .await
            .map_err(|e| anyhow::anyhow!("{label}: ingest body asset: {e}"))?;

        // Immutable by policy: a fresh cluster writes the first version;
        // an unchanged re-seed is a no-op; a changed body/form/title
        // appends a new current version and retires the prior one, so a
        // Notation already opened against the old bytes keeps resolving to
        // them (`notation.template_id` pins the version).
        let saved = crate::templates::save_version(
            surreal,
            None,
            &fm.code,
            crate::templates::Version {
                title: fm.title,
                respondent_type: fm.respondent_type,
                asset_id: Some(asset_id),
                form_code: fm.form,
                kind: fm.kind,
                // The seeded workspace catalog comes from bundled files,
                // not a git repo — no commit provenance.
                source_commit_sha: None,
            },
        )
        .await?;
        if saved.was_written() {
            report.templates_inserted += 1;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct CredentialRec {
    person: PersonEmailRef,
    jurisdiction: JurisdictionRef,
    license_number: String,
}

async fn seed_credentials(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<CredentialRec>(canonical::CREDENTIAL, "Credential.yaml")? {
        let Some(p) = crate::persons::find_by_email_ci(surreal, &rec.person.email).await? else {
            continue;
        };
        let Some(j) = jurisdictions::find_by_name(surreal, &rec.jurisdiction.name).await? else {
            continue;
        };
        // Find-or-grant rather than read-then-write: the seed runs on
        // every boot, and two boots racing would otherwise both miss the
        // read and collide on `credential_person_jurisdiction`.
        let before = crate::credentials::find_by_person_and_jurisdiction(surreal, p.id, j.id)
            .await?
            .is_some();
        crate::credentials::find_or_grant(surreal, p.id, j.id, &rec.license_number).await?;
        if !before {
            report.credentials_inserted += 1;
        }
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

/// Materialize the authored firm glossary into `glossary_term` rows — in
/// SurrealDB, where the table lives since its slice of #1093 (ENG-20).
///
/// Reference data, so it sits in the canonical seed beside jurisdictions:
/// environment-blind, identical in every deployment, and idempotent — the
/// write is keyed on slug, so an edited definition converges rather than
/// appending a second row. It inserts nothing matter-scoped, which is why
/// it is safe in production.
async fn seed_glossary_terms(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    report.glossary_terms_written +=
        crate::glossary::materialize(surreal, crate::glossary::GLOSSARY_MD).await?;
    Ok(())
}

/// Seed the jurisdiction reference table — into SurrealDB, where the
/// table lives since its slice of #1093 (ENG-20). Canonical seed data:
/// it runs identically in every environment, production included, and
/// is idempotent on `code`.
async fn seed_jurisdictions(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<JurisdictionRec>(canonical::JURISDICTION, "Jurisdiction.yaml")? {
        if jurisdictions::find_by_code(surreal, &rec.code)
            .await?
            .is_some()
        {
            continue;
        }
        match jurisdictions::create(
            surreal,
            &NewJurisdiction::new(rec.name, rec.code, rec.jurisdiction_type),
        )
        .await
        {
            Ok(_) => report.jurisdictions_inserted += 1,
            // A concurrent boot won the `jurisdiction_code` unique index
            // between the check and the insert; the row exists, which is
            // all the seed wants.
            Err(jurisdictions::JurisdictionError::CodeTaken) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct EntityTypeRec {
    name: String,
}

/// Seed the entity-type reference table — into SurrealDB, where the
/// table lives since its slice of #1093 (ENG-20). Canonical seed data:
/// it runs identically in every environment, production included, and
/// is idempotent on `name` — `find_or_create` absorbs a concurrent
/// boot's winning write.
async fn seed_entity_types(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for rec in parse::<EntityTypeRec>(canonical::ENTITY_TYPE, "EntityType.yaml")? {
        if !seen.insert(rec.name.clone()) {
            continue;
        }
        if crate::entity_types::find_by_name(surreal, &rec.name)
            .await?
            .is_some()
        {
            continue;
        }
        crate::entity_types::find_or_create(surreal, &rec.name).await?;
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

async fn seed_entities(
    surreal: &SurrealDb,
    yaml: &str,
    path: &str,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    // Rows the seed already owns whose `firm_anchor_key` disagrees with
    // the name [`FIRM_ENTITY_NAME`] now carries. Setting is deferred to a
    // second pass so every stale key is surrendered before the new one is
    // claimed: the `entity_firm_anchor` index is UNIQUE, and a rename
    // between two names the seed both ships would otherwise be refused by
    // the row it is moving away from.
    let mut claim_anchor: Vec<(Uuid, String)> = Vec::new();
    for rec in parse::<EntityRec>(yaml, path)? {
        let et = crate::entity_types::find_by_name(surreal, &rec.entity_type.name)
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
        let jur = jurisdictions::find_by_name(surreal, jurisdiction_name)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("Entity.yaml references unknown jurisdiction {jurisdiction_name:?}")
            })?;
        if let Some(row) = crate::entities::find_by_name_and_type(surreal, &rec.name, et.id).await?
        {
            // A persisted database can outlive the memory-backed local
            // engine, whose re-seeded jurisdictions carry fresh ids, and
            // the engine never validated the link. Repoint so the
            // reference resolves again instead of dangling.
            if row.jurisdiction_id != jur.id {
                crate::entities::repoint_jurisdiction(surreal, row.id, jur.id).await?;
            }
            // Nothing above rewrites an existing row, so moving the
            // configured anchor would strand `firm_anchor_key` on the
            // outgoing firm and never mint it on the incoming one — and
            // that column, not the name, is what `delete_unless_firm_anchor`
            // reads. Reconcile the row to the name it should carry now.
            let expected = crate::entity_commands::firm_anchor_key(FIRM_ENTITY_NAME, &rec.name);
            if row.firm_anchor_key != expected {
                match expected {
                    Some(key) => claim_anchor.push((row.id, key)),
                    None => {
                        crate::entities::set_firm_anchor_key(surreal, row.id, None).await?;
                    }
                }
            }
            continue;
        }
        match crate::entities::create(surreal, &new_entity(&rec.name, et.id, jur.id)).await {
            Ok(_) => report.entities_inserted += 1,
            // The firm's own row is the one fixture two seeds can race for,
            // because it is the only one that takes `firm_anchor_key` — and
            // the UNIQUE index is what makes the loser lose. The cucumber
            // suites run scenarios concurrently against one shared engine and
            // each re-seeds this fixture, so losing that race has to be
            // "someone else already did it", not a failed seed.
            Err(crate::entities::EntityError::FirmAnchorTaken) => {}
            Err(error) => return Err(error.into()),
        }
    }
    for (id, key) in claim_anchor {
        // Losing the key to a concurrent seed is the same outcome the create
        // arm absorbs: another pass already moved the anchor onto this
        // identity, so this one has nothing left to do.
        match crate::entities::set_firm_anchor_key(surreal, id, Some(key)).await {
            Ok(_) | Err(crate::entities::EntityError::FirmAnchorTaken) => {}
            Err(error) => return Err(error.into()),
        }
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

async fn seed_persons(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<PersonRec>(canonical::PERSON, "Person.yaml")? {
        let email = rec.email.clone();
        let before = crate::persons::find_by_email_ci(surreal, &email).await?;
        crate::persons::find_or_create(
            surreal,
            &crate::persons::NewPerson {
                profile_image_url: rec.profile_image_url,
                ..crate::persons::NewPerson::new(rec.name, rec.email)
            },
        )
        .await?;
        if before.is_none() {
            report.persons_inserted += 1;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct TestimonialRec {
    project: ProjectCodenameRef,
    person: PersonEmailRef,
    #[serde(default)]
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

async fn seed_testimonials(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<TestimonialRec>(canonical::TESTIMONIAL, "Testimonial.yaml")? {
        let Some(project) = crate::projects::find_by_name(surreal, &rec.project.codename).await?
        else {
            continue;
        };
        let Some(person) = crate::persons::find_by_email_ci(surreal, &rec.person.email).await?
        else {
            continue;
        };
        crate::testimonials::find_or_create(
            surreal,
            &crate::testimonials::NewTestimonial {
                project_id: project.id,
                person_id: person.id,
                quote: &rec.quote,
                attribution_label: rec.attribution_label,
                consented_at: rec.consented_at,
                published_at: rec.published_at,
                display_order: rec.display_order,
            },
        )
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

/// Firm-domain convention for seeded organization roles: any `owner`, `admin`,
/// or `clerk` row must use a **lowercase** `@neonlaw.com` email. `lawyer` is a
/// licensed-lawyer tier, not an employment or email-domain assertion, so an
/// outside lawyer's Lawyer seed may use their own domain. The
/// lowercase requirement is exact-match: the seed is the canonical
/// source of truth, so it stores one spelling rather than relying on
/// readers to normalize. Lookups themselves are case-insensitive
/// (`store::persons::find_by_email_ci`, backed by the
/// `persons_email_lower_key` unique index), so mixed-case input
/// resolves correctly — this rule keeps the seed data itself tidy.
/// See `docs/access-model.md`.
fn require_firm_domain(email: &str, role: crate::persons::Role) -> anyhow::Result<()> {
    use crate::persons::Role;
    if !matches!(role, Role::Owner | Role::Admin | Role::Clerk) {
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
             convention — owner/admin/clerk records must use an @neonlaw.com email \
             (see docs/access-model.md)",
        );
    }
    Ok(())
}

/// User.yaml carries a `role` per person; the `users` table doesn't
/// exist as its own entity here — the system-wide tier lives on
/// `persons.role`. Resolve each user record by email, parse the role
/// token, and update the row if the requested tier is higher than
/// what's already stored. The ladder is owner > admin > lawyer > clerk > client.
async fn seed_user_roles(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    use crate::persons::Role;

    fn parse_role_token(s: &str) -> Role {
        match s {
            "owner" => Role::Owner,
            "admin" => Role::Admin,
            "lawyer" => Role::Lawyer,
            "clerk" => Role::Clerk,
            _ => Role::Client,
        }
    }
    for rec in parse::<UserRec>(canonical::USER, "User.yaml")? {
        let requested = parse_role_token(&rec.role);
        require_firm_domain(&rec.person.email, requested)?;
        let Some(p) = crate::persons::find_by_email_ci(surreal, &rec.person.email).await? else {
            continue;
        };
        if p.role.authority_rank() >= requested.authority_rank() {
            continue;
        }
        crate::persons::set_role(surreal, p.id, requested).await?;
        report.persons_updated += 1;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct GitRepoRec {
    repository_name: String,
}

/// Seed the tracked-repository provenance rows — into SurrealDB, where
/// the table lives since its slice of #1093 (ENG-20). Idempotent on
/// `remote_hash`; `find_or_create` absorbs a concurrent boot's winning
/// write.
async fn seed_git_repositories(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<GitRepoRec>(canonical::GIT_REPOSITORY, "GitRepository.yaml")? {
        let remote_hash = remote_hash(&rec.repository_name);
        if crate::git_repositories::find_by_remote_hash(surreal, &remote_hash)
            .await?
            .is_some()
        {
            continue;
        }
        crate::git_repositories::find_or_create(surreal, &remote_hash, &"0".repeat(40)).await?;
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
    /// `lawyer` | `client` | `both` — which side of the intake sees this
    /// question. Defaults `both` when the YAML omits it.
    #[serde(default)]
    audience: Option<String>,
    // `help_text` / `choices` exist in the YAML but the schema has
    // no column for them — silently dropped.
}

async fn seed_questions(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<QuestionRec>(canonical::QUESTION, "Question.yaml")? {
        // `find_or_create` rather than read-then-insert: the cucumber
        // suites run scenarios concurrently against one shared engine, so a
        // seeder that assumed exclusivity would lose the race and surface
        // `CodeTaken`. It also keeps the second dev boot a no-op.
        let existed = crate::questions::find_by_code(surreal, &rec.code)
            .await?
            .is_some();
        crate::questions::find_or_create(
            surreal,
            &crate::questions::NewQuestion::new(
                rec.code,
                rec.prompt,
                rec.question_type.unwrap_or_else(|| "string".into()),
            )
            .with_audience(
                rec.audience
                    .unwrap_or_else(|| crate::questions::AUDIENCE_BOTH.to_string()),
            ),
        )
        .await?;
        if !existed {
            report.questions_inserted += 1;
        }
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
struct MailroomRec {
    name: String,
}

/// `mailrooms.address_id` is NOT NULL; the YAML carries no separate
/// address for the mailroom itself. We synthesize a placeholder
/// address per mailroom so the column is satisfied — flagged with a
/// `(via mailroom)` line1 so it's obvious in row dumps. That placeholder
/// is the row the `address` schema's missing XOR assert exists for: it
/// names neither a person nor an entity.
async fn seed_mailrooms(
    surreal: &SurrealDb,
    yaml: &str,
    path: &str,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    for rec in parse::<MailroomRec>(yaml, path)? {
        if crate::mailrooms::find_by_name(surreal, &rec.name)
            .await?
            .is_some()
        {
            continue;
        }
        let addr = crate::addresses::create(
            surreal,
            &crate::addresses::NewAddress {
                line1: format!("(via mailroom: {})", rec.name),
                ..crate::addresses::NewAddress::default()
            },
        )
        .await?;
        // Find-or-create, not create: the cucumber suites seed
        // concurrently against one shared engine, so the read above can
        // miss a mailroom another scenario is creating right now.
        let created = crate::mailrooms::find_or_create(surreal, &rec.name, addr.id).await?;
        if created.address_id == addr.id {
            report.mailrooms_inserted += 1;
        }
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

/// Both `person_id` and `entity_id` are real `record<>` links since the
/// entities cluster ported (ENG-120), so this step is single-engine.
async fn seed_addresses(
    surreal: &SurrealDb,
    brand: BrandSeed,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    let Some((yaml, path)) = brand.addresses() else {
        return Ok(());
    };
    for rec in parse::<AddressRec>(yaml, path)? {
        let Some(ent) = crate::entities::find_by_name(surreal, &rec.entity.name).await? else {
            continue;
        };
        let (_, created) = crate::addresses::find_or_create_for_entity(
            surreal,
            &crate::addresses::NewAddress {
                entity_id: Some(ent.id),
                line1: rec.street,
                city: rec.city,
                region: rec.state,
                postal_code: rec.zip,
                country: rec.country,
                ..crate::addresses::NewAddress::default()
            },
        )
        .await?;
        if created {
            report.addresses_inserted += 1;
        }
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

async fn seed_letters(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<LetterRec>(canonical::LETTER, "Letter.yaml")? {
        let Some(mr) = crate::mailrooms::find_by_name(surreal, &rec.mailroom.name).await? else {
            continue;
        };
        if crate::letters::find_by_mailroom_sender_summary(
            surreal,
            mr.id,
            &rec.sender,
            &rec.subject,
        )
        .await?
        .is_some()
        {
            continue;
        }
        crate::letters::record(
            surreal,
            &crate::letters::NewLetter {
                mailroom_id: mr.id,
                direction: crate::letters::DIRECTION_INCOMING.to_string(),
                sender: rec.sender,
                recipient: rec.mailroom.name.clone(),
                summary: rec.subject,
            },
        )
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

async fn seed_answers(surreal: &SurrealDb, report: &mut SeedReport) -> anyhow::Result<()> {
    for rec in parse::<AnswerRec>(canonical::ANSWER, "Answer.yaml")? {
        let Some(q) = crate::questions::find_by_code(surreal, &rec.question_code).await? else {
            continue;
        };
        let Some(p) = crate::persons::find_by_email_ci(surreal, &rec.person_email).await? else {
            continue;
        };
        let value = crate::answers::primitive(&rec.value);
        // Idempotent on the lookup fields (question, person, value) so a
        // second dev boot inserts zero duplicates. These fixtures are
        // person-scoped and carry no Notation, so there is no state to key
        // on — the value itself is the natural key.
        if crate::answers::exists_with_value(surreal, q.id, p.id, &value).await? {
            continue;
        }
        crate::answers::record(surreal, &crate::answers::NewAnswer::new(q.id, p.id, value)).await?;
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
    surreal: &SurrealDb,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    for rec in parse::<PersonEntityRoleRec>(canonical::PERSON_ENTITY_ROLE, "PersonEntityRole.yaml")?
    {
        let Some(p) = crate::persons::find_by_email_ci(surreal, &rec.person.email).await? else {
            continue;
        };
        let Some(e) = crate::entities::find_by_name(surreal, &rec.entity.name).await? else {
            continue;
        };
        // `grant` is find-or-create behind the UNIQUE `entity_role_tie`
        // index, so re-seeding a live database adds nothing and two
        // concurrent seeds settle on one edge rather than racing.
        let existing = crate::entity_roles::find(surreal, p.id, e.id, &rec.role).await?;
        crate::entity_roles::grant(surreal, p.id, e.id, &rec.role).await?;
        if existing.is_none() {
            report.person_entity_roles_inserted += 1;
        }
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
    surreal: &SurrealDb,
    report: &mut SeedReport,
) -> anyhow::Result<()> {
    for rec in
        parse::<PersonProjectRoleRec>(canonical::PERSON_PROJECT_ROLE, "PersonProjectRole.yaml")?
    {
        let Some(p) = crate::persons::find_by_email_ci(surreal, &rec.person.email).await? else {
            continue;
        };
        let Some(pr) = crate::projects::find_by_name(surreal, &rec.project.codename).await? else {
            continue;
        };
        if crate::projects::participation_for_person(surreal, p.id, pr.id)
            .await?
            .is_some()
        {
            continue;
        }
        crate::projects::add_participation(surreal, pr.id, p.id, &rec.role).await?;
        report.person_project_roles_inserted += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{seed_canonical, seeded_template_codes, SEEDED_TEMPLATES};
    use crate::jurisdictions;
    use crate::persons::{self, Role};
    use crate::question_registry::QuestionType;
    use crate::test_support::mem_surreal;

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

    /// The firm's own Entity is the one seeded row that takes
    /// `firm_anchor_key`, so it is the one row the `entity_firm_anchor`
    /// index can refuse (ENG-120). The seed re-runs over a live database
    /// on every boot, so it has to survive losing that write.
    ///
    /// Shook Law PLLC holds its own private mailbox at the Ridgeview Mail
    /// Center, and within that mail centre the box number is the whole address
    /// — `405-9002`, `405-9005`, and `405-9999` are the same street, suite, and
    /// ZIP, so a wrong suffix delivers the firm's mail to another entity of
    /// ours rather than bouncing.
    ///
    /// The two halves land in different layers on purpose, and this asserts
    /// both: the Entity is canonical, so every deployment carries it, while
    /// the mailbox is the Firm's own and rides the brand layer.
    #[tokio::test]
    async fn shook_law_holds_mailbox_9002_at_ridgeview() {
        let surreal = mem_surreal().await;
        let storage = fs_storage().await;

        seed_canonical(&surreal, &storage).await.expect("seed");
        let firm = crate::entities::find_by_name(&surreal, super::FIRM_ENTITY_NAME)
            .await
            .unwrap()
            .expect("the firm anchor is canonical, so every deployment carries it");

        // Canonical alone carries no address: the mailbox is the Firm's.
        assert!(
            crate::addresses::for_entity(&surreal, firm.id)
                .await
                .unwrap()
                .is_empty(),
            "the Firm's own addresses must stay out of the canonical layer"
        );

        super::seed_brand(&surreal, super::BrandSeed::Neon)
            .await
            .expect("brand seed");
        let at_ridgeview: Vec<String> = crate::addresses::for_entity(&surreal, firm.id)
            .await
            .unwrap()
            .into_iter()
            .map(|a| a.line1)
            .filter(|line| line.starts_with("5150 Mae Anne Ave"))
            .collect();
        assert_eq!(
            at_ridgeview,
            vec!["5150 Mae Anne Ave Ste 405-9002".to_string()],
            "the firm holds exactly one box at the mail centre, and 9002 is that box"
        );

        // The box is unique across the mail center: no other seeded entity
        // may answer to it, or mail routes to whichever row is read first.
        let holders = crate::addresses::list_all(&surreal)
            .await
            .unwrap()
            .into_iter()
            .filter(|a| a.line1.ends_with("405-9002"))
            .count();
        assert_eq!(holders, 1, "one box, one holder");
    }

    /// The California law corporation is the Firm's, so it rides the brand
    /// layer rather than the shared registry — and it seeds under its
    /// own jurisdiction, not the Nevada every sibling row carries.
    ///
    /// The jurisdiction is worth pinning because the two lookups in
    /// `seed_entities` do not agree: the *entity type* resolves by name
    /// alone, so the seed finds `Professional Corporation` whatever
    /// jurisdiction it was declared under, while the *entity's* jurisdiction
    /// comes from the nested `jurisdiction.name`. Drop that nested key and
    /// the row still seeds cleanly — into Nevada, silently, because `Nevada`
    /// is the fallback. A law corporation in the wrong state is not a
    /// cosmetic error: its registration and its regulator both follow the
    /// jurisdiction.
    #[tokio::test]
    async fn the_california_law_corporation_seeds_under_california() {
        let surreal = mem_surreal().await;

        seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("seed");
        assert!(
            crate::entities::find_by_name(&surreal, "Yakcobieus Industries PC")
                .await
                .unwrap()
                .is_none(),
            "the Firm's own corporation must not reach the shared registry"
        );

        super::seed_brand(&surreal, super::BrandSeed::Neon)
            .await
            .expect("brand seed");

        let pc = crate::entities::find_by_name(&surreal, "Yakcobieus Industries PC")
            .await
            .unwrap()
            .expect("the California law corporation seeds on a brand boot");
        let jurisdiction = jurisdictions::find_by_id(&surreal, pc.jurisdiction_id)
            .await
            .unwrap()
            .expect("its jurisdiction resolves");
        assert_eq!(jurisdiction.name, "California");

        let entity_type = crate::entity_types::find_by_name(&surreal, "Professional Corporation")
            .await
            .unwrap()
            .expect("the professional-corporation type seeds");
        assert_eq!(pc.entity_type_id, entity_type.id);
    }

    /// Losing it is not hypothetical: this seed finds an existing entity
    /// by `(name, entity_type_id)`, so an anchor row carrying a different
    /// entity type is invisible to the find and reaches the create — and
    /// the index refuses it. That has to read as "already seeded", not as
    /// a failed boot.
    #[tokio::test]
    async fn a_seed_that_loses_the_firm_anchor_write_still_succeeds() {
        let surreal = mem_surreal().await;

        // An anchor the seed's own find cannot see: right name, a type it
        // will not look under.
        let decoy_type = crate::entity_types::create(&surreal, "Decoy Type")
            .await
            .unwrap();
        crate::entities::create(
            &surreal,
            &crate::entities::NewEntity {
                name: super::FIRM_ENTITY_NAME.into(),
                entity_type_id: decoy_type.id,
                jurisdiction_id: uuid::Uuid::now_v7(),
                phone: None,
                url: None,
                firm_anchor_key: Some(super::FIRM_ENTITY_NAME.to_lowercase()),
            },
        )
        .await
        .unwrap();

        seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("a seed that loses the anchor write must still succeed");

        let anchors = crate::entities::all(&surreal)
            .await
            .unwrap()
            .into_iter()
            .filter(|row| row.name == super::FIRM_ENTITY_NAME)
            .count();
        assert_eq!(anchors, 1, "the firm anchor must stay a single row");
    }

    /// The firm anchor has moved twice — to `Neon Law` and back to
    /// `Shook Law PLLC` when the practice consolidated under the Neon Law mark
    /// — and every deployment that booted under a previous name carries that
    /// row with the key still on it. The seed skips rows that already exist, so
    /// without reconciliation the outgoing firm would stay undeletable and the
    /// incoming one would be deletable: `delete_unless_firm_anchor` reads
    /// `firm_anchor_key`, not the name.
    ///
    /// The retired partnership is gone from `Entity.yaml`, so the wrong holder
    /// here is any other seeded row. That is the more general statement of the
    /// same rule, and it is what a real database looks like — whatever row last
    /// held the key still holds it until a reseed takes it away.
    #[tokio::test]
    async fn a_reseed_moves_the_anchor_key_off_the_previous_firm() {
        let surreal = mem_surreal().await;
        seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("seed");

        // Rewind to the pre-rename shape: the key sits on a row the seed no
        // longer anchors on, and the anchor carries none.
        let anchor = crate::entities::find_by_name(&surreal, super::FIRM_ENTITY_NAME)
            .await
            .unwrap()
            .expect("the anchor seeds");
        let previous = crate::entities::find_by_name(&surreal, "Neon Law Foundation")
            .await
            .unwrap()
            .expect("the Foundation is an ordinary seeded Entity");
        crate::entities::set_firm_anchor_key(&surreal, anchor.id, None)
            .await
            .unwrap();
        crate::entities::set_firm_anchor_key(
            &surreal,
            previous.id,
            Some("neon law foundation".to_string()),
        )
        .await
        .unwrap();

        seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("a reseed reconciles the anchor");

        let anchor = crate::entities::find_by_id(&surreal, anchor.id)
            .await
            .unwrap()
            .expect("the anchor row survives");
        assert!(
            anchor.is_firm_anchor(),
            "{} must hold the key the delete guard reads",
            super::FIRM_ENTITY_NAME
        );
        let previous = crate::entities::find_by_id(&surreal, previous.id)
            .await
            .unwrap()
            .expect("the previous firm survives as an ordinary row");
        assert!(
            !previous.is_firm_anchor(),
            "Shook Law PLLC must surrender the key and become deletable"
        );
        assert_eq!(
            crate::entities::all(&surreal)
                .await
                .unwrap()
                .into_iter()
                .filter(crate::entities::Entity::is_firm_anchor)
                .count(),
            1,
            "exactly one row may be protected"
        );
    }

    #[tokio::test]
    async fn seeds_full_question_set() {
        let surreal = mem_surreal().await;
        let report = seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("seed");
        // The canonical question catalog is the closed type registry.
        // Template-specific prompt keys live after the `__` discriminator
        // in state names rather than as seeded question rows.
        let expected = QuestionType::all_tokens().len();
        let qs = crate::questions::list_all(&surreal).await.unwrap();
        assert_eq!(qs.len(), expected);
        assert!(qs.iter().any(|q| q.code == "person"));
        assert!(qs.iter().any(|q| q.code == "people"));
        assert!(qs.iter().any(|q| q.code == "custom_text"));
        assert!(qs.iter().any(|q| q.code == "custom_single_choice"));
        assert!(qs.iter().any(|q| q.code == "custom_datetime"));
        assert_eq!(report.questions_inserted, expected);
    }

    #[tokio::test]
    async fn seeds_full_jurisdiction_set() {
        let surreal = mem_surreal().await;
        seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("seed");
        let js = jurisdictions::list_all(&surreal).await.unwrap();
        // 50 states + DC + the ISO 3166-1 country set (alpha-3 codes,
        // with United States and Germany on their pre-ISO codes).
        assert_eq!(js.len(), 248);
        let codes: Vec<&str> = js.iter().map(|j| j.code.as_str()).collect();
        for code in [
            "NV", "CA", "NY", "TX", "WY", "DC", "US", "GMBH", "MEX", "CAN", "GBR",
        ] {
            assert!(codes.contains(&code), "expected `{code}` in jurisdictions");
        }
        // `jurisdiction_type` is reconciled with the seed: states are
        // `state`, sovereigns are `country` — the boundary the `country`
        // question type's option filter rides on.
        let by_code = |c: &str| js.iter().find(|j| j.code == c).unwrap();
        assert_eq!(by_code("NV").jurisdiction_type, "state");
        assert_eq!(by_code("US").jurisdiction_type, "country");
        assert_eq!(by_code("GMBH").jurisdiction_type, "country");
        assert_eq!(by_code("MEX").jurisdiction_type, "country");
        // The state Georgia and the country Georgia stay distinct by
        // name, so a name-keyed answer can never be ambiguous.
        assert_eq!(by_code("GA").name, "Georgia");
        assert_eq!(by_code("GEO").name, "Georgia (country)");
    }

    #[test]
    fn seeded_template_codes_are_derived_from_the_bundled_catalog() {
        let codes = seeded_template_codes().expect("seeded template codes");
        assert_eq!(codes.len(), SEEDED_TEMPLATES.len());
        assert!(codes.iter().any(|code| code == "onboarding__retainer"));
        assert!(codes.iter().any(|code| code == "northstar__will"));
        assert!(
            !codes
                .iter()
                .any(|code| code.starts_with("onboarding__retainer_")),
            "the service-specific retainers are retired; one generic retainer remains"
        );
    }

    #[tokio::test]
    async fn seeds_the_bundled_template_catalog() {
        let surreal = mem_surreal().await;
        let report = seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("seed");
        assert_eq!(
            report.templates_inserted,
            SEEDED_TEMPLATES.len(),
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
                crate::templates::resolve(&surreal, None, code)
                    .await
                    .unwrap()
                    .is_some(),
                "expected bundled template `{code}` to be seeded"
            );
        }
        let tmpl = crate::templates::resolve(&surreal, None, "onboarding__retainer")
            .await
            .unwrap()
            .expect("template row");
        assert_eq!(tmpl.title, "Retainer Agreement");
        assert_eq!(tmpl.respondent_type, "person_and_entity");
        assert!(tmpl.project_id.is_none(), "bundled templates are shared");
        // The body now lives in a blob — fetch it via the storage
        // accessor. Just the markdown body, no frontmatter, so the
        // renderer's dotted glossary interpolation finds
        // its targets.
        let body = crate::templates::body(&surreal, &fs_storage().await, &tmpl)
            .await
            .expect("template body in storage");
        assert!(
            !body.starts_with("---"),
            "body should not include the YAML frontmatter; got {:?}",
            &body[..body.len().min(20)]
        );
        assert!(body.contains("{{person__client.name}}"));
        assert!(body.contains("{{person__client.email}}"));
        assert!(body.contains("{{project__engagement.name}}"));
        assert!(body.contains("{{custom_clauses}}"));
    }

    #[tokio::test]
    async fn template_seeder_is_idempotent_on_second_pass() {
        let surreal = mem_surreal().await;
        let first = seed_canonical(&surreal, &fs_storage().await).await.unwrap();
        let second = seed_canonical(&surreal, &fs_storage().await).await.unwrap();
        assert_eq!(first.templates_inserted, SEEDED_TEMPLATES.len());
        assert_eq!(
            second.templates_inserted, 0,
            "second pass must skip every existing template"
        );
        let count = crate::templates::list_current(&surreal)
            .await
            .unwrap()
            .into_iter()
            .filter(|t| t.code == "onboarding__retainer")
            .count();
        assert_eq!(count, 1, "exactly one current retainer template row");
    }

    /// The firm's one engagement agreement carries the three load-bearing
    /// elements every matter needs: the JAMS arbitration clause (forum
    /// selection only, with the non-waivable fee-arbitration carve-out and
    /// the independent-counsel sentence — never a liability limitation), the
    /// `contact@neonlaw.com` reach-the-Firm clause, and the custom-clause
    /// slot the fee terms and any practice-area ethics reading arrive through.
    ///
    /// These moved here when the twelve service-specific retainers retired:
    /// they were the only bodies carrying them, so without this the firm
    /// would have shipped an engagement agreement with no arbitration clause
    /// and no fee-arbitration disclosure.
    #[tokio::test]
    async fn the_retainer_carries_arbitration_contact_and_the_clause_slot() {
        let surreal = mem_surreal().await;
        seed_canonical(&surreal, &fs_storage().await).await.unwrap();
        let storage = fs_storage().await;

        // Distinctive phrases from the three clauses. Checked against the
        // body with its line wrapping collapsed, so reflowing a paragraph to
        // satisfy the Markdown linter cannot silently drop a clause from
        // this guard.
        let required = [
            "binding arbitration administered by **JAMS**",
            "seated in **Reno, Nevada**",
            "limit, cap, or waive the Firm's responsibility for its own work",
            "right to consult independent counsel of your own choosing before you agree to it",
            "Mandatory Fee Arbitration Act",
            "Washington State Bar Association",
            "Write to contact@neonlaw.com",
            "{{custom_clauses}}",
            "{{client.signature}}",
            "{{firm.signature}}",
        ];

        let code = "onboarding__retainer";
        let tmpl = crate::templates::resolve(&surreal, None, code)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{code} seeded"));
        let body = crate::templates::body(&surreal, &storage, &tmpl)
            .await
            .expect("retainer body");
        let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
        for phrase in required {
            assert!(
                flat.contains(phrase),
                "{code} must carry the clause phrase {phrase:?}"
            );
        }

        // It states the basis of the fee without stating an amount or a
        // cadence: the figure arrives as a custom clause (ENG-146).
        assert!(
            !flat.contains("billed monthly") && !flat.contains("rate sheet attached"),
            "the generic retainer asserts no cadence and no rate sheet"
        );

        // It is practice-neutral. The old body excluded litigation, which
        // would have made the firm's own litigation practice unopenable on
        // its only engagement agreement.
        assert!(
            !flat.contains("does not include litigation"),
            "the engagement agreement must not exclude the firm's own practice areas"
        );

        // The arbitration clause must not read as a liability waiver
        // (RPC 1.8(h)). Guard against a regression that re-introduces
        // limiting language.
        for forbidden in ["limit our liability", "waive any claim against the Firm"] {
            assert!(
                !flat.contains(forbidden),
                "{code} must not limit malpractice liability ({forbidden:?})"
            );
        }

        // Governing law is fillable per engagement (#364 pattern propagated
        // in #363): the clause names the questionnaire variable, not a
        // hardcoded jurisdiction. The token is bare, not a code span, so the
        // letter renderer fills and highlights it like every other
        // placeholder. The arbitration *seat* stays fixed at Reno (asserted
        // above) — venue does not flex with governing law.
        assert!(
            flat.contains("This Agreement is governed by the law of")
                && flat.contains("{{custom_single_choice__governing_law}}"),
            "{code} must fill governing law from the questionnaire, not hardcode it"
        );
        assert!(
            !flat.contains("decided under Nevada law"),
            "{code} must not hardcode 'decided under Nevada law'; use the fillable clause"
        );
    }

    #[tokio::test]
    async fn seed_is_idempotent() {
        let surreal = mem_surreal().await;
        let first = seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("seed 1");
        let second = seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("seed 2");
        assert_eq!(second.questions_inserted, 0);
        assert_eq!(second.jurisdictions_inserted, 0);
        assert_eq!(second.persons_inserted, 0);
        assert!(first.questions_inserted > 0);
    }

    #[tokio::test]
    async fn seed_is_idempotent_when_a_seeded_email_has_different_casing() {
        let surreal = mem_surreal().await;
        seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("initial seed");
        let nick = persons::find_by_email_ci(&surreal, "nick@neonlaw.com")
            .await
            .unwrap()
            .expect("nick exists");
        persons::edit(
            &surreal,
            nick.id,
            &crate::persons::PersonEdit {
                email: Some("Nick@NeonLaw.com".into()),
                ..crate::persons::PersonEdit::default()
            },
        )
        .await
        .expect("re-case Nick's email");

        let rerun = seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("re-seeding must resolve case-insensitive email references");
        assert_eq!(rerun.persons_inserted, 0);
        assert_eq!(
            persons::find_by_email_ci(&surreal, "nick@neonlaw.com")
                .await
                .unwrap()
                .expect("case-insensitive lookup preserves Nick")
                .role,
            Role::Admin
        );
    }

    #[tokio::test]
    async fn seeds_attorney_credentials_with_correct_numbers() {
        let surreal = mem_surreal().await;
        let report = seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("seed");
        let nick = persons::find_by_email_ci(&surreal, "nick@neonlaw.com")
            .await
            .unwrap()
            .expect("nick exists");
        let creds = crate::credentials::for_person(&surreal, nick.id)
            .await
            .unwrap();
        assert_eq!(creds.len(), 3, "expected NV + CA + WA admissions");
        // The state bar numbers are public-record disclosures; pin them
        // explicitly so a seed YAML edit can't silently change the
        // attorney advertising disclosure rendered on the firm site.
        let mut by_juris: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for c in &creds {
            let j = jurisdictions::find_by_id(&surreal, c.jurisdiction_id)
                .await
                .unwrap()
                .expect("the credential's jurisdiction is a seeded Surreal row");
            by_juris.insert(j.code, c.license_number.clone());
        }
        assert_eq!(by_juris.get("NV").map(String::as_str), Some("13400"));
        assert_eq!(by_juris.get("CA").map(String::as_str), Some("337252"));
        assert_eq!(by_juris.get("WA").map(String::as_str), Some("63446"));
        assert_eq!(report.credentials_inserted, 3);
    }

    #[tokio::test]
    async fn user_role_lifts_persons_to_admin() {
        let surreal = mem_surreal().await;
        seed_canonical(&surreal, &fs_storage().await)
            .await
            .expect("seed");
        let nick = persons::find_by_email_ci(&surreal, "nick@neonlaw.com")
            .await
            .unwrap()
            .expect("nick exists");
        assert_eq!(nick.role, Role::Admin);
    }

    #[test]
    fn firm_domain_convention_accepts_lowercase_neon_law_for_organization_roles() {
        use super::require_firm_domain;
        use crate::persons::Role;
        assert!(require_firm_domain("owner@neonlaw.com", Role::Owner).is_ok());
        assert!(require_firm_domain("nick@neonlaw.com", Role::Admin).is_ok());
        assert!(require_firm_domain("clerk@neonlaw.com", Role::Clerk).is_ok());
    }

    #[test]
    fn firm_domain_convention_rejects_mixed_case_privileged_emails() {
        use super::require_firm_domain;
        use crate::persons::Role;
        assert!(require_firm_domain("Owner@NeonLaw.com", Role::Owner).is_err());
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
        use crate::persons::Role;
        assert!(require_firm_domain("libra@example.com", Role::Client).is_ok());
        // Client rows aren't held to lowercase here; that's a normalization
        // concern for the persons table, not the seed convention.
        assert!(require_firm_domain("Libra@Example.com", Role::Client).is_ok());
    }

    #[test]
    fn firm_domain_convention_allows_an_external_lawyer() {
        use super::require_firm_domain;
        use crate::persons::Role;
        assert!(require_firm_domain("counsel@legalaid.example", Role::Lawyer).is_ok());
    }

    #[test]
    fn question_choices_is_empty_after_the_vocabulary_collapse() {
        use super::question_choices;
        // With the vocabulary collapsed to the registry, no seeded question
        // carries a `choices:` block — a one-off choice set (`fee_status`,
        // `management_structure`, …) lives in the template that asks it, as a
        // `custom_single_choice__<key>` state. So the seed reader is empty for
        // every code, and an unknown code still answers with an empty vec
        // rather than panicking.
        assert!(question_choices("custom_single_choice").is_empty());
        assert!(question_choices("custom_text").is_empty());
        assert!(question_choices("no_such_question_code").is_empty());
    }

    /// The seed vocabulary is exactly the closed registry — every question
    /// is a glossary ORM model (record/reference), its plural list form, or
    /// a `custom_*` primitive. No bespoke per-matter codes. This grounds
    /// `Question.yaml` to `store::question_registry::QuestionType` so the two
    /// can never drift (issue #235).
    #[test]
    fn question_yaml_is_exactly_the_registry() {
        use std::collections::BTreeSet;
        let codes: BTreeSet<String> =
            super::parse::<super::QuestionRec>(super::canonical::QUESTION, "Question.yaml")
                .unwrap()
                .into_iter()
                .map(|q| q.code)
                .collect();
        let registry: BTreeSet<String> = crate::question_registry::QuestionType::all_tokens()
            .into_iter()
            .map(str::to_string)
            .collect();
        assert_eq!(
            codes, registry,
            "Question.yaml codes must be exactly store::question_registry::QuestionType"
        );
    }

    /// Every localized prompt maps to a real question code — no orphaned
    /// translations after a rename.
    #[test]
    fn firm_domain_convention_rejects_off_domain_organization_role_seeds() {
        use super::require_firm_domain;
        use crate::persons::Role;
        let err = require_firm_domain("libra@example.com", Role::Clerk).unwrap_err();
        assert!(
            err.to_string().contains("@neonlaw.com"),
            "error should mention the firm domain, got: {err}",
        );
        assert!(require_firm_domain("nick@gmail.com", Role::Admin).is_err());
    }
}
