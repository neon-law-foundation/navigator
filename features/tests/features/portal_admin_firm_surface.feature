Feature: /portal/admin/* — firm-wide CRUD, staff-tier only

  Every firm-wide CRUD route (people, entities, templates, …)
  answers at `/portal/admin/*`. Authorization is `staff_tier` only,
  via OPA. Client tier gets a redirect-to-login from OPA — the
  matter doesn't exist from their perspective.

  Background:
    Given the Navigator app is running

  Scenario: An admin reads the firm-wide people index
    Given a seeded person "nick@neonlaw.com" with role "admin"
    When "nick@neonlaw.com" opens /portal/admin/people
    Then the response status is 200

  Scenario: A staff member reads the firm-wide entities index
    Given a seeded person "staff@neonlaw.com" with role "staff"
    When "staff@neonlaw.com" opens /portal/admin/entities
    Then the response status is 200

  Scenario: A staff member reads the firm dashboard at /portal/admin
    Given a seeded person "staff@neonlaw.com" with role "staff"
    When "staff@neonlaw.com" opens /portal/admin
    Then the response status is 200
    And the response body contains "Admin"

  # The client-blocked-from-/portal/admin scenario is enforced by OPA's
  # `/portal/admin` staff_tier rule
  # in production; the BDD app runs with `PolicyClient::passthrough`
  # so every request reaches the handler. Verify that flow against a
  # live KIND cluster instead — covered by the smoke test in PR 4.
