Feature: OIDC callback persons linking

  On every `/auth/callback`, the server decodes `sub` + `email` +
  `name` from the id_token and links a `persons` row to the IdP via
  the `oidc_subject` column. Sign-up is **operator-mediated**: an
  identity the firm hasn't seeded is rejected, never JIT-created.
  Three paths:

    1. No seeded row → `403`, no row created (sign-up is
       operator-mediated; `resolve_person_from_claims` returns
       `NotPreSeeded`).
    2. Seeded email with `oidc_subject IS NULL` → PROMOTE (link the
       existing row, preserve the seeded role).
    3. Returning sub → no-op (idempotent).

  The id_token is RS256-signed with the shared test keypair and the
  app carries the matching verifier, so every scenario passes through
  the production signature + `iss`/`aud`/`nonce` checks. wiremock
  stands in for Keycloak.

  Background:
    Given a mock IdP returning an id_token

  Scenario: An unseeded identity is rejected — sign-up is operator-mediated
    Given the IdP issues sub="kc-uuid-libra", email="libra@example.com", name="Libra"
    When Libra completes the OAuth login dance
    Then the callback is rejected with 403
    And exactly 0 persons rows exist

  Scenario: A seeded email is promoted on first login, preserving the staff role
    Given a seeded person with email "staff@neonlaw.com" and role "staff"
    And the IdP issues sub="kc-uuid-staff", email="staff@neonlaw.com", name="Staff"
    When Staff completes the OAuth login dance
    Then the callback redirects with 303
    And exactly 1 persons row exists
    And the persons row has oidc_subject "kc-uuid-staff"
    And the persons row has email "staff@neonlaw.com"
    And the persons row keeps the "staff" role

  Scenario: A returning subject does not create a duplicate row
    Given a seeded person with email "cancer@example.com" and role "client"
    And the IdP issues sub="kc-uuid-cancer", email="cancer@example.com", name="Cancer"
    When Cancer completes the OAuth login dance
    And Cancer completes the OAuth login dance again
    Then exactly 1 persons row exists
