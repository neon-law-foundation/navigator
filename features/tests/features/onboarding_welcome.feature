Feature: Welcome email on operator-mediated signup

  Sign-up is operator-mediated: an unseeded identity is rejected
  (403), never JIT-created, so there is no self-service "brand-new
  signup" welcome. The one identity the callback may create on first
  login is the operator's configured admin email — the bootstrap
  carve-out so a fresh deployment is never locked out — created as an
  ordinary `admin` (there is no separate "super" tier). That first
  login fires the welcome once; promotion of an operator-seeded email
  is not a fresh signup and sends nothing.

  The synchronous send is a stopgap. The durable version drives the
  same email via the `onboarding__welcome` workflow spec
  (`workflows/specs/onboarding__welcome.yaml`), `email_send__welcome`
  step, executed on the Restate worker. The spec is bundled today;
  the worker handler for `email_send__*` is a follow-up.

  Background:
    Given a CapturingEmail backend wired into the app

  Scenario: The bootstrap admin's first login fires a welcome
    Given the IdP issues sub="kc-uuid-nick", email="nick@neonlaw.com", name="Nick"
    When the bootstrap admin completes the OAuth login dance
    Then exactly 1 captured email exists
    And the captured email is addressed to "nick@neonlaw.com"
    And the captured email subject is "Welcome to Neon Law"
    And the captured email body mentions "Nick"

  Scenario: The bootstrap admin's return login does not re-send the welcome
    Given the IdP issues sub="kc-uuid-nick", email="nick@neonlaw.com", name="Nick"
    When the bootstrap admin completes the OAuth login dance
    And the bootstrap admin completes the OAuth login dance again
    Then exactly 1 captured email exists

  Scenario: Seeded email promotion does not trigger a welcome
    Given a seeded person with email "staff@neonlaw.com" and role "staff"
    And the IdP issues sub="kc-uuid-staff", email="staff@neonlaw.com", name="Staff"
    When Staff completes the OAuth login dance
    Then no captured emails exist
