//! The closed question-type registry — the single source of truth for the
//! `<type>` half of a questionnaire state name (`<type>__<role>`).
//!
//! Every type is either **glossary-grounded** (it maps to a `store::entity`
//! SQL model — a `record` that creates/links a row, or a `reference` that
//! selects a seeded one) or a **custom primitive** (the value lives in the
//! answer JSON, no SQL grounding). Each glossary-grounded type has a
//! **singular** form (one row) and, where a matter collects several, an
//! explicit **plural/aggregate** form (an array of the singular's shape) —
//! `person`→`people`, `entity`→`entities`, and so on. The pairing is
//! explicit because there is no pluralization helper and `person`→`people`
//! is irregular.
//!
//! [`QuestionType`] is that closed set. The guards (`N113`–`N115`), the
//! render/form-fill resolver, and the walkers all read cardinality, shape,
//! and grounding from here rather than re-deriving them — see issue #235.
//! A grounding test pins every record/reference variant to a real
//! `store::entity` and bars every deny-listed table.

use sea_orm::entity::prelude::*;
use sea_orm::Iterable;
use serde::{Deserialize, Serialize};

/// Whether a type creates/links a SQL row (`Record`), selects a seeded one
/// (`Reference`), or carries a primitive value with no SQL grounding
/// (`Custom`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// The answer creates or links a `store::entity` row.
    Record,
    /// The answer selects an existing seeded `store::entity` row.
    Reference,
    /// A primitive value living in the answer JSON; no SQL grounding.
    Custom,
}

/// One row (`Singular`) versus many collected under one question
/// (`Aggregate` — the answer JSON is an array of the singular's shape).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cardinality {
    /// A single record/reference/primitive.
    Singular,
    /// A plural collection — an array of the singular's shape. Barred from
    /// `__for_` (its children are inline).
    Aggregate,
}

/// The parts a `people`/`person` aggregate row collects, in canonical
/// order. The single source the widget assembler and the render/fill
/// resolver key on — the aggregate shape is an array of these fields.
pub const PERSON_ROW_PARTS: [&str; 7] =
    ["name", "title", "street", "city", "state", "zip", "country"];

/// The closed set of question types — the `<type>` half of a
/// `<type>__<role>` state name. Stored as `TEXT`; modelled like
/// [`crate::entity::person::Role`] so the string form round-trips.
#[derive(Clone, Copy, Debug, Eq, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum QuestionType {
    // --- Record types (create/link a SQL row) — singular ---
    #[sea_orm(string_value = "person")]
    Person,
    #[sea_orm(string_value = "entity")]
    Entity,
    #[sea_orm(string_value = "address")]
    Address,
    #[sea_orm(string_value = "role")]
    Role,
    #[sea_orm(string_value = "filing")]
    Filing,
    #[sea_orm(string_value = "credential")]
    Credential,
    #[sea_orm(string_value = "disclosure")]
    Disclosure,
    #[sea_orm(string_value = "issuance")]
    Issuance,
    #[sea_orm(string_value = "signature")]
    Signature,
    #[sea_orm(string_value = "notarization")]
    Notarization,
    // --- Record types — aggregate (array of the singular's shape) ---
    #[sea_orm(string_value = "people")]
    People,
    #[sea_orm(string_value = "entities")]
    Entities,
    #[sea_orm(string_value = "addresses")]
    Addresses,
    #[sea_orm(string_value = "roles")]
    Roles,
    #[sea_orm(string_value = "filings")]
    Filings,
    #[sea_orm(string_value = "credentials")]
    Credentials,
    #[sea_orm(string_value = "disclosures")]
    Disclosures,
    #[sea_orm(string_value = "issuances")]
    Issuances,
    // --- Reference types (select a seeded row) — singular ---
    #[sea_orm(string_value = "jurisdiction")]
    Jurisdiction,
    #[sea_orm(string_value = "entity_type")]
    EntityType,
    #[sea_orm(string_value = "product")]
    Product,
    #[sea_orm(string_value = "statute")]
    Statute,
    #[sea_orm(string_value = "project")]
    Project,
    // --- Reference types — aggregate ---
    #[sea_orm(string_value = "jurisdictions")]
    Jurisdictions,
    #[sea_orm(string_value = "entity_types")]
    EntityTypes,
    #[sea_orm(string_value = "products")]
    Products,
    #[sea_orm(string_value = "statutes")]
    Statutes,
    // --- Custom primitives (value in the answer JSON, no SQL grounding) ---
    #[sea_orm(string_value = "custom_text")]
    CustomText,
    #[sea_orm(string_value = "custom_yes_no")]
    CustomYesNo,
    #[sea_orm(string_value = "custom_single_choice")]
    CustomSingleChoice,
    #[sea_orm(string_value = "custom_multiple_choice")]
    CustomMultipleChoice,
    #[sea_orm(string_value = "custom_usd")]
    CustomUsd,
    #[sea_orm(string_value = "custom_datetime")]
    CustomDatetime,
}

