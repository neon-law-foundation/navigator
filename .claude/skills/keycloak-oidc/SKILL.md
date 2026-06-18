---
name: keycloak-oidc
description: >
  Keycloak as the local OIDC identity provider for `web` — realm/client config, Authorization Code + PKCE flow, JWKS
  verification, id_token decoding, and the persons-table linking story. Trigger when editing
  `k8s/keycloak/keycloak.yaml`, changing the OIDC client config, debugging the `/auth/callback` handler, touching the
  `oauth2` or `jsonwebtoken` crates, or planning a swap to a different OIDC provider (Google Cloud Identity, Auth0).
  Identity providers are pluggable — code paths must stay spec-compliant.
---

# Keycloak (and OIDC) in the navigator workspace

Keycloak is the **local** IdP. The production target is Google Cloud Identity / Google Identity Services. The contract between the two is the OIDC discovery doc — anything spec-compliant works, and swapping IdPs is an env-var change.

## What lives where

| Concern | Source of truth |
|---|---|
| **Identity** (`sub`, `email`) | The IdP. Keycloak in dev, Google in prod. |
| **Profile** (name, roles, project membership, billing) | `persons` table in our database, linked to the IdP via `oidc_subject`. |

This split is deliberate. Keycloak stays a minimal install: no realm-level role management, no custom user attributes, no per-product policy. It hands us identity; we own the profile. Swap targets must give us `sub` + `email` and nothing more.

## Local realm

`k8s/keycloak/keycloak.yaml` auto-imports a realm on startup:

- **Realm:** `navigator`
- **Client:** `navigator-web` (confidential, Authorization Code + PKCE)
- **User:** `staff` / password `staff`, with role `staff`

That's the entire dev fixture — enough to exercise every code path end-to-end. Admin console is on host port `30080` (see [[kind-local-dev]]).

## Environment variables

```
OAUTH_ISSUER_URL=http://keycloak.navigator.svc.cluster.local:8080/keycloak/realms/navigator
OAUTH_CLIENT_ID=navigator-web
OAUTH_CLIENT_SECRET=<from secret>
OAUTH_REDIRECT_URI=http://localhost:3001/auth/callback   # host-runs-web mode
SESSION_SECRET=<32+ bytes, HMAC>
```

For the host-runs-web flow, `OAUTH_ISSUER_URL` points at `keycloak.navigator.svc.cluster.local` *only when the web binary is also in-cluster*. When `cargo run -p web` runs on the host, the issuer URL is `http://localhost:30080/keycloak/realms/navigator` and the redirect URI is `http://localhost:3001/auth/callback`. The `/keycloak` prefix comes from `KC_HTTP_RELATIVE_PATH` in `k8s/overlays/kind/deps/keycloak.yaml`; Keycloak's hostname-v2 split routes the browser to `localhost:8080/keycloak/...` (ingress) and the pod to `keycloak:8080/keycloak/...` (cluster DNS). The `.devx/env` file sets this correctly — don't hand-roll it.

## The login flow

1. `GET /auth/login` → server builds an Authorization Code + PKCE URL using the issuer's `authorization_endpoint`, stores the PKCE verifier in a signed cookie, redirects.
2. User authenticates at Keycloak, gets redirected to `/auth/callback?code=…&state=…`.
3. `GET /auth/callback`:
   - Validates `state` against the cookie.
   - Exchanges `code` for an id_token at `token_endpoint` (with the PKCE verifier).
   - Verifies id_token signature against JWKS (`jwks_uri`), checks `iss`, `aud`, `exp`, `nbf`.
   - Decodes `sub`, `email`, optional `name`, optional `roles`.
4. **Persons linking** (see `web/src/auth/`):
   - `persons.oidc_subject = sub` → use it.
   - else `persons.email = email AND oidc_subject IS NULL` → promote it (seeded persons become real users on first login).
   - else insert a new `persons` row with `oidc_subject`, `email`, `name`.
5. Stamp `person_id` into the signed session cookie; every subsequent request has a local DB identity for free.

## Crates

- `oauth2` for the Authorization Code + PKCE state machine.
- `jsonwebtoken` for id_token verification against JWKS. RS256 in prod; we accept HS256 in tests only.
- `reqwest` for fetching the discovery doc and JWKS — bounded retry at startup (see [[rust-service-lifecycle]] init step 5).

## Swap to Google's OIDC

Google Cloud Identity / Google Identity Services both expose <https://accounts.google.com/.well-known/openid-configuration>. Create a Web-application OAuth 2.0 Client ID in the Google Cloud Console and set:

```
OAUTH_ISSUER_URL=https://accounts.google.com
OAUTH_CLIENT_ID=<from console>
OAUTH_CLIENT_SECRET=<from console>
OAUTH_REDIRECT_URI=https://your.domain/auth/callback
```

Same flow, same id_token decoding, same persons upsert. Google's `sub` is an opaque numeric string (`117483746…`); Keycloak's is a UUID. The column is `String`; either works.

## Common debug steps

- **`/auth/callback` returns 400 "invalid state".** Cookie path / SameSite mismatch. The cookie set in `/auth/login` must be readable from `/auth/callback`. In dev over HTTP, `SameSite=Lax` + `Secure=false` is required.
- **JWKS fetch fails with TLS error.** `OAUTH_ISSUER_URL` is `https` but the in-cluster Keycloak is plain HTTP — set it to `http://…` for the in-cluster case; the spec allows it.
- **Token exchange returns "invalid_client".** `OAUTH_CLIENT_SECRET` doesn't match the realm's client. Re-import the realm fixture or rotate the secret in the Keycloak admin console.
- **id_token verifies but `roles` is empty.** Roles aren't in id_tokens by default in Keycloak — add a "User Realm Role" mapper to the `navigator-web` client and choose "Add to ID token". Or skip realm roles entirely and source roles from our `persons` table (the recommended pattern).

## Anti-patterns

- Reading user attributes (name, address, organization) from id_token claims. Identity is `sub` + `email`; everything else is in `persons`.
- Using Keycloak realm roles for authorization decisions. We use [[opa-policy]] against session metadata, not the IdP's role catalog.
- Hardcoding the issuer or token endpoint URLs. Always fetch via the discovery doc — it's the seam that makes Google ↔ Keycloak swap a one-line change.
- Storing the access_token. We don't need it; we extract identity from the id_token, link to a person, and use a signed session cookie thereafter.

## Canonical sources

- OIDC Core 1.0 (the spec): <https://openid.net/specs/openid-connect-core-1_0.html>
- OIDC Discovery 1.0: <https://openid.net/specs/openid-connect-discovery-1_0.html>
- OAuth 2.0 + PKCE (RFC 7636): <https://datatracker.ietf.org/doc/html/rfc7636>
- Keycloak documentation: <https://www.keycloak.org/documentation>
- Keycloak repository: <https://github.com/keycloak/keycloak>
- Keycloak on Quay (image): <https://quay.io/repository/keycloak/keycloak>
- Google Identity (OIDC): <https://developers.google.com/identity/openid-connect/openid-connect>
- `oauth2` crate: <https://docs.rs/oauth2> · <https://github.com/ramosbugs/oauth2-rs>
- `jsonwebtoken` crate: <https://docs.rs/jsonwebtoken>
