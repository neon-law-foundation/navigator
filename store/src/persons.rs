//! The person directory: the [`Role`] authorization tier and every query
//! that reads or writes a `person` row.
//!
//! # This table lives in SurrealDB
//!
//! Sign-in resolves against this engine: the OIDC callback looks up the
//! row by `oidc_subject` or by email and reads `role` from it.
//!
//! [`Role`] is the system-wide tier every authorization gate evaluates
//! against. It is read from the database row at callback time, never
//! trusted from the OIDC token: the Rauthy (or Google) id_token carries
//! only `sub` and `email`. Authorization stays *above* the database
//! (#1145) — the table keeps `PERMISSIONS NONE` and this module is the
//! only thing that reads or writes it. See
//! [`docs/access-model.md`](../../../docs/access-model.md).
//!
//! # Four engine facts this module is shaped around
//!
//! **An index cannot be defined over an expression.** `DEFINE INDEX …
//! FIELDS string::lowercase(email)` is refused as the statement runs —
//! the engine evaluates the expression at define time and reports
//! `string::lowercase()` receiving `NONE`. So one email per person is
//! enforced by the stored `email_lower` field
//! (`VALUE string::lowercase(email)`) with a plain UNIQUE index on it.
//! Every case-insensitive email match therefore filters that stored
//! field rather than lowercasing `email` in the predicate, so the lookup
//! and the constraint agree by construction.
//!
//! **A unique violation carries no typed detail.** It arrives as
//! [`surrealdb::types::ErrorDetails::Internal`] with the index name in
//! the message and nothing structured to match on, so
//! [`classify_write`] discriminates on the index name — the one part of
//! the text the schema pins — and
//! [`a_duplicate_email_is_reported_as_the_email_being_taken`] holds it
//! against a real engine.
//!
//! **`IF … THEN … ELSE … END` does not parse inside `ORDER BY`.** The
//! authority ladder is therefore ranked in Rust via
//! [`Role::authority_rank`] rather than written a second time in
//! SurrealQL — see [`default_firm_dri`].
//!
//! **The key-value layer is optimistic, so a write can lose a race.**
//! Two writers touching one record conflict, the loser is rolled back,
//! and the engine reports `QueryError::TransactionConflict` — this one
//! typed, unlike the unique violation above. Nothing was wrong with the
//! statement, so [`writing`] re-runs it rather than letting a
//! simultaneous save read as a database fault. How long it re-runs for is
//! not this module's decision — [`crate::surreal::retry`] holds that
//! policy for the whole crate, and `person` is only its most contended
//! caller.
//!
//! # A link is not validated
//!
//! `person_project_role`, `notation`, and the rest reference a person
//! through a `record<person>` link, and the engine accepts one naming a
//! row that was never written. A caller that needs the person behind a
//! link resolves it here, which is what [`find_by_id`] and
//! [`find_by_ids`] exist for.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;
use uuid::Uuid;

use crate::surreal::{record_id, record_uuid, retry, SurrealDb};

/// The table these rows live in.
const TABLE: &str = "person";

/// System-wide authorization tier for a [`Person`]. Stored as a `string`
/// with an `ASSERT $value IN [...]` on the field, which is what closes
/// the vocabulary. Anonymous callers have no row at all.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The accountable principal who owns the Navigator application. Owner
    /// inherits every Admin and Lawyer capability and alone may assign the
    /// Owner role.
    Owner,
    /// A licensed lawyer with system-administration authority. Bypasses
    /// project-scoping entirely and is a member of the lawyer tier.
    Admin,
    /// A licensed lawyer authorized to perform legal work through Navigator.
    /// The lawyer tier may perform work only on assigned projects and supervise
    /// Clerk work where a future Clerk-specific capability permits it. This is
    /// not an employment, email-domain, or source-forge membership grant.
    Lawyer,
    /// A supervised non-lawyer firm worker. This role is intentionally
    /// outside the lawyer tier and receives no `/lawyer`, MCP, Git, or
    /// legal-work authority merely by existing. Narrow Clerk project work
    /// must name its own route and supervision boundary.
    Clerk,
    /// A person the firm represents on at least one matter. Sees
    /// only projects with a matching `person_project_roles` row.
    ///
    /// The default, for both seeded rows and freshly-created ones:
    /// promotion above Client is always opt-in.
    #[default]
    Client,
}

impl Role {
    /// String form used in embedded Rego policy inputs, the URL-encoded
    /// admin form, and the stored `role` field.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Lawyer => "lawyer",
            Self::Clerk => "clerk",
            Self::Client => "client",
        }
    }

    /// The role named by its stored spelling, or `None` for anything
    /// else. The inverse of [`Role::as_str`], and the only way a stored
    /// `role` becomes a [`Role`].
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "lawyer" => Some(Self::Lawyer),
            "clerk" => Some(Self::Clerk),
            "client" => Some(Self::Client),
            _ => None,
        }
    }

    /// The role's place in the application authority ladder.
    ///
    /// Higher roles may govern lower roles. Participation remains a separate
    /// matter-scope fact and is not represented here.
    #[must_use]
    pub const fn authority_rank(self) -> u8 {
        match self {
            Self::Owner => 4,
            Self::Admin => 3,
            Self::Lawyer => 2,
            Self::Clerk => 1,
            Self::Client => 0,
        }
    }

    /// `true` for Owner and Admin — the system-wide tiers that gate
    /// `/admin/*` and bypass project scoping.
    #[must_use]
    pub fn is_admin_tier(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }

    /// `true` only for the accountable Owner tier.
    #[must_use]
    pub fn is_owner(self) -> bool {
        matches!(self, Self::Owner)
    }

    /// `true` for `Lawyer`, `Admin`, and `Owner` — the lawyer tiers that gate
    /// `/lawyer/*` legal work. Clerk is deliberately excluded: its
    /// non-lawyer work must be granted by a separate, supervised capability.
    #[must_use]
    pub fn is_lawyer_tier(self) -> bool {
        matches!(self, Self::Owner | Self::Admin | Self::Lawyer)
    }

    /// `true` when this role is the explicit non-lawyer Clerk tier.
    #[must_use]
    pub fn is_clerk(self) -> bool {
        matches!(self, Self::Clerk)
    }
}

