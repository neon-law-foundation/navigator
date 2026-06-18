Feature: /portal/projects/:id — single matter detail, scoped to the caller

  The project detail page is the place clients spend their time. It
  reads from the same `visible_projects` rule that gates the
  portal-landing list, applied per-row: callers who can see the
  project get `200`; callers who cannot get `404`, not `403`. Lower
  tiers don't get to learn that the matter exists.

  Admins still bypass per-row scoping silently per
  [`docs/access-model.md`](../../../../docs/access-model.md).

  Background:
    Given the Navigator app is running

  Scenario: An admin can read any project's detail
    Given a seeded person "nick@neonlaw.com" with role "admin"
    And a project "Atlas LLC" with no participants
    When "nick@neonlaw.com" opens the detail page for "Atlas LLC"
    Then the response status is 200
    And the response body contains "Atlas LLC"

  Scenario: A staff participant reads the detail page
    Given a seeded person "staff@neonlaw.com" with role "staff"
    And a project "Borealis Trust" with "staff@neonlaw.com" as a participant
    When "staff@neonlaw.com" opens the detail page for "Borealis Trust"
    Then the response status is 200
    And the response body contains "Borealis Trust"

  Scenario: A staff member who isn't on the matter gets a 404
    Given a seeded person "staff@neonlaw.com" with role "staff"
    And a project "Cetus Holdings" with no participants
    When "staff@neonlaw.com" opens the detail page for "Cetus Holdings"
    Then the response status is 404

  Scenario: A client participant reads their own matter
    Given a seeded person "capricorn@example.com" with role "client"
    And a project "Capricorn Matter" with "capricorn@example.com" as a participant
    When "capricorn@example.com" opens the detail page for "Capricorn Matter"
    Then the response status is 200
    And the response body contains "Capricorn Matter"

  Scenario: A client cannot peek at someone else's matter (404, not 403)
    Given a seeded person "sagittarius@example.com" with role "client"
    And a project "Other Client's Matter" with no participants
    When "sagittarius@example.com" opens the detail page for "Other Client's Matter"
    Then the response status is 404
