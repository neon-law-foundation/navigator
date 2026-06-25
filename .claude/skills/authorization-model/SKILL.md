---
name: authorization-model
description: >
  Neon Law Navigator's role + participation authorization model — the **canonical answer** to "who can see what." Every person
  carries exactly one role in `persons.role` (`client`, `staff`, or `admin`); per-project scope lives separately in
  `person_project_roles.participation`. Admin bypasses project-scoping silently. Trigger when the user mentions any of
  `role`, `roles`, `staff`, `client`, `admin`, `participation`, OPA, "who can see", "what does Libra/Nick see", or
  before adding a new authz check anywhere in `web`. Also trigger when reaching for a JSON-array `roles[…]` shape — the
  schema collapsed to a single `role` column in migration `m20260619_collapse_persons_roles_to_role`, and the doc/PR
  drift to fix is to use the singular column.
---

# Authorization model — role + participation

The one-liner that captures the whole thing:

> **Role decides the tier; participation decides the scope.**

Neon Law Navigator separates *what a person is* (system-wide tier) from *what they see* (per-project scope). Both
columns live in the database, both flow into OPA, neither lives in the IdP token.

## One role per person

- **`client`** — a person the firm represents on at least one matter. Sees only projects with a matching
  `person_project_roles` row.
- **`staff`** — a firm employee (attorney, paralegal, support). Same per-project scope as `client`; the tier
  difference shows up in *actions* (edit, sign, file), not visibility.
- **`admin`** — a firm employee with system-administration authority. Bypasses project-scoping; sees every project,
  silently (no audit row per read).

A person has **exactly one** row in `persons` and therefore **exactly one** role. `admin` is not "staff + a flag" — it's
a separate value of the same enum. Flipping a row from `staff` to `admin` is one SQL `UPDATE`.

Visibility-wise, `admin` is a superset of `staff` (every URL `staff` can hit, `admin` can too). But at the row level,
two people on the same tier may be assigned to different projects via `person_project_roles` — staff and admin aren't
*set-equivalent* to each other through participation; admin's superset comes from the OPA-level bypass, not from rows.

## Per-project participation

`person_project_roles.participation` is a free-form `TEXT` column. Today's values: `attorney`, `paralegal`, `client`,
`co_counsel`. New matter-side participations (`translator`, `guardian_ad_litem`) arrive without a migration. OPA does
**not** read `participation` — the existence of the row is the signal; the value is descriptive.

## Concrete people in the seed data

- **Nick** (`nick@neonlaw.com`, lowercase) — the firm administrator. Role: `admin`. Sees every project. The lowercase
  spelling is exact: `store::seed::require_firm_domain` rejects mixed-case staff/admin seeds at load time.
- **Staff** — firm employees. Role: `staff`. Convention: lowercase `*@neonlaw.com` emails for any seeded staff record.
  **Staff** (`staff@neonlaw.com`) — KIND-only Keycloak fixture for the OIDC walk-through. Role: `staff` (per
  `docs/RUNBOOK.md` step 3). It is **staff**, exactly one role. Not admin.
- **Clients** — any seeded non-firm person. Role: `client`. Email is the client's real address; no domain restriction.

## Where it's enforced

Two enforcement points; both must agree:

1. **OPA (sidecar)** — `k8s/base/opa/opa.yaml` decides whether the URL is reachable at all. `/portal/admin/*` requires
   `staff_tier` (`staff` or `admin`); `/portal/projects/*` allows any authenticated person; the handler then…
2. **`web::access::visible_projects` / `can_see_project`** — row-scopes the response. `admin` bypasses; `client` and
   `staff` see only their participation rows.

`web/src/admin.rs::is_staff_tier` is the in-handler gate for the project write surface — clients get `404` (not `403`)
on `/portal/projects/:id/edit` and friends. The 404 is intentional: the management surface "doesn't exist" for them.

## Drift to watch for

- Anything saying `roles = '["staff"]'` or `session.roles contains "staff"` is referring to the **legacy** array-column
  shape from migration `m20260528_add_roles_to_persons`. The current schema collapsed it to a single `role` column in
  `m20260619_collapse_persons_roles_to_role` — fix the prose to use `role = 'staff'` and `session.role == "staff"`.
- Don't conflate `persons.role` with `person_project_roles.participation`. Same English word ("role"), different
  columns, different decisions.

## Canonical references (read these, don't paraphrase)

- [`docs/access-model.md`](../../../docs/access-model.md) — full narrative + Rego rules.
  [`store/src/entity/person.rs`](../../../store/src/entity/person.rs) — the `Role` enum.
  [`web/src/access.rs`](../../../web/src/access.rs) — `visible_projects` + `can_see_project`.
  [`k8s/base/opa/opa.yaml`](../../../k8s/base/opa/opa.yaml) — the live Rego policy.
  [`docs/oidc.md`](../../../docs/oidc.md) — how the role enters the session at callback time.
