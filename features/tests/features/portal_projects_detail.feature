Feature: /app/projects/:id — single matter detail, scoped to the caller

  The project detail page is the place clients spend their time. It
  reads from the same client-lens visibility rule that gates the
  portal-landing list, applied per-row: callers who can see the
  project as clients get `200`; callers who cannot get `404`, not
  `403`. Lower tiers don't get to learn that the matter exists.

  Admins keep their bypass on `/lawyer`, not on the client lens the
  `/app/projects` surface renders. A firm admin who is also a client
  sees their own client-side matters here.

  Background:
    Given the Neon Law Navigator app is running

  Scenario: An admin cannot read an unrelated project through the client lens
    Given a seeded person "nick@neonlaw.com" with role "admin"
    And a project "Atlas LLC" with no participants
    When "nick@neonlaw.com" opens the detail page for "Atlas LLC"
    Then the response status is 404

  Scenario: A lawyer who is a client participant reads the detail page
    Given a seeded person "lawyer@neonlaw.com" with role "lawyer"
    And a project "Borealis Trust" with "lawyer@neonlaw.com" as a participant
    When "lawyer@neonlaw.com" opens the detail page for "Borealis Trust"
    Then the response status is 200
    And the response body contains "Borealis Trust"

  Scenario: A lawyer who isn't on the matter gets a 404
    Given a seeded person "lawyer@neonlaw.com" with role "lawyer"
    And a project "Cetus Holdings" with no participants
    When "lawyer@neonlaw.com" opens the detail page for "Cetus Holdings"
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
