# OIDC + DB-role authorization

Neon Law Navigator separates **identity** (who you are) from **authorization** (what you can do). The OIDC provider
(Keycloak today, Google tomorrow) owns identity only — a stable `sub` and an `email`. The `persons` table in our
database owns everything else: profile, project memberships, billing relationships, and the **single role** (`client` /
`staff` / `admin`) that gates the back-office. OPA evaluates the rego policy against that DB-sourced role, never against
the IdP token. See [`docs/access-model.md`](access-model.md) for the role/participation split.

This document is the canonical narrative for the system. The Rust modules link back to it from their rustdoc:

- [`web::oauth`](../web/src/oauth.rs) — `/auth/login`, `/auth/callback`, `upsert_person_from_claims`.
  [`web::session`](../web/src/session.rs) — signed cookie shape (`SessionData`). [`web::policy`](../web/src/policy.rs) —
  `PolicyClient` and `require_policy` middleware. [`store::entity::person`](../store/src/entity/person.rs) — the
  `persons` row, including the `role` enum column. Schema migrations:
  [`m20260527_add_oidc_subject_to_persons`](../store/src/migration/m20260527_add_oidc_subject_to_persons.rs),
  [`m20260528_add_roles_to_persons`](../store/src/migration/m20260528_add_roles_to_persons.rs) (legacy `roles[]`), and
  [`m20260619_collapse_persons_roles_to_role`](../store/src/migration/m20260619_collapse_persons_roles_to_role.rs)
  (collapsed to a single `role` column).

## Login sequence

The full Authorization Code + PKCE flow, end to end, with the upsert step that links the IdP to a local `persons` row
and the OPA decision that gates the admin route.

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant Browser
    participant Web as navigator-web
    participant IdP as Keycloak / Google
    participant DB as Postgres
    participant OPA as OPA sidecar

    User->>Browser: click "Sign in"
    Browser->>Web: GET /auth/login?return_to=/portal
    Web->>Web: generate PKCE verifier + state
    Web-->>Browser: 302 Location: <IdP>/authorize?...&code_challenge=...
    Note over Web,Browser: Set-Cookie: navigator_pre_auth=...<br/>(HMAC-signed, HttpOnly, SameSite=Lax)
    Browser->>IdP: GET /authorize?...
    IdP-->>Browser: login page
    User->>IdP: credentials
    IdP-->>Browser: 302 Location: /auth/callback?code=...&state=...
    Browser->>Web: GET /auth/callback?code=...&state=...
    Web->>Web: verify pre-auth cookie + state
    Web->>IdP: POST /token (grant_type=authorization_code, code_verifier=...)
    IdP-->>Web: { id_token: { sub, email, name } }
    Note over Web: token carries identity only —<br/>no role, no profile

    Web->>DB: SELECT * FROM persons WHERE oidc_subject = sub
    alt subject already linked
        DB-->>Web: existing row
    else not linked
        Web->>DB: SELECT * FROM persons WHERE email = ?
        alt email matches a seeded row
            Web->>DB: UPDATE persons SET oidc_subject = sub WHERE id = ?
            DB-->>Web: row promoted, keeps prior role
        else no match
            Web->>DB: INSERT INTO persons (sub, email, name, role='client')
            DB-->>Web: new row, role=client
        end
    end

    Web->>Web: session.role = row.role  (NOT token.role)
    Web-->>Browser: 302 Location: /portal
    Note over Web,Browser: Set-Cookie: navigator_session=<HMAC>(<br/>  sub, email, person_id, role, exp, csrf_token<br/>)<br/>+ navigator_pre_auth cleared

    Browser->>Web: GET /portal
    Web->>Web: decode signed session cookie
    Web->>OPA: POST /v1/data/navigator/authz/allow<br/>{ path, method, session }
    OPA-->>Web: { result: true|false }
    alt allow
        Web-->>Browser: 200 admin page
    else deny
        Web-->>Browser: 403 Forbidden
    end
```

## Identity vs authorization split

```mermaid
flowchart LR
    subgraph IdP[OIDC Provider]
        sub[sub<br/>kc-uuid-staff]
        email[email<br/>staff@neonlaw.com]
        name[name<br/>Staff]
    end
    subgraph DB[persons row]
        oidc_subject[oidc_subject<br/>kc-uuid-staff]
        local_email[email<br/>staff@neonlaw.com]
        local_name[name<br/>Staff]
        role["role<br/>staff"]
        profile[other profile<br/>columns...]
    end
    subgraph Session[signed session cookie]
        s_sub[sub]
        s_email[email]
        s_person_id[person_id]
        s_role[role &lt;-- from DB]
    end
    sub -->|id_token claim| oidc_subject
    email -->|id_token claim| local_email
    name -->|id_token claim| local_name
    oidc_subject --> s_sub
    local_email --> s_email
    role --> s_role
    profile -.->|never leaves the DB| profile
