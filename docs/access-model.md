# Access model — role + participation

Navigator separates **what a person is** (system-wide tier) from **what a person sees** (per-project scope). Both
answers live in the database, both flow into OPA, neither lives in the IdP token. The IdP supplies only identity (`sub`,
`email`).

> **Role decides the tier; participation decides the scope.**

The two columns:

| Column | Table | Decides |
| --- | --- | --- |
| `role` | `persons` | The tier: `client`, `staff`, or `admin`. Anonymous = no row. |
| `participation` | `person_project_roles` | The matter-side role on a Project (`attorney`, `paralegal`, `client`). |

The two columns are independent. A paralegal who is *also* a client of the firm for their own LLC carries the staff role
on the persons row (their day job) and a `person_project_roles` row on their personal matter with the client
participation. The system answers "what can this person do" by reading both.

## The three tiers

`persons.role` is a `text` column with `CHECK (role IN ('client','staff','admin'))`. SeaORM models it as an
`ActiveEnum`.

### `client`

A person the firm represents on at least one matter. Sees only projects with a matching `person_project_roles` row.

### `staff`

A firm employee — attorney, paralegal, support. Same per-project scoping as `client`. The tier difference shows up in
*what they can do* on a visible project (edit, sign, file), not in *what's visible*.

### `admin`

A firm employee with system-administration authority — manage the persons table, rotate keys, archive projects. Bypasses
project-scoping entirely. Sees every project, silently, without writing an audit row.

### *anonymous*

No row in `persons` at all. Sees only the public marketing surface (homepage, `/foundation/*`, `/openapi.json`,
`/auth/login`).

`admin` is a superset of `staff`, not a separate axis. A firm administrator is, by definition, someone who could be
assigned to any matter; making them ask for participation rows on every project they need to touch buys nothing.

## Participation

`person_project_roles.participation` is a free-form `text` column. The values currently in use (from
`store/seeds/PersonProjectRole.yaml` and live writes):

- `attorney` — lead attorney on the matter.
- `paralegal` — supporting paralegal.
- `client` — the natural-person client.
- `co_counsel` — outside counsel collaborating on the matter.

The matter-side vocabulary is open: new participation kinds (`translator`, `guardian_ad_litem`) arrive as the firm takes
on new kinds of work without needing a migration.

Every row carries `inserted_at` + `updated_at` (the workspace timestamp convention). Those answer "is this still true
right now and how stale is the fact." They do **not** answer "was Libra ever an attorney on this matter." If you need
participation history, append a row to `relationship_logs` — that's what the table exists for
([`m20260526_create_provenance_tables.rs`](../store/src/migration/m20260526_create_provenance_tables.rs)).

## What `participation` is NOT

It is not the `disclosures` table. Disclosures are formal records the firm keeps about *conflicts of interest* and
*related-party relationships* — information flowing *from* the client *to* the firm about who the client is connected
to. Project membership is the opposite direction: an internal record of *who the firm has put on the matter*. The two
concepts share the same English word in casual speech ("Libra is disclosed on the Acme matter") but they're different
columns in different tables answering different questions.

If you find yourself reaching for `disclosures` to decide whether someone can see a project, stop — you want
`person_project_roles`. See [glossary entry "Disclosure"](glossary.md#disclosure).

## How OPA decides

The web middleware (`web::policy::require_policy`) posts an `input` document to OPA on every request:

```json
{
  "path":       ["admin", "projects", "9a..."],
  "method":     "GET",
  "session":    {
    "sub":   "<idp subject>",
    "email": "libra@example.com",
    "role":  "staff"
  },
  "project_id": "9a..."
}
```

`project_id` is populated by the route handler when the URL is project-scoped (`/portal/projects/:id`,
`/portal/projects/:id/documents/...`). Routes without a project parameter leave it absent.

OPA's allow rules in priority order:

1. **Admin bypass** — `session.role == "admin"` allows every authenticated request. No project-membership check, no
   per-read audit. The trust call is that admin already implies a fiduciary duty audited elsewhere (Drive activity, DB
   write logs).
2. **Staff-tier writes** — `/portal/admin/persons`, `/portal/admin/templates`, and other firm-internal CRUD gate on
   `session.role` being either `"staff"` or `"admin"`.
3. **Project-scoped reads** — `/portal/projects/:id/...` and `/api/projects/:id/...` allow if there is a
   `person_project_roles` row with `person_id = session.person_id` and `project_id = input.project_id`. The
   participation value isn't checked at OPA — it's enough that the row exists. Action-level distinctions (a client can
   *read* the engagement letter, only staff can *edit* it) live in the route layer, not the visibility gate.

The web side ships a single helper for the visibility query:

```rust
// web/src/access.rs
pub async fn visible_projects(db: &Db, person_id: Uuid, role: Role) -> Result<Vec<Project>, DbErr>;
```

Every project-list and project-detail handler funnels through this helper. Inlining the SQL into individual handlers is
the failure mode we are explicitly avoiding — it's how authz quietly drifts.

## Related

- [`docs/oidc.md`](oidc.md) — Authorization Code + PKCE login flow and how the persons row is upserted.
- [`docs/glossary.md`](glossary.md) — Person, Project, Disclosure, Participation.
- [`k8s/base/opa/opa.yaml`](../k8s/base/opa/opa.yaml) — the live Rego policy.
- [`web::policy`](../web/src/policy.rs) — the `require_policy` middleware that posts to OPA.
