---
name: keycloak-oidc
description: >
  Keycloak as the local OIDC identity provider for `web` — realm/client config, Authorization Code + PKCE flow, JWKS
  verification, id_token decoding, and the persons-table linking story. Trigger when editing
  `k8s/overlays/kind/deps/keycloak.yaml`, changing the OIDC client config, debugging the `/auth/callback` handler,
  touching the `oauth2` or `jsonwebtoken` crates, or planning a swap to a different OIDC provider (Google Cloud
  Identity, Auth0). Identity providers are pluggable — code paths must stay spec-compliant.
---

# Keycloak (and OIDC) in the navigator workspace

Keycloak is the **local** IdP; production targets Google Cloud Identity. The contract between them is the OIDC discovery
doc — anything spec-compliant works, and swapping IdPs is an env-var change, never a code change.

**Everything factual lives in the doc** — read [`docs/oidc.md`](../../../docs/oidc.md) and keep it, not this skill,
authoritative: the realm/client fixture and env vars, the full Authorization Code + PKCE login sequence, the
frontchannel/backchannel split, how the role enters the session, the crates, troubleshooting, and the Google swap.

## How to treat it (the load-bearing rules)

- **Identity is `sub` + `email`, nothing more.** Keycloak hands us a stable subject and an email; the `persons` table
  owns name, role, project membership, and billing. Never read profile/role from id_token claims.
- **The session role comes from the DB, not the token.** At `/auth/callback` we upsert the `persons` row and stamp
  `session.role = row.role` — a token-side role is silently ignored. Authz is then [[opa-policy]] against that session.
- **Fetch every endpoint via the discovery doc.** Never hardcode `authorization_endpoint` / `token_endpoint` /
  `jwks_uri` — discovery is the seam that makes the Google ↔ Keycloak swap a one-line env change.
- **Verify the id_token fully.** Signature against JWKS plus `iss` / `aud` / `exp` / `nbf`. RS256 in prod; HS256 only in
  tests. Then link to a person and ride a signed session cookie — don't store the access_token.

## Anti-patterns

- Reading user attributes (name, address, org) from id_token claims — identity is `sub` + `email`, the rest is in
  `persons`.
- Using Keycloak realm roles for authorization — authz is [[opa-policy]] against the DB-sourced session role.
- Hardcoding issuer/token/JWKS URLs instead of fetching the discovery doc.
- Storing the access_token — we extract identity from the id_token and use a signed session cookie thereafter.

## Boundaries

- The role + participation model and "who can see what": [[authorization-model]] and `docs/access-model.md`.
- The authorization decision point (Rego, `require_policy`): [[opa-policy]].
- Bringing the local Keycloak up in KIND: [[kind-local-dev]].
