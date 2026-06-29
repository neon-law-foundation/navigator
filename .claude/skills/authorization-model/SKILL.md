---
name: authorization-model
description: >
  Neon Law Navigator's role + participation authorization model — the **canonical answer** to "who can see what." Every
  person carries exactly one role in `persons.role` (`client`, `staff`, or `admin`); per-project scope lives separately
  in `person_project_roles.participation`. Admin bypasses project-scoping silently. Trigger when the user mentions any
  of `role`, `roles`, `staff`, `client`, `admin`, `participation`, OPA, "who can see", "what does Libra/Nick see", or
  before adding a new authz check anywhere in `web`. Also trigger when reaching for a JSON-array `roles[…]` shape — the
  schema collapsed to a single `role` column in migration `m20260619_collapse_persons_roles_to_role`, and the doc/PR
  drift to fix is to use the singular column.
---

# Authorization model — role + participation

The one-liner that captures the whole thing:

> **Role decides the tier; participation decides the scope.**

*What a person is* (system-wide tier) is separate from *what they see* (per-project scope). Both columns live in the DB,
both flow into OPA, neither lives in the IdP token. **Everything factual lives in the doc** — read
[`docs/access-model.md`](../../../docs/access-model.md) and keep it, not this skill, authoritative: the three tiers and
anonymous, the participation vocabulary, the seeded people, and the OPA allow rules.

## How to treat it (the load-bearing rules)

- **One singular `role` column, never a `roles` array.** A person has exactly one row in `persons` and one
  `role` (`client`/`staff`/`admin`). Anything saying `roles = '["staff"]'` or `session.roles contains "staff"` is the
  legacy array shape from `m20260528_add_roles_to_persons`, collapsed in `m20260619_collapse_persons_roles_to_role` —
  fix the prose to `role = 'staff'` and `session.role == "staff"`.
- **`staff` INCLUDES attorneys.** Attorney, paralegal, and support are all the `staff` tier — there is no separate
  "attorney" role. The tier difference vs `client` shows up in *actions* (edit, sign, file), not visibility.
- **Admin bypasses project-scoping silently.** `session.role == "admin"` allows every authenticated request with no
  per-read audit row. `admin` is a separate enum value, not "staff + a flag"; visibility-wise it is a superset of staff.
- **`participation` is descriptive; OPA does not read its value.** `attorney`/`paralegal`/`co_counsel`/`client` are
  per-project participation values — the *existence* of the `person_project_roles` row is the signal, not the string.
  New kinds (`translator`, `guardian_ad_litem`) arrive without a migration.
- **Don't conflate `persons.role` with `person_project_roles.participation`** — same English word, different columns,
  different decisions. And `participation` is not the `disclosures` table (conflicts of interest), which flows the other
  direction.

## Boundaries

- The OPA decision point (Rego, sidecar, `require_policy` middleware): [[opa-policy]] and `docs/opa-policy.md`.
- How the `role` enters the session at login: [[keycloak-oidc]] and `docs/oidc.md`.