/// One person in the directory.
///
/// The application-facing shape: plain Rust types, no engine handles.
/// [`PersonRow`] is the seam that turns it into (and back out of) what
/// the SDK reads and writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Person {
    pub id: Uuid,
    /// The display name. The structured legal-name parts below are what
    /// a filing that must split the name reads.
    pub name: String,
    /// The person's given (first) name. `None` until a matter needs the
    /// legal name split into parts.
    pub given_name: Option<String>,
    /// The person's family (last) name. `None` until set.
    pub family_name: Option<String>,
    /// The person's middle name, if any. `None` until set, and `None`
    /// for a person with no middle name.
    pub middle_name: Option<String>,
    /// The mailbox, as supplied. Matching is case-insensitive through
    /// the stored `email_lower` field — see [`find_by_email_ci`].
    pub email: String,
    /// OIDC `sub` claim — stable identifier from the IdP (Rauthy,
    /// Google, etc.). `None` for seeded persons not yet linked.
    pub oidc_subject: Option<String>,
    /// System-wide tier.
    pub role: Role,
    /// The contact's role at their organization (e.g. "Executive
    /// Director"). Free text; `None` until set by the importer or an
    /// admin edit.
    pub title: Option<String>,
    /// The contact's direct phone line. `None` until set.
    pub phone: Option<String>,
    /// Xero `ContactID` (GUID) once this person has been mirrored to
    /// Xero Contacts via the billing seam (one-way, Neon Law Navigator →
    /// Xero). `None` until first synced.
    pub xero_contact_id: Option<String>,
    /// Optional public profile image URL. Used only on consented public
    /// attribution surfaces such as testimonials.
    pub profile_image_url: Option<String>,
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The row as the engine reads it. Separate from [`Person`] because the
/// SDK owns its own `RecordId` and `Datetime`, and `role` arrives as the
/// stored string; the conversion belongs at this seam rather than in
/// every caller.
#[derive(SurrealValue)]
struct PersonRow {
    id: surrealdb::types::RecordId,
    name: String,
    given_name: Option<String>,
    family_name: Option<String>,
    middle_name: Option<String>,
    email: String,
    oidc_subject: Option<String>,
    role: String,
    title: Option<String>,
    phone: Option<String>,
    xero_contact_id: Option<String>,
    profile_image_url: Option<String>,
    inserted_at: surrealdb::types::Datetime,
    updated_at: surrealdb::types::Datetime,
}

impl PersonRow {
    /// `None` when the record id is not a native UUID key (see
    /// [`crate::surreal`] for why the two key spellings differ) or when
    /// `role` is not one the ladder names. Both are rows this workspace
    /// could not have written; reporting them as a [`Person`] would
    /// invent an id or an authority tier.
    fn into_person(self) -> Option<Person> {
        Some(Person {
            id: record_uuid(&self.id)?,
            name: self.name,
            given_name: self.given_name,
            family_name: self.family_name,
            middle_name: self.middle_name,
            email: self.email,
            oidc_subject: self.oidc_subject,
            role: Role::parse(&self.role)?,
            title: self.title,
            phone: self.phone,
            xero_contact_id: self.xero_contact_id,
            profile_image_url: self.profile_image_url,
            inserted_at: self.inserted_at.into(),
            updated_at: self.updated_at.into(),
        })
    }
}

/// The projection every read shares, so one field list describes the row
/// and a new column cannot reach [`PersonRow`] from only one query.
/// `email_lower` is deliberately absent: it is a stored derivation of
/// `email` that exists for the unique index, not a fact a caller needs.
const SELECT: &str = "id, name, given_name, family_name, middle_name, email, oidc_subject, \
                      role, title, phone, xero_contact_id, profile_image_url, \
                      inserted_at, updated_at";

/// Errors reading or writing a person.
#[derive(Debug, thiserror::Error)]
pub enum PersonError {
    /// A database operation failed.
    #[error("database: {0}")]
    Db(#[from] surrealdb::Error),
    /// The write collided with `person_email_lower` — another row
    /// already holds this mailbox, case-insensitively.
    #[error("that email is already in use")]
    EmailTaken,
    /// The write collided with `person_oidc_subject` — another row is
    /// already linked to this IdP identity.
    #[error("that IdP identity is already linked to another person")]
    OidcSubjectTaken,
    /// A write reported success but returned no row, or returned one
    /// this module could not read back — see [`PersonRow::into_person`].
    #[error("writing a person returned no usable row")]
    WriteReturnedNothing,
}

/// Turn a write failure into the caller-correctable conflict it names,
/// or leave it as a database fault.
///
/// A unique violation carries **no typed detail** — the engine reports
/// it as `ErrorDetails::Internal`, so there is nothing structured to
/// match on and the index name in the message is the only discriminator
/// available. That is not pattern-matching on prose: the names are
/// `DEFINE INDEX` identifiers this workspace chose in
/// `store/src/schema/navigator.surql`, and
/// [`a_duplicate_email_is_reported_as_the_email_being_taken`] pins each
/// one against a real engine so a rename cannot silently reclassify a
/// conflict as a server fault.
fn classify_write(error: surrealdb::Error) -> PersonError {
    let message = error.to_string();
    if message.contains("person_email_lower") {
        PersonError::EmailTaken
    } else if message.contains("person_oidc_subject") {
        PersonError::OidcSubjectTaken
    } else {
        PersonError::Db(error)
    }
}

/// Run a write under the shared retry policy
/// ([`crate::surreal::retry`]), classifying whatever finally comes back
/// as a person-shaped error.
///
/// Only the mapping lives here. How long a lost race is re-run, and
/// which engine conditions count as a lost race, are one policy for the
/// whole crate — see
/// [`a_write_that_loses_an_optimistic_race_is_retried_not_surfaced`].
async fn writing<F, Q>(attempt: F) -> Result<surrealdb::IndexedResults, PersonError>
where
    F: FnMut() -> Q,
    Q: std::future::IntoFuture<Output = Result<surrealdb::IndexedResults, surrealdb::Error>>,
{
    retry::writing(attempt).await.map_err(classify_write)
}

/// The fields a new person row carries. Everything but `name` and
/// `email` has a sensible empty default, so a caller that only knows
/// those two writes them and takes the rest.
#[derive(Debug, Clone, Default)]
pub struct NewPerson {
    pub name: String,
    pub email: String,
    /// Defaults to [`Role::Client`]: promotion is always opt-in.
    pub role: Role,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub middle_name: Option<String>,
    pub oidc_subject: Option<String>,
    pub title: Option<String>,
    pub phone: Option<String>,
    pub profile_image_url: Option<String>,
}

impl NewPerson {
    /// The common case: a display name and a mailbox, everything else
    /// defaulted.
    #[must_use]
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            email: email.into(),
            ..Self::default()
        }
    }

    /// The same, at an explicit authority tier.
    #[must_use]
    pub fn with_role(name: impl Into<String>, email: impl Into<String>, role: Role) -> Self {
        Self {
            role,
            ..Self::new(name, email)
        }
    }
}

/// The fields an edit may change, in the shape the People command
/// boundary needs: a `None` leaves the column alone, and the doubled
/// option on a structured name part keeps "clear it" distinct from
/// "don't touch it" — the PATCH distinction a single option collapses.
#[derive(Debug, Clone, Default)]
// The doubled option is the whole point — see the doc comment.
#[allow(clippy::option_option)]
pub struct PersonEdit {
    pub name: Option<String>,
    pub email: Option<String>,
    pub role: Option<Role>,
    pub given_name: Option<Option<String>>,
    pub family_name: Option<Option<String>>,
    pub middle_name: Option<Option<String>>,
}

/// The contact facts a directory import owns: the display name and the
/// two free-text contact columns. Separate from [`PersonEdit`] because
/// an import must never touch `role` or `email` — the row it found is
/// the row it updates, and a person promoted to lawyer stays promoted.
#[derive(Debug, Clone, Default)]
pub struct ContactUpdate {
    pub name: String,
    pub title: Option<String>,
    pub phone: Option<String>,
}

/// Read one person out of a query response, dropping a row this module
/// could not have written.
fn one(mut response: surrealdb::IndexedResults) -> Result<Option<Person>, PersonError> {
    let row: Option<PersonRow> = response.take(0)?;
    Ok(row.and_then(PersonRow::into_person))
}