```

Two consequences fall out of this split:

1. **Granting/revoking access is one SQL statement**: `UPDATE persons SET role = 'staff' WHERE id = ?`. No IdP
   configuration change, no realm export, no new federated mapper.
2. **Replacing the IdP is an env-var swap**. The `sub` shape changes (Keycloak UUID → Google numeric string), but every
   column accepting `sub` is just `String`. See [`README.md → Swap to Google's OIDC`](../README.md). Production already
   runs this swap — `examples/deploy/k8s/gke/patches/web-env.yaml` sets `OAUTH_ISSUER_URL=https://accounts.google.com`.
   Keycloak is KIND-only and never reaches GKE.

### KIND-only: the frontchannel / backchannel split

Local Keycloak is dual-homed: Chrome hits `http://localhost:8080/keycloak/...` (KIND host port-map → nginx ingress →
Keycloak Service) and the navigator-web pod hits `http://keycloak:8080/keycloak/...` (cluster DNS, direct). One URL is
browser-reachable, the other is pod-reachable; they're not interchangeable. Keycloak v25's hostname-v2 keeps the
discovery doc honest: `KC_HOSTNAME=http://localhost:8080/keycloak` sets the frontchannel `authorization_endpoint`, and
`KC_HOSTNAME_BACKCHANNEL_DYNAMIC=true` lets `token_endpoint` and friends follow whatever URL the pod used. The OIDC
client in `web/src/oauth.rs` stays provider-agnostic — no rewrite layer needed. Production uses Google Identity Services
and never sees any of this.

## How the role enters the session

```mermaid
stateDiagram-v2
    [*] --> AwaitingLogin
    AwaitingLogin --> Authorizing: GET /auth/login
    Authorizing --> Callback: IdP redirect with code
    Callback --> TokenExchange: POST /token
    TokenExchange --> ClaimsDecoded: id_token parsed (sub, email, name)
    ClaimsDecoded --> UpsertPerson: find_or_create persons row
    UpsertPerson --> RoleLoaded: row.role read back
    RoleLoaded --> SessionWritten: session.role = row.role
    SessionWritten --> AdminRequest: subsequent GET /portal/admin/*
    AdminRequest --> PolicyEval: POST OPA /v1/data/navigator/authz/allow
    PolicyEval --> Allow: result == true
    PolicyEval --> Deny: result == false
    Allow --> [*]: handler renders
    Deny --> [*]: 403 Forbidden
```

Critically, the arrow into `SessionWritten` reads from the `persons` row, not from the id_token. A token-side role, if
present, is silently ignored — the `IdTokenClaims` struct in `web::oauth` doesn't even include a `role` field.

## Rego policy

The default policy that ships in `k8s/opa/opa.yaml`:

```rego
package navigator.authz

default allow := false

staff_tier := {"staff", "admin"}

# /portal/admin requires the DB-stamped staff (or admin) role.
allow if {
    input.path[0] == "portal"
    input.path[1] == "admin"
    input.session
    staff_tier[input.session.role]
}

# Authenticated read API.
allow if {
    input.path[0] == "api"
    input.method == "GET"
    input.session
}

# Public surface.
allow if {
    input.path[0] == "openapi.json"
}
```

`input.session.role` is whatever `persons.role` was at callback time. A user demoted to `client` in the database is
denied at their next login — no IdP coordination required.

## Verified end-to-end

`web/tests/oidc_e2e.rs` exercises the entire pipeline against a mocked Keycloak and a mocked OPA. Six tests:

1. `full_oidc_flow_upserts_person_and_passes_opa_allow` — happy path; person row created with email + name from the
   id_token.
2. `opa_deny_blocks_admin_route_with_403` — OPA returning `false` results in 403 from the admin route.
3. `second_login_with_same_subject_does_not_create_duplicate_person` — re-running the login doesn't insert a second row.
4. `user_with_db_staff_role_can_hit_every_admin_route` — pre-seeds `role = staff` in the DB, logs in (promoting the
   row), hits eight portal routes (`/portal`, `/portal/admin/people`, `/portal/admin/entities`,
   `/portal/admin/jurisdictions`, `/portal/admin/entity-types`, `/portal/admin/templates`, `/portal/admin/questions`,
   `/portal/projects`) under an OPA mock that only allows when `input.session.role == "staff"`.
5. `user_with_empty_db_roles_is_denied_even_when_token_would_have_granted` — fresh user, default `role = client`; every
   `/portal/admin/*` route returns 403.
6. `db_role_revocation_takes_effect_on_next_login` — a staff user starts with staff, succeeds; row is updated to `role =
   'client'`; next login produces a session that fails the OPA check.

Run them with:

```bash
cargo test -p web --test oidc_e2e
```
