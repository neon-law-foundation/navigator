Feature: /portal routes a person to the right home for their role

  One portal, three roles. `Role` decides the tier; participation
  decides the per-project scope. `GET /portal` is the single entry
  point — it inspects `SessionData.role` and either renders a list,
  redirects to the firm dashboard, or sends the visitor to log in.

  See [`docs/access-model.md`](../../../../docs/access-model.md).

  The N=1-client-redirect-to-`/portal/projects/:id` behaviour lives
  in [`portal_projects_detail.feature`](portal_projects_detail.feature);
  it isn't re-exercised here.

  Background:
    Given the Neon Law Navigator app is running

  Scenario: An anonymous visitor is bounced to the login flow
    When an anonymous visitor opens /portal
    Then the response status is 303
    And the redirect location starts with "/auth/login"

  Scenario: An admin lands on the firm dashboard
    Given a seeded person "nick@neonlaw.com" with role "admin"
    When "nick@neonlaw.com" opens /portal
    Then the response status is 303
    And the redirect location is "/portal/admin"

  Scenario: A staff member sees only the projects they participate in
    Given a seeded person "staff@neonlaw.com" with role "staff"
    And a project "Atlas LLC" with "staff@neonlaw.com" as a participant
    And a project "Borealis Trust" with "staff@neonlaw.com" as a participant
    And a project "Cetus Holdings" with no participants
    When "staff@neonlaw.com" opens /portal
    Then the response status is 200
    And the response body contains "Atlas LLC"
    And the response body contains "Borealis Trust"
    And the response body does not contain "Cetus Holdings"

  Scenario: A client with multiple matters sees the list of their matters
    Given a seeded person "sagittarius@example.com" with role "client"
    And a project "Matter One" with "sagittarius@example.com" as a participant
    And a project "Matter Two" with "sagittarius@example.com" as a participant
    When "sagittarius@example.com" opens /portal
    Then the response status is 200
    And the response body contains "Matter One"
    And the response body contains "Matter Two"

  Scenario: A client with no matters sees an empty-state message
    Given a seeded person "aquarius@example.com" with role "client"
    When "aquarius@example.com" opens /portal
    Then the response status is 200
    And the response body contains "Your portal is empty"

  Scenario: A client with exactly one matter is taken straight to it
    Given a seeded person "capricorn@example.com" with role "client"
    And a project "Only Matter" with "capricorn@example.com" as a participant
    When "capricorn@example.com" opens /portal
    Then the response status is 303
    And the redirect location is the project page for "Only Matter"

  Scenario: A staff member with exactly one matter still sees the list
    Given a seeded person "staff@neonlaw.com" with role "staff"
    And a project "Sole Project" with "staff@neonlaw.com" as a participant
    When "staff@neonlaw.com" opens /portal
    Then the response status is 200
    And the response body contains "Sole Project"