/// Read every person out of a query response, in the order the engine
/// returned them.
fn many(mut response: surrealdb::IndexedResults) -> Result<Vec<Person>, PersonError> {
    let rows: Vec<PersonRow> = response.take(0)?;
    Ok(rows
        .into_iter()
        .filter_map(PersonRow::into_person)
        .collect())
}

/// Resolve a person by id.
///
/// # Errors
///
/// [`PersonError::Db`] if the lookup fails.
pub async fn find_by_id(db: &SurrealDb, id: Uuid) -> Result<Option<Person>, PersonError> {
    let response = db
        .query(format!("SELECT {SELECT} FROM ONLY $id LIMIT 1"))
        .bind(("id", record_id(TABLE, id)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    one(response)
}

/// Resolve several people at once, for a caller holding a batch of
/// `person_id`s from a table that has not moved yet. Ids with no row are
/// simply absent from the result, so the caller sees exactly who exists.
///
/// # Errors
///
/// [`PersonError::Db`] if the lookup fails.
pub async fn find_by_ids(db: &SurrealDb, ids: &[Uuid]) -> Result<Vec<Person>, PersonError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let records: Vec<surrealdb::types::RecordId> =
        ids.iter().map(|id| record_id(TABLE, *id)).collect();
    let response = db
        .query(format!("SELECT {SELECT} FROM person WHERE id IN $ids"))
        .bind(("ids", records))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    many(response)
}

/// Resolve a person by `email`, matched case-insensitively.
///
/// Email is a case-insensitive identifier: `Attorney@Example.com` and
/// `attorney@example.com` are the same mailbox, and an IdP may present a
/// casing that differs from the stored row. Every lookup keyed on email
/// goes through here, filtering the stored `email_lower` field so it
/// agrees with the `person_email_lower` unique index by construction —
/// the engine rejects an expression index, so lowercasing in the
/// predicate instead would leave the lookup and the constraint able to
/// disagree.
///
/// # Errors
///
/// [`PersonError::Db`] if the lookup fails.
pub async fn find_by_email_ci(db: &SurrealDb, email: &str) -> Result<Option<Person>, PersonError> {
    let response = db
        .query(format!(
            "SELECT {SELECT} FROM ONLY person WHERE email_lower = $email LIMIT 1"
        ))
        .bind(("email", email.trim().to_lowercase()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    one(response)
}

/// Resolve a person by their IdP `sub` claim — the first thing the OIDC
/// callback asks, before falling back to the email.
///
/// # Errors
///
/// [`PersonError::Db`] if the lookup fails.
pub async fn find_by_oidc_subject(
    db: &SurrealDb,
    subject: &str,
) -> Result<Option<Person>, PersonError> {
    let response = db
        .query(format!(
            "SELECT {SELECT} FROM ONLY person WHERE oidc_subject = $subject LIMIT 1"
        ))
        .bind(("subject", subject.to_string()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    one(response)
}

/// The firm-side person to designate as a matter's lawyer DRI when a create
/// path has no explicit opener — the self-serve intake (no lawyer in the
/// room), the CLI, and AIDA's tool calls. Returns the lowest-id `owner`,
/// else the lowest-id `admin`, else the lowest-id `lawyer` — i.e. the firm
/// principal in a seeded install, resolved by **role**, not a hard-coded
/// email, so a white-label fork gets its own principal with no code
/// change. `None` only on a database with no firm-side person at all
/// (which the caller treats as an error).
///
/// The ladder is ranked here rather than in the query: `IF … THEN … ELSE
/// … END` does not parse inside `ORDER BY`, and the alternative — three
/// ordered queries, or the ladder spelled a second time in SurrealQL —
/// would let [`Role::authority_rank`] and the resolver drift apart.
///
/// # Errors
///
/// [`PersonError::Db`] if the lookup fails.
pub async fn default_firm_dri(db: &SurrealDb) -> Result<Option<Uuid>, PersonError> {
    let firm_side = [Role::Owner, Role::Admin, Role::Lawyer].map(|role| role.as_str().to_string());
    let response = db
        .query(format!(
            "SELECT {SELECT} FROM person WHERE role IN $roles ORDER BY id ASC"
        ))
        .bind(("roles", firm_side.to_vec()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;

    // `ORDER BY id ASC` settles the tie inside a tier, and `min_by_key`
    // returns the FIRST minimum — so ranking by the reversed authority
    // gives the lowest-id row of the highest tier. (`max_by_key` returns
    // the *last* maximum, which would pick the highest id in the tier.)
    Ok(many(response)?
        .into_iter()
        .min_by_key(|person| std::cmp::Reverse(person.role.authority_rank()))
        .map(|person| person.id))
}

/// The person directory the lawyer people page renders, filtered and sorted by
/// the JSON:API `?sort=` / `filter[...]` query parameters. `filter_name` /
/// `filter_email` are case-insensitive substring matches (empty = no filter);
/// `sort` is a list of `(key, descending)` pairs where `key` is `"name"` or
/// `"email"` (any other key is ignored — the caller validates and 400s on an
/// unadvertised field before reaching here). With no sort, rows come back
/// ordered by display name. The shared base query so the Dioxus people
/// component (issue #641 / #355) and the existing admin handler agree.
///
/// # Errors
///
/// [`PersonError::Db`] if the lookup fails.
pub async fn list_directory(
    db: &SurrealDb,
    filter_name: &str,
    filter_email: &str,
    sort: &[(String, bool)],
) -> Result<Vec<Person>, PersonError> {
    let mut order = Vec::new();
    for (key, descending) in sort {
        // The column list is a whitelist, never the caller's string —
        // an unadvertised key is ignored here and 400s upstream.
        let column = match key.as_str() {
            "name" => "name",
            "email" => "email",
            _ => continue,
        };
        order.push(format!(
            "{column} {}",
            if *descending { "DESC" } else { "ASC" }
        ));
    }
    if order.is_empty() {
        order.push("name ASC".to_string());
    }

    let response = db
        .query(format!(
            "SELECT {SELECT} FROM person \
             WHERE ($name = '' OR string::contains(string::lowercase(name), $name)) \
               AND ($email = '' OR string::contains(email_lower, $email)) \
             ORDER BY {}",
            order.join(", ")
        ))
        .bind(("name", filter_name.to_lowercase()))
        .bind(("email", filter_email.to_lowercase()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    many(response)
}

/// Fuzzy-find people by an optional name and/or email substring. Both
/// needles are matched case-insensitively as substrings and ANDed when
/// both are supplied; the caller is responsible for rejecting the
/// all-`None` case (a blank query would return the whole directory).
/// Results are ordered by name and capped at `limit`.
///
/// This is the read half of the People command boundary: the AIDA
/// `aida_show_person` tool and any web lookup share this one query
/// instead of re-implementing the predicate.
///
/// # Errors
///
/// [`PersonError::Db`] if the lookup fails.
pub async fn search(
    db: &SurrealDb,
    name: Option<&str>,
    email: Option<&str>,
    limit: u64,
) -> Result<Vec<Person>, PersonError> {
    let name = name.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
    let email = email.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
    let response = db
        .query(format!(
            "SELECT {SELECT} FROM person \
             WHERE ($name = '' OR string::contains(string::lowercase(name), $name)) \
               AND ($email = '' OR string::contains(email_lower, $email)) \
             ORDER BY name ASC LIMIT $limit"
        ))
        .bind(("name", name.to_lowercase()))
        .bind(("email", email.to_lowercase()))
        .bind(("limit", i64::try_from(limit).unwrap_or(i64::MAX)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    many(response)
}

/// Write a new person row.
///
/// The record id is minted from a fresh v7 `Uuid` through
/// [`crate::surreal::record_id`], so the key stays the native UUID
/// spelling every cross-engine `person_id` still addresses.
///
/// # Errors
///
/// [`PersonError::EmailTaken`] when another row already holds this
/// mailbox case-insensitively, [`PersonError::OidcSubjectTaken`] when
/// another row is already linked to this IdP identity, and
/// [`PersonError::Db`] for anything else.
pub async fn create(db: &SurrealDb, input: &NewPerson) -> Result<Person, PersonError> {
    let id = Uuid::now_v7();
    let response = writing(|| {
        db.query(format!(
            "CREATE $id SET \
             name = $name, \
             email = $email, \
             role = $role, \
             given_name = $given_name, \
             family_name = $family_name, \
             middle_name = $middle_name, \
             oidc_subject = $oidc_subject, \
             title = $title, \
             phone = $phone, \
             profile_image_url = $profile_image_url \
             RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, id)))
        .bind(("name", input.name.trim().to_string()))
        .bind(("email", input.email.trim().to_string()))
        .bind(("role", input.role.as_str().to_string()))
        .bind(("given_name", input.given_name.clone()))
        .bind(("family_name", input.family_name.clone()))
        .bind(("middle_name", input.middle_name.clone()))
        .bind(("oidc_subject", input.oidc_subject.clone()))
        .bind(("title", input.title.clone()))
        .bind(("phone", input.phone.clone()))
        .bind(("profile_image_url", input.profile_image_url.clone()))
    })
    .await?;

    one(response)?.ok_or(PersonError::WriteReturnedNothing)
}

/// The person holding this mailbox, creating them if nobody does.
///
/// The canonical seed runs on every boot and every `navigator db list`,
/// so two processes can start together. The email probe and the `CREATE`
/// therefore share one transaction: concurrent callers read and write the
/// same index key, making a loser retry the whole decision instead of
/// letting two independent creates both commit that key.
///
/// Only the mailbox is matched. A caller that also needs the name or role
/// to be right brings them up to date itself — this settles identity, not
/// content.
///
/// # Errors
///
/// [`PersonError::OidcSubjectTaken`] when `input` carries an IdP identity
/// another person already holds — a real conflict, not a race — and
/// [`PersonError::Db`] for anything else.
pub async fn find_or_create(db: &SurrealDb, input: &NewPerson) -> Result<Person, PersonError> {
    let mut response = writing(|| {
        db.query(format!(
            "BEGIN; \
             LET $existing = (SELECT VALUE id FROM {TABLE} \
                 WHERE email_lower = $email_lower LIMIT 1)[0]; \
             IF $existing = NONE {{ \
                 CREATE $id SET \
                     name = $name, \
                     email = $email, \
                     role = $role, \
                     given_name = $given_name, \
                     family_name = $family_name, \
                     middle_name = $middle_name, \
                     oidc_subject = $oidc_subject, \
                     title = $title, \
                     phone = $phone, \
                     profile_image_url = $profile_image_url; \
             }}; \
             SELECT {SELECT} FROM ONLY {TABLE} WHERE email_lower = $email_lower LIMIT 1; \
             COMMIT;"
        ))
        .bind(("id", record_id(TABLE, Uuid::now_v7())))
        .bind(("name", input.name.trim().to_string()))
        .bind(("email", input.email.trim().to_string()))
        .bind(("email_lower", input.email.trim().to_lowercase()))
        .bind(("role", input.role.as_str().to_string()))
        .bind(("given_name", input.given_name.clone()))
        .bind(("family_name", input.family_name.clone()))
        .bind(("middle_name", input.middle_name.clone()))
        .bind(("oidc_subject", input.oidc_subject.clone()))
        .bind(("title", input.title.clone()))
        .bind(("phone", input.phone.clone()))
        .bind(("profile_image_url", input.profile_image_url.clone()))
    })
    .await?;

    // `BEGIN` and `LET` occupy slots 0 and 1; the `IF` occupies slot 2,
    // so the canonical read is slot 3 before `COMMIT`.
    let row: Option<PersonRow> = response.take(3)?;
    row.and_then(PersonRow::into_person)
        .ok_or(PersonError::WriteReturnedNothing)
}

/// Apply an update statement to one person, returning the row as it now
/// stands or `None` when no such person exists.
///
/// `UPDATE` never creates: the engine checks the record exists before it
/// touches anything, so a stale cross-engine `person_id` updates nothing
/// and reads back as `None`. That is `UPSERT`'s job, and nothing in this
/// module reaches for it — a person is created only through [`create`],
/// which mints its own key. See
/// [`an_update_never_creates_the_person_upsert_would_have`].
async fn update_one(
    db: &SurrealDb,
    id: Uuid,
    assignments: &str,
    bindings: Vec<(&'static str, surrealdb::types::Value)>,
) -> Result<Option<Person>, PersonError> {
    let response = writing(|| {
        let mut query = db
            .query(format!(
                "UPDATE person SET {assignments}, updated_at = time::now() \
                 WHERE id = $id RETURN {SELECT}"
            ))
            .bind(("id", record_id(TABLE, id)));
        // Rebound each attempt: awaiting a `Query` consumes it, so a
        // retry builds a fresh one rather than reusing a spent handle.
        for binding in bindings.iter().cloned() {
            query = query.bind(binding);
        }
        query
    })
    .await?;
    one(response)
}

/// A bound value, in the shape [`update_one`] collects them.
fn bind<T: SurrealValue>(name: &'static str, value: T) -> (&'static str, surrealdb::types::Value) {
    (name, value.into_value())
}

/// Edit a person's directory fields. `None` on a field leaves the column
/// alone; a present-but-`None` structured name part clears it. Returns
/// `None` when the person no longer exists.
///
/// This is the persistence half of the People command boundary — the
/// bootstrap-owner guard, the authority-ladder check, and the validation
/// live in [`crate::people_commands`], which calls this once it has
/// decided the edit is allowed.
///
/// # Errors
///
/// [`PersonError::EmailTaken`] when the new email belongs to another
/// row, and [`PersonError::Db`] for anything else.
pub async fn edit(
    db: &SurrealDb,
    id: Uuid,
    input: &PersonEdit,
) -> Result<Option<Person>, PersonError> {
    let mut assignments: Vec<&str> = Vec::new();
    if input.name.is_some() {
        assignments.push("name = $name");
    }
    if input.email.is_some() {
        assignments.push("email = $email");
    }
    if input.role.is_some() {
        assignments.push("role = $role");
    }
    if input.given_name.is_some() {
        assignments.push("given_name = $given_name");
    }
    if input.family_name.is_some() {
        assignments.push("family_name = $family_name");
    }
    if input.middle_name.is_some() {
        assignments.push("middle_name = $middle_name");
    }
    if assignments.is_empty() {
        return find_by_id(db, id).await;
    }

    update_one(
        db,
        id,
        &assignments.join(", "),
        vec![
            bind("name", input.name.as_ref().map(|v| v.trim().to_string())),
            bind("email", input.email.as_ref().map(|v| v.trim().to_string())),
            bind("role", input.role.map(|role| role.as_str().to_string())),
            bind("given_name", input.given_name.clone().unwrap_or_default()),
            bind("family_name", input.family_name.clone().unwrap_or_default()),
            bind("middle_name", input.middle_name.clone().unwrap_or_default()),
        ],
    )
    .await
}

/// Set a person's authority tier. Returns `None` when the person no
/// longer exists.
///
/// # Errors
///
/// [`PersonError::Db`] if the write fails.
pub async fn set_role(db: &SurrealDb, id: Uuid, role: Role) -> Result<Option<Person>, PersonError> {
    update_one(
        db,
        id,
        "role = $role",
        vec![bind("role", role.as_str().to_string())],
    )
    .await
}

/// Cache the Xero `ContactID` on a person. No-op (`Ok(None)`) when the
/// person row no longer exists. Idempotent: re-setting the same id just
/// bumps `updated_at`.
///
/// # Errors
///
/// [`PersonError::Db`] if the write fails.
pub async fn set_xero_contact_id(
    db: &SurrealDb,
    id: Uuid,
    xero_contact_id: &str,
) -> Result<Option<Person>, PersonError> {
    update_one(
        db,
        id,
        "xero_contact_id = $xero",
        vec![bind("xero", xero_contact_id.to_string())],
    )
    .await
}

/// Link a person to the IdP identity that just authenticated as them.
/// Returns `None` when the person no longer exists.
///
/// # Errors
///
/// [`PersonError::OidcSubjectTaken`] when another row already holds this
/// `sub`, and [`PersonError::Db`] for anything else.
pub async fn link_oidc_subject(
    db: &SurrealDb,
    id: Uuid,
    subject: &str,
) -> Result<Option<Person>, PersonError> {
    update_one(
        db,
        id,
        "oidc_subject = $subject",
        vec![bind("subject", subject.to_string())],
    )
    .await
}

/// Apply a directory import's contact facts: the display name and the
/// two free-text contact columns. Deliberately cannot reach `email` or
/// `role` — a re-import is authoritative for how to reach someone, never
/// for who they are or what they may do.
///
/// # Errors
///
/// [`PersonError::Db`] if the write fails.
pub async fn update_contact(
    db: &SurrealDb,
    id: Uuid,
    input: &ContactUpdate,
) -> Result<Option<Person>, PersonError> {
    update_one(
        db,
        id,
        "name = $name, title = $title, phone = $phone",
        vec![
            bind("name", input.name.trim().to_string()),
            bind("title", input.title.clone()),
            bind("phone", input.phone.clone()),
        ],
    )
    .await
}

/// Remove a person. Idempotent: deleting one that is not there is a
/// no-op.
///
/// Whether a person *may* be deleted — only clients, never the bootstrap
/// Owner — is [`crate::people_commands::delete_person`]'s question, asked
/// before this is called.
///
/// # Errors
///
/// [`PersonError::Db`] if the delete fails.
pub async fn delete(db: &SurrealDb, id: Uuid) -> Result<(), PersonError> {
    writing(|| db.query("DELETE $id").bind(("id", record_id(TABLE, id)))).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        create, default_firm_dri, delete, edit, find_by_email_ci, find_by_id, find_by_ids,
        find_by_oidc_subject, find_or_create, link_oidc_subject, list_directory, retry, search,
        set_role, set_xero_contact_id, update_contact, ContactUpdate, NewPerson, PersonEdit,
        PersonError, Role,
    };
    use crate::surreal::test_support::mem;
    use crate::surreal::{record_id, SurrealDb};
    use uuid::Uuid;

    /// How many writers the contention assertions race for one record.
    ///
    /// Not a round number for its own sake: it is the herd size at which
    /// a counted five-attempt budget gives up on about 21% of individual
    /// writes, so at least one racer is refused on essentially every
    /// run. Smaller herds fail only intermittently, which is why a
    /// budget that does not scale with contention reads as a flake
    /// rather than as the policy defect it is.
    const CONTENDED_WRITERS: usize = 32;

    async fn person(db: &SurrealDb, name: &str, email: &str) -> super::Person {
        create(db, &NewPerson::new(name, email)).await.unwrap()
    }

    async fn person_at(db: &SurrealDb, name: &str, email: &str, role: Role) -> super::Person {
        create(db, &NewPerson::with_role(name, email, role))
            .await
            .unwrap()
    }

    #[test]
    fn lawyer_and_clerk_tiers_are_disjoint() {
        assert!(Role::Owner.is_lawyer_tier());
        assert!(Role::Owner.is_admin_tier());
        assert!(Role::Owner.is_owner());
        assert_eq!(Role::Owner.authority_rank(), 4);
        assert!(Role::Admin.is_admin_tier());
        assert!(!Role::Admin.is_owner());
        assert!(Role::Lawyer.is_lawyer_tier());
        assert!(Role::Admin.is_lawyer_tier());
        assert!(!Role::Clerk.is_lawyer_tier());
        assert!(Role::Clerk.is_clerk());
        assert!(!Role::Client.is_clerk());
    }

    #[test]
    fn every_role_round_trips_through_its_stored_spelling() {
        // The stored spelling is what the schema's `ASSERT $value IN
        // [...]` names and what `PersonRow` reads back, so a role that
        // did not round-trip would be a row this module wrote and then
        // could not load.
        for role in [
            Role::Owner,
            Role::Admin,
            Role::Lawyer,
            Role::Clerk,
            Role::Client,
        ] {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        assert_eq!(Role::parse("wizard"), None);
        assert_eq!(Role::default(), Role::Client);
    }

    #[tokio::test]
    async fn a_created_person_reads_back_by_id_and_by_email() {
        let db = mem().await;
        let created = create(
            &db,
            &NewPerson {
                given_name: Some("Libra".into()),
                family_name: Some("Scales".into()),
                title: Some("Executive Director".into()),
                phone: Some("+1-555-0100".into()),
                ..NewPerson::with_role("Libra Scales", "libra@example.com", Role::Lawyer)
            },
        )
        .await
        .unwrap();

        assert_eq!(created.role, Role::Lawyer);
        assert_eq!(created.given_name.as_deref(), Some("Libra"));
        assert_eq!(created.title.as_deref(), Some("Executive Director"));
        assert!(created.oidc_subject.is_none());
        assert!(created.xero_contact_id.is_none());

        assert_eq!(find_by_id(&db, created.id).await.unwrap(), Some(created));
    }

    #[tokio::test]
    async fn create_trims_the_name_and_email() {
        let db = mem().await;
        let row = person(&db, "  Libra ", "  libra@example.com ").await;
        assert_eq!(row.name, "Libra");
        assert_eq!(row.email, "libra@example.com");
    }

    #[tokio::test]
    async fn create_defaults_the_role_to_client() {
        let db = mem().await;
        assert_eq!(
            person(&db, "Libra", "libra@example.com").await.role,
            Role::Client
        );
    }

    #[tokio::test]
    async fn find_by_id_is_none_for_a_person_who_never_existed() {
        let db = mem().await;
        assert!(find_by_id(&db, Uuid::now_v7()).await.unwrap().is_none());
    }

    /// The auth path's whole contract: an IdP may present a casing the
    /// stored row does not have, and it must still resolve.
    #[tokio::test]
    async fn email_matches_case_insensitively_in_both_directions() {
        let db = mem().await;
        let stored = person(&db, "Attorney", "Attorney@Example.com").await;

        for probe in [
            "attorney@example.com",
            "ATTORNEY@EXAMPLE.COM",
            "Attorney@Example.com",
            "  attorney@example.com  ",
        ] {
            assert_eq!(
                find_by_email_ci(&db, probe).await.unwrap().map(|p| p.id),
                Some(stored.id),
                "{probe} did not resolve to the stored row"
            );
        }
        assert!(find_by_email_ci(&db, "someone@example.com")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn a_duplicate_email_is_reported_as_the_email_being_taken() {
        let db = mem().await;
        person(&db, "Libra", "dup@example.com").await;

        // Byte-identical, and the case variant the `email_lower` index
        // exists to catch. Both are the same mailbox.
        for duplicate in ["dup@example.com", "DUP@Example.com"] {
            let refused = create(&db, &NewPerson::new("Other", duplicate)).await;
            assert!(
                matches!(refused, Err(PersonError::EmailTaken)),
                "{duplicate} was not classified as a taken email: {refused:?}"
            );
        }
    }

    /// A write that loses an optimistic-concurrency race is retried, not
    /// surfaced.
    ///
    /// SurrealDB's key-value layer is optimistic: concurrent
    /// read-modify-write passes over one record race, the loser is rolled
    /// back, and the engine says so — with a message that ends "This
    /// transaction can be retried". Nothing about the statement was
    /// wrong, so surfacing it would make two simultaneous saves look like
    /// a database fault to whoever lost. That is what the cucumber suite
    /// hit, since its scenarios share one engine.
    ///
    /// Contention on ONE record, not many: separate records touch
    /// separate keys and do not conflict. That is what makes this a test
    /// of the retry rather than of concurrency in general.
    ///
    /// [`CONTENDED_WRITERS`] is why it is a test of the retry *policy*
    /// as well. A counted five-attempt budget drains a herd of eight
    /// about 98% of the time, so at that size the property holds on most
    /// runs and the rest reads as noise. At the herd size below such a
    /// budget fails essentially every run, which is what a property is
    /// supposed to look like when it does not hold.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_write_that_loses_an_optimistic_race_is_retried_not_surfaced() {
        let db = mem().await;
        let subject = person(&db, "Contended", "contended@example.com").await.id;
        let roles = [
            Role::Owner,
            Role::Admin,
            Role::Lawyer,
            Role::Clerk,
            Role::Client,
        ];

        let writes: Vec<_> = (0..CONTENDED_WRITERS)
            .map(|n| {
                let db = db.clone();
                let role = roles[n % roles.len()];
                tokio::spawn(async move { set_role(&db, subject, role).await })
            })
            .collect();

        for (n, write) in writes.into_iter().enumerate() {
            let result = write.await.expect("write task");
            assert!(
                result.is_ok(),
                "concurrent write {n} was surfaced instead of retried: {result:?}",
            );
        }
    }

    /// [`find_or_create`] settles on one row when callers race for the
    /// same mailbox — the property the canonical seed depends on, since
    /// it runs on every boot and two processes can start together.
    ///
    /// Every racer must return the canonical row. The transaction makes
    /// concurrent decisions touch the same email key, so the shared write
    /// retry policy re-runs a losing decision and its final read observes
    /// the winner.
    #[tokio::test(flavor = "multi_thread")]
    async fn concurrent_find_or_create_for_one_mailbox_settles_on_one_row() {
        let db = mem().await;

        let racers: Vec<_> = (0..8)
            .map(|_| {
                let db = db.clone();
                tokio::spawn(async move {
                    find_or_create(&db, &NewPerson::new("Contested", "contested@example.com")).await
                })
            })
            .collect();

        let mut ids = std::collections::BTreeSet::new();
        for (n, racer) in racers.into_iter().enumerate() {
            let person = racer
                .await
                .expect("racer task")
                .unwrap_or_else(|e| panic!("racer {n} was refused instead of settling: {e:?}"));
            ids.insert(person.id);
        }

        assert_eq!(ids.len(), 1, "the racers disagreed about which row won");
        assert_eq!(
            list_directory(&db, "", "", &[]).await.unwrap().len(),
            1,
            "a race must not leave a second row behind",
        );
    }

    /// A pre-existing row is returned as it stands. `find_or_create`
    /// settles identity, not content: it must not quietly rewrite a name
    /// or demote a role to match what the caller happened to pass.
    #[tokio::test]
    async fn find_or_create_returns_the_existing_row_untouched() {
        let db = mem().await;
        let seeded = person_at(&db, "Original", "held@example.com", Role::Admin).await;

        let found = find_or_create(
            &db,
            &NewPerson::with_role("Different", "HELD@example.com", Role::Client),
        )
        .await
        .unwrap();

        assert_eq!(found.id, seeded.id, "the case variant is the same mailbox");
        assert_eq!(found.name, "Original");
        assert_eq!(
            found.role,
            Role::Admin,
            "an existing role must not be lowered"
        );
    }

    /// A timeout is not a lost race. The shared policy owns the
    /// predicate now, but this crate's most contended table is where a
    /// widened retryable set would do the most damage, so the negative
    /// case is pinned from here too.
    #[test]
    fn a_timeout_is_not_treated_as_a_lost_race() {
        let timed_out = surrealdb::Error::query(
            "Query timed out".to_string(),
            Some(surrealdb::types::QueryError::TimedOut {
                duration: std::time::Duration::from_secs(30),
            }),
        );
        assert!(
            !retry::is_retryable(&timed_out),
            "a timeout is not a lost race and must not be re-run",
        );
    }

    /// Why [`classify_write`] reads the message rather than the typed
    /// detail, held against the engine so a future SDK that *does* type
    /// this fails here and the workaround can go.
    ///
    /// `surrealdb_types::ErrorDetails` has an `AlreadyExists` variant, so
    /// "match structurally" looks available — but the engine raises the
    /// unique violation as `surrealdb_core::err::Error::IndexExists`
    /// through `bail!`, and nothing maps that variant onto the public
    /// detail. It arrives as the `Internal` catch-all. What survives is
    /// the message, whose `Database index \`{index}\`` prefix carries a
    /// name this workspace chose in `navigator.surql`.
    #[tokio::test]
    async fn a_unique_violation_carries_no_typed_detail_only_the_index_name() {
        use surrealdb::types::ErrorDetails;

        let db = mem().await;
        person(&db, "Libra", "dup@example.com").await;

        let raw = db
            .query("CREATE $id SET name = 'Other', email = 'dup@example.com', role = 'client'")
            .bind(("id", record_id(super::TABLE, Uuid::now_v7())))
            .await
            .and_then(surrealdb::IndexedResults::check)
            .expect_err("the second write must collide");

        assert!(
            matches!(raw.details(), ErrorDetails::Internal),
            "a typed detail is now available — classify_write should match on it \
             instead of the message; got {:?}",
            raw.details()
        );
        assert!(
            raw.to_string().contains("person_email_lower"),
            "the index name is the only discriminator, and it is gone: {raw}"
        );
    }

    /// The line every write in this module sits on: `UPDATE` never
    /// creates, `UPSERT` does.
    ///
    /// A stale cross-engine `person_id` reaching an update must change
    /// nothing rather than conjure a person, and what guarantees that is
    /// the statement chosen — `Document::update` checks the record exists
    /// before touching anything. Pinned as a pair so the difference is a
    /// tested fact rather than a remembered one: swap one keyword and a
    /// dangling reference silently becomes a row.
    #[tokio::test]
    async fn an_update_never_creates_the_person_upsert_would_have() {
        let db = mem().await;
        let ghost = Uuid::now_v7();

        db.query("UPDATE $id SET name = 'Conjured', email = 'conjured@example.com'")
            .bind(("id", record_id(super::TABLE, ghost)))
            .await
            .unwrap()
            .check()
            .expect("an update against a missing record is not an error");
        assert!(
            find_by_id(&db, ghost).await.unwrap().is_none(),
            "UPDATE must never create the person it was pointed at"
        );

        // The same statement as an UPSERT does create it — which is why
        // no write here is spelled that way.
        db.query("UPSERT $id SET name = 'Conjured', email = 'conjured@example.com'")
            .bind(("id", record_id(super::TABLE, ghost)))
            .await
            .unwrap()
            .check()
            .unwrap();
        assert!(
            find_by_id(&db, ghost).await.unwrap().is_some(),
            "UPSERT is the statement that creates — the distinction this module rests on"
        );
    }

    #[tokio::test]
    async fn a_duplicate_oidc_subject_is_reported_as_that_identity_being_linked() {
        let db = mem().await;
        create(
            &db,
            &NewPerson {
                oidc_subject: Some("sub-1".into()),
                ..NewPerson::new("Libra", "libra@example.com")
            },
        )
        .await
        .unwrap();

        let refused = create(
            &db,
            &NewPerson {
                oidc_subject: Some("sub-1".into()),
                ..NewPerson::new("Aries", "aries@example.com")
            },
        )
        .await;
        assert!(
            matches!(refused, Err(PersonError::OidcSubjectTaken)),
            "{refused:?}"
        );
    }

    /// The index is nullable-unique: seeded people not yet linked to an
    /// IdP must coexist.
    #[tokio::test]
    async fn several_people_may_be_unlinked_at_once() {
        let db = mem().await;
        person(&db, "Libra", "libra@example.com").await;
        person(&db, "Aries", "aries@example.com").await;
        person(&db, "Virgo", "virgo@example.com").await;

        assert_eq!(list_directory(&db, "", "", &[]).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_linked_subject_resolves_and_an_unknown_one_does_not() {
        let db = mem().await;
        let row = person(&db, "Libra", "libra@example.com").await;

        assert!(find_by_oidc_subject(&db, "sub-1").await.unwrap().is_none());
        let linked = link_oidc_subject(&db, row.id, "sub-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(linked.oidc_subject.as_deref(), Some("sub-1"));
        assert_eq!(
            find_by_oidc_subject(&db, "sub-1")
                .await
                .unwrap()
                .map(|p| p.id),
            Some(row.id)
        );
    }

    #[tokio::test]
    async fn linking_a_subject_another_person_holds_is_refused() {
        let db = mem().await;
        let first = person(&db, "Libra", "libra@example.com").await;
        let second = person(&db, "Aries", "aries@example.com").await;
        link_oidc_subject(&db, first.id, "sub-1").await.unwrap();

        let refused = link_oidc_subject(&db, second.id, "sub-1").await;
        assert!(
            matches!(refused, Err(PersonError::OidcSubjectTaken)),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn find_by_ids_returns_only_the_people_who_exist() {
        let db = mem().await;
        let a = person(&db, "Aries", "aries@example.com").await;
        let b = person(&db, "Libra", "libra@example.com").await;
        person(&db, "Virgo", "virgo@example.com").await;

        let found = find_by_ids(&db, &[a.id, Uuid::now_v7(), b.id])
            .await
            .unwrap();
        let mut ids: Vec<Uuid> = found.into_iter().map(|p| p.id).collect();
        ids.sort();
        let mut expected = vec![a.id, b.id];
        expected.sort();
        assert_eq!(ids, expected);

        assert!(find_by_ids(&db, &[]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_default_firm_dri_walks_down_the_authority_ladder() {
        let db = mem().await;
        // Nobody firm-side at all.
        person(&db, "Client", "client@example.com").await;
        assert!(default_firm_dri(&db).await.unwrap().is_none());

        let lawyer = person_at(&db, "Stella", "stella@neonlaw.com", Role::Lawyer).await;
        assert_eq!(default_firm_dri(&db).await.unwrap(), Some(lawyer.id));

        let admin = person_at(&db, "Ada", "ada@neonlaw.com", Role::Admin).await;
        assert_eq!(default_firm_dri(&db).await.unwrap(), Some(admin.id));

        let owner = person_at(&db, "Ozzy", "ozzy@neonlaw.com", Role::Owner).await;
        assert_eq!(default_firm_dri(&db).await.unwrap(), Some(owner.id));

        // A Clerk is not firm-side for this purpose: it is outside the
        // lawyer tier and cannot be a matter's lawyer DRI.
        person_at(&db, "Clio", "clio@neonlaw.com", Role::Clerk).await;
        assert_eq!(default_firm_dri(&db).await.unwrap(), Some(owner.id));
    }

    #[tokio::test]
    async fn the_default_firm_dri_takes_the_lowest_id_within_a_tier() {
        let db = mem().await;
        let first = person_at(&db, "First", "first@neonlaw.com", Role::Lawyer).await;
        let second = person_at(&db, "Second", "second@neonlaw.com", Role::Lawyer).await;
        assert!(first.id < second.id, "v7 ids are ordered by creation");
        assert_eq!(default_firm_dri(&db).await.unwrap(), Some(first.id));
    }

    #[tokio::test]
    async fn the_directory_sorts_by_name_and_filters_case_insensitively() {
        let db = mem().await;
        person(&db, "Sagittarius", "sagittarius@example.com").await;
        person(&db, "Aquarius", "aquarius@neonlaw.com").await;
        person(&db, "Aries", "aries@example.com").await;

        let names = |rows: Vec<super::Person>| -> Vec<String> {
            rows.into_iter().map(|p| p.name).collect()
        };

        assert_eq!(
            names(list_directory(&db, "", "", &[]).await.unwrap()),
            vec!["Aquarius", "Aries", "Sagittarius"]
        );
        assert_eq!(
            names(list_directory(&db, "ARI", "", &[]).await.unwrap()),
            vec!["Aquarius", "Aries", "Sagittarius"]
        );
        assert_eq!(
            names(list_directory(&db, "", "NEONLAW", &[]).await.unwrap()),
            vec!["Aquarius"]
        );
    }

    #[tokio::test]
    async fn the_directory_honours_a_sort_key_and_ignores_an_unknown_one() {
        let db = mem().await;
        person(&db, "Aquarius", "zeta@example.com").await;
        person(&db, "Sagittarius", "alpha@example.com").await;

        let sorted = |rows: Vec<super::Person>| -> Vec<String> {
            rows.into_iter().map(|p| p.email).collect()
        };

        assert_eq!(
            sorted(
                list_directory(&db, "", "", &[("email".into(), false)])
                    .await
                    .unwrap()
            ),
            vec!["alpha@example.com", "zeta@example.com"]
        );
        assert_eq!(
            sorted(
                list_directory(&db, "", "", &[("name".into(), true)])
                    .await
                    .unwrap()
            ),
            vec!["alpha@example.com", "zeta@example.com"]
        );
        // An unadvertised key falls back to the default name order.
        assert_eq!(
            sorted(
                list_directory(&db, "", "", &[("shoe_size".into(), true)])
                    .await
                    .unwrap()
            ),
            vec!["zeta@example.com", "alpha@example.com"]
        );
    }

    #[tokio::test]
    async fn search_matches_substrings_ands_both_needles_and_respects_the_limit() {
        let db = mem().await;
        person(&db, "Aquarius", "aquarius@neonlaw.com").await;
        person(&db, "Aries", "aries@example.com").await;
        person(&db, "Sagittarius", "sagittarius@neonlaw.com").await;

        let names = |rows: Vec<super::Person>| -> Vec<String> {
            rows.into_iter().map(|p| p.name).collect()
        };

        assert_eq!(
            names(search(&db, Some("ARI"), None, 50).await.unwrap()),
            vec!["Aquarius", "Aries", "Sagittarius"]
        );
        assert_eq!(
            names(search(&db, Some("ari"), Some("neonlaw"), 50).await.unwrap()),
            vec!["Aquarius", "Sagittarius"]
        );
        assert_eq!(search(&db, Some("a"), None, 1).await.unwrap().len(), 1);
        assert!(search(&db, Some("ghost"), None, 50)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn an_edit_touches_only_the_fields_it_names() {
        let db = mem().await;
        let row = create(
            &db,
            &NewPerson {
                given_name: Some("Gemma".into()),
                family_name: Some("Twin".into()),
                title: Some("Director".into()),
                ..NewPerson::with_role("Gem", "gem@example.com", Role::Lawyer)
            },
        )
        .await
        .unwrap();

        let renamed = edit(
            &db,
            row.id,
            &PersonEdit {
                name: Some("Gemini".into()),
                ..PersonEdit::default()
            },
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(renamed.name, "Gemini");
        assert_eq!(renamed.email, "gem@example.com");
        assert_eq!(renamed.role, Role::Lawyer, "an unnamed field is preserved");
        assert_eq!(renamed.given_name.as_deref(), Some("Gemma"));
        assert_eq!(renamed.title.as_deref(), Some("Director"));
    }

    #[tokio::test]
    async fn an_edit_clears_a_present_but_empty_name_part() {
        let db = mem().await;
        let row = create(
            &db,
            &NewPerson {
                given_name: Some("Gemma".into()),
                family_name: Some("Twin".into()),
                ..NewPerson::new("Gem", "gem@example.com")
            },
        )
        .await
        .unwrap();

        let cleared = edit(
            &db,
            row.id,
            &PersonEdit {
                given_name: Some(None),
                ..PersonEdit::default()
            },
        )
        .await
        .unwrap()
        .unwrap();

        assert!(cleared.given_name.is_none(), "a present None clears it");
        assert_eq!(
            cleared.family_name.as_deref(),
            Some("Twin"),
            "an omitted part is untouched"
        );
    }

    #[tokio::test]
    async fn an_edit_onto_another_persons_email_is_refused() {
        let db = mem().await;
        person(&db, "Libra", "libra@example.com").await;
        let aries = person(&db, "Aries", "aries@example.com").await;

        let refused = edit(
            &db,
            aries.id,
            &PersonEdit {
                email: Some("LIBRA@example.com".into()),
                ..PersonEdit::default()
            },
        )
        .await;
        assert!(
            matches!(refused, Err(PersonError::EmailTaken)),
            "{refused:?}"
        );
    }

    /// Every write filters by id rather than addressing the record, so a
    /// stale reference updates nothing instead of conjuring a person.
    /// `UPDATE person:<id>` would have created one.
    #[tokio::test]
    async fn a_write_against_a_missing_person_is_a_no_op() {
        let db = mem().await;
        let ghost = Uuid::now_v7();

        assert!(edit(
            &db,
            ghost,
            &PersonEdit {
                name: Some("Ghost".into()),
                ..PersonEdit::default()
            }
        )
        .await
        .unwrap()
        .is_none());
        assert!(set_role(&db, ghost, Role::Owner).await.unwrap().is_none());
        assert!(set_xero_contact_id(&db, ghost, "x")
            .await
            .unwrap()
            .is_none());
        assert!(link_oidc_subject(&db, ghost, "sub")
            .await
            .unwrap()
            .is_none());
        assert!(update_contact(&db, ghost, &ContactUpdate::default())
            .await
            .unwrap()
            .is_none());

        assert!(
            list_directory(&db, "", "", &[]).await.unwrap().is_empty(),
            "a no-op write must not have created a row"
        );
    }

    #[tokio::test]
    async fn setting_a_role_moves_the_person_up_the_ladder() {
        let db = mem().await;
        let row = person(&db, "Stella", "stella@neonlaw.com").await;
        assert_eq!(row.role, Role::Client);

        let promoted = set_role(&db, row.id, Role::Lawyer).await.unwrap().unwrap();
        assert_eq!(promoted.role, Role::Lawyer);
        assert_eq!(
            find_by_id(&db, row.id).await.unwrap().unwrap().role,
            Role::Lawyer
        );
    }

    #[tokio::test]
    async fn set_xero_contact_id_caches_then_is_idempotent() {
        let db = mem().await;
        let row = person(&db, "Capricorn", "capricorn@example.com").await;
        assert!(row.xero_contact_id.is_none());

        let updated = set_xero_contact_id(&db, row.id, "xero-contact-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.xero_contact_id.as_deref(), Some("xero-contact-1"));

        // Re-set the same id — still one value, no error.
        let again = set_xero_contact_id(&db, row.id, "xero-contact-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(again.xero_contact_id.as_deref(), Some("xero-contact-1"));
    }

    #[tokio::test]
    async fn update_contact_replaces_the_contact_facts_and_nothing_else() {
        let db = mem().await;
        let row = person_at(&db, "Libra", "libra@example.com", Role::Lawyer).await;

        let updated = update_contact(
            &db,
            row.id,
            &ContactUpdate {
                name: "Libra Scales".into(),
                title: Some("Executive Director".into()),
                phone: Some("+1-555-0100".into()),
            },
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(updated.name, "Libra Scales");
        assert_eq!(updated.title.as_deref(), Some("Executive Director"));
        assert_eq!(updated.phone.as_deref(), Some("+1-555-0100"));
        assert_eq!(
            updated.email, "libra@example.com",
            "an import cannot move the mailbox"
        );
        assert_eq!(
            updated.role,
            Role::Lawyer,
            "an import cannot change authority"
        );
    }

    #[tokio::test]
    async fn deleting_removes_the_person_and_is_idempotent() {
        let db = mem().await;
        let row = person(&db, "Cleo", "cleo@example.com").await;

        delete(&db, row.id).await.unwrap();
        assert!(find_by_id(&db, row.id).await.unwrap().is_none());
        assert!(find_by_email_ci(&db, "cleo@example.com")
            .await
            .unwrap()
            .is_none());

        // Deleting again, and deleting one that never existed, are no-ops.
        delete(&db, row.id).await.unwrap();
        delete(&db, Uuid::now_v7()).await.unwrap();
    }

    /// A deleted mailbox is free again — the unique index must not keep
    /// it reserved after the row is gone.
    #[tokio::test]
    async fn a_deleted_email_can_be_reused() {
        let db = mem().await;
        let first = person(&db, "Cleo", "cleo@example.com").await;
        delete(&db, first.id).await.unwrap();

        let second = person(&db, "Cleo Again", "cleo@example.com").await;
        assert_ne!(second.id, first.id);
    }
}