impl QuestionType {
    /// The `<type>` token as it appears in a state name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        use QuestionType::{
            Address, Addresses, Credential, Credentials, CustomDatetime, CustomMultipleChoice,
            CustomSingleChoice, CustomText, CustomUsd, CustomYesNo, Disclosure, Disclosures,
            Entities, Entity, EntityType, EntityTypes, Filing, Filings, Issuance, Issuances,
            Jurisdiction, Jurisdictions, Notarization, People, Person, Product, Products, Project,
            Role, Roles, Signature, Statute, Statutes,
        };
        match self {
            Person => "person",
            Entity => "entity",
            Address => "address",
            Role => "role",
            Filing => "filing",
            Credential => "credential",
            Disclosure => "disclosure",
            Issuance => "issuance",
            Signature => "signature",
            Notarization => "notarization",
            People => "people",
            Entities => "entities",
            Addresses => "addresses",
            Roles => "roles",
            Filings => "filings",
            Credentials => "credentials",
            Disclosures => "disclosures",
            Issuances => "issuances",
            Jurisdiction => "jurisdiction",
            EntityType => "entity_type",
            Product => "product",
            Statute => "statute",
            Project => "project",
            Jurisdictions => "jurisdictions",
            EntityTypes => "entity_types",
            Products => "products",
            Statutes => "statutes",
            CustomText => "custom_text",
            CustomYesNo => "custom_yes_no",
            CustomSingleChoice => "custom_single_choice",
            CustomMultipleChoice => "custom_multiple_choice",
            CustomUsd => "custom_usd",
            CustomDatetime => "custom_datetime",
        }
    }

    /// Parse a `<type>` token into its variant, or `None` if it is not a
    /// registered type. This is the closed-set membership check `N113` runs.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        Self::iter().find(|t| t.as_str() == token)
    }

    /// Parse the `<type>` out of a full `<type>__<role>` state name (or a
    /// bare `<type>`), then look it up. `__for_<role>` children resolve on
    /// their `<type>` prefix just like any other.
    #[must_use]
    pub fn from_state_name(state: &str) -> Option<Self> {
        let token = state.split("__").next().unwrap_or(state);
        Self::from_token(token)
    }

    /// Record, reference, or custom.
    #[must_use]
    pub fn kind(&self) -> Kind {
        use QuestionType::{
            CustomDatetime, CustomMultipleChoice, CustomSingleChoice, CustomText, CustomUsd,
            CustomYesNo, EntityType, EntityTypes, Jurisdiction, Jurisdictions, Product, Products,
            Project, Statute, Statutes,
        };
        match self {
            CustomText | CustomYesNo | CustomSingleChoice | CustomMultipleChoice | CustomUsd
            | CustomDatetime => Kind::Custom,
            Jurisdiction | Jurisdictions | EntityType | EntityTypes | Product | Products
            | Statute | Statutes | Project => Kind::Reference,
            _ => Kind::Record,
        }
    }

    /// Singular (one row/value) or aggregate (an array of the singular's
    /// shape).
    #[must_use]
    pub fn cardinality(&self) -> Cardinality {
        use QuestionType::{
            Addresses, Credentials, Disclosures, Entities, EntityTypes, Filings, Issuances,
            Jurisdictions, People, Products, Roles, Statutes,
        };
        match self {
            People | Entities | Addresses | Roles | Filings | Credentials | Disclosures
            | Issuances | Jurisdictions | EntityTypes | Products | Statutes => {
                Cardinality::Aggregate
            }
            _ => Cardinality::Singular,
        }
    }

    /// The aggregate form of a singular type, if one exists.
    #[must_use]
    pub fn plural(&self) -> Option<Self> {
        use QuestionType::{
            Address, Addresses, Credential, Credentials, Disclosure, Disclosures, Entities, Entity,
            EntityType, EntityTypes, Filing, Filings, Issuance, Issuances, Jurisdiction,
            Jurisdictions, People, Person, Product, Products, Role, Roles, Statute, Statutes,
        };
        Some(match self {
            Person => People,
            Entity => Entities,
            Address => Addresses,
            Role => Roles,
            Filing => Filings,
            Credential => Credentials,
            Disclosure => Disclosures,
            Issuance => Issuances,
            Jurisdiction => Jurisdictions,
            EntityType => EntityTypes,
            Product => Products,
            Statute => Statutes,
            _ => return None,
        })
    }

    /// The singular form of an aggregate type, if this is an aggregate.
    #[must_use]
    pub fn singular(&self) -> Option<Self> {
        use QuestionType::{
            Address, Addresses, Credential, Credentials, Disclosure, Disclosures, Entities, Entity,
            EntityType, EntityTypes, Filing, Filings, Issuance, Issuances, Jurisdiction,
            Jurisdictions, People, Person, Product, Products, Role, Roles, Statute, Statutes,
        };
        Some(match self {
            People => Person,
            Entities => Entity,
            Addresses => Address,
            Roles => Role,
            Filings => Filing,
            Credentials => Credential,
            Disclosures => Disclosure,
            Issuances => Issuance,
            Jurisdictions => Jurisdiction,
            EntityTypes => EntityType,
            Products => Product,
            Statutes => Statute,
            _ => return None,
        })
    }

    /// The `store::entity` table this type grounds to, or `None` for a
    /// custom primitive. Singular and aggregate forms ground to the same
    /// table (the aggregate is many rows of the singular).
    #[must_use]
    pub fn entity_table(&self) -> Option<&'static str> {
        use QuestionType::{
            Address, Addresses, Credential, Credentials, Disclosure, Disclosures, Entities, Entity,
            EntityType, EntityTypes, Filing, Filings, Issuance, Issuances, Jurisdiction,
            Jurisdictions, Notarization, People, Person, Product, Products, Project, Role, Roles,
            Signature, Statute, Statutes,
        };
        Some(match self {
            Person | People => "persons",
            Entity | Entities => "entities",
            Address | Addresses => "addresses",
            Role | Roles => "person_entity_roles",
            Filing | Filings => "filings",
            Credential | Credentials => "credentials",
            Disclosure | Disclosures => "disclosures",
            Issuance | Issuances => "share_issuances",
            Signature => "signatures",
            Notarization => "notarizations",
            Jurisdiction | Jurisdictions => "jurisdictions",
            EntityType | EntityTypes => "entity_types",
            Product | Products => "products",
            Statute | Statutes => "statutes",
            Project => "projects",
            _ => return None,
        })
    }

    /// The glossary term this type documents, for LSP hover and the docs
    /// grammar. `None` for custom primitives, which have no glossary entity.
    #[must_use]
    pub fn glossary_term(&self) -> Option<&'static str> {
        use QuestionType::{
            Address, Addresses, Credential, Credentials, Disclosure, Disclosures, Entities, Entity,
            EntityType, EntityTypes, Filing, Filings, Issuance, Issuances, Jurisdiction,
            Jurisdictions, Notarization, People, Person, Product, Products, Project, Role, Roles,
            Signature, Statute, Statutes,
        };
        Some(match self {
            Person | People => "Person",
            Entity | Entities => "Entity",
            Address | Addresses => "Address",
            Role | Roles => "Person Entity Role",
            Filing | Filings => "Filing",
            Credential | Credentials => "Credential",
            Disclosure | Disclosures => "Disclosure",
            Issuance | Issuances => "Share Issuance",
            Signature => "Signature",
            Notarization => "Notarization",
            Jurisdiction | Jurisdictions => "Jurisdiction",
            EntityType | EntityTypes => "Entity Type",
            Product | Products => "Product",
            Statute | Statutes => "Statute",
            Project => "Project",
            _ => return None,
        })
    }

    /// The row parts an aggregate collects, or `&[]` for a singular. Today
    /// only `people` renders as a multi-part row widget; other aggregates
    /// collect a single reference per row.
    #[must_use]
    pub fn row_parts(&self) -> &'static [&'static str] {
        match self {
            QuestionType::People => &PERSON_ROW_PARTS,
            _ => &[],
        }
    }
}

/// The widget `answer_type` string that denotes an aggregate (plural)
/// question. Walkers and widgets dispatch on this rather than a hardcoded
/// `answer_type == "people_list"` special case: the `people_list` widget
/// collects the `people` aggregate.
#[must_use]
pub fn answer_type_is_aggregate(answer_type: &str) -> bool {
    answer_type == "people_list"
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::EntityName;

    /// Every record/reference type grounds to a real `store::entity` — the
    /// declared `entity_table()` must equal that entity's actual
    /// `table_name()`, so the registry can never drift from the schema.
    #[test]
    fn every_record_and_reference_type_grounds_to_a_real_entity() {
        use crate::entity;
        for qt in QuestionType::iter() {
            let real: Option<&'static str> = match qt {
                QuestionType::Person | QuestionType::People => {
                    Some(entity::person::Entity.table_name())
                }
                QuestionType::Entity | QuestionType::Entities => {
                    Some(entity::entity::Entity.table_name())
                }
                QuestionType::Address | QuestionType::Addresses => {
                    Some(entity::address::Entity.table_name())
                }
                QuestionType::Role | QuestionType::Roles => {
                    Some(entity::person_entity_role::Entity.table_name())
                }
                QuestionType::Filing | QuestionType::Filings => {
                    Some(entity::filing::Entity.table_name())
                }
                QuestionType::Credential | QuestionType::Credentials => {
                    Some(entity::credential::Entity.table_name())
                }
                QuestionType::Disclosure | QuestionType::Disclosures => {
                    Some(entity::disclosure::Entity.table_name())
                }
                QuestionType::Issuance | QuestionType::Issuances => {
                    Some(entity::share_issuance::Entity.table_name())
                }
                QuestionType::Signature => Some(entity::signature::Entity.table_name()),
                QuestionType::Notarization => Some(entity::notarization::Entity.table_name()),
                QuestionType::Jurisdiction | QuestionType::Jurisdictions => {
                    Some(entity::jurisdiction::Entity.table_name())
                }
                QuestionType::EntityType | QuestionType::EntityTypes => {
                    Some(entity::entity_type::Entity.table_name())
                }
                QuestionType::Product | QuestionType::Products => {
                    Some(entity::product::Entity.table_name())
                }
                QuestionType::Statute | QuestionType::Statutes => {
                    Some(entity::statute::Entity.table_name())
                }
                QuestionType::Project => Some(entity::project::Entity.table_name()),
                _ => None,
            };
            match qt.kind() {
                Kind::Custom => assert!(
                    qt.entity_table().is_none() && real.is_none(),
                    "{} is custom and must not ground to a table",
                    qt.as_str()
                ),
                Kind::Record | Kind::Reference => {
                    assert_eq!(
                        qt.entity_table(),
                        real,
                        "{} must ground to its real store::entity table",
                        qt.as_str()
                    );
                }
            }
        }
    }

    /// No question type may point at a deny-listed table — the tables that
    /// are internal artifacts, comms, audit, billing, authz, or governance,
    /// not questionnaire vocabulary (issue #235's deny-list).
    #[test]
    fn no_type_grounds_to_a_deny_listed_table() {
        const DENY: &[&str] = &[
            "questions",
            "answers",
            "blobs",
            "templates",
            "notations",
            "communications",
            "sent_emails",
            "events",
            "git_access_tokens",
            "git_repositories",
            "invoices",
            "xero_invoices",
            "subscriptions",
            "coupons",
            "person_project_roles",
            "testimonials",
            "playbooks",
            "expunge_records",
            "expunge_requests",
            "relationship_edges",
            "letters",
            "mailroom",
            "attestations",
        ];
        for qt in QuestionType::iter() {
            if let Some(table) = qt.entity_table() {
                assert!(
                    !DENY.contains(&table),
                    "{} grounds to deny-listed table `{table}`",
                    qt.as_str()
                );
            }
        }
    }

    #[test]
    fn singular_and_plural_pair_symmetrically() {
        for qt in QuestionType::iter() {
            if let Some(plural) = qt.plural() {
                assert_eq!(qt.cardinality(), Cardinality::Singular);
                assert_eq!(plural.cardinality(), Cardinality::Aggregate);
                assert_eq!(
                    plural.singular(),
                    Some(qt),
                    "{} plural round-trips",
                    qt.as_str()
                );
            }
            if let Some(singular) = qt.singular() {
                assert_eq!(qt.cardinality(), Cardinality::Aggregate);
                assert_eq!(
                    singular.plural(),
                    Some(qt),
                    "{} singular round-trips",
                    qt.as_str()
                );
            }
        }
    }

    #[test]
    fn parses_type_out_of_state_names() {
        assert_eq!(
            QuestionType::from_state_name("entity__company"),
            Some(QuestionType::Entity)
        );
        assert_eq!(
            QuestionType::from_state_name("address__for_trustor"),
            Some(QuestionType::Address)
        );
        assert_eq!(
            QuestionType::from_state_name("custom_single_choice"),
            Some(QuestionType::CustomSingleChoice)
        );
        assert_eq!(QuestionType::from_state_name("not_a_type"), None);
    }

    #[test]
    fn people_row_parts_come_from_the_registry() {
        assert_eq!(QuestionType::People.row_parts(), &PERSON_ROW_PARTS);
        assert!(QuestionType::Person.row_parts().is_empty());
        assert!(answer_type_is_aggregate("people_list"));
        assert!(!answer_type_is_aggregate("string"));
    }
}
