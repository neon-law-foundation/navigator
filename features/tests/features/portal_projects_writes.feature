Feature: /portal/projects writes — staff/admin only, clients get 404

  The project routes are role-aware. Clients see their matter's
  lightweight detail at `/portal/projects/:id` and never see the
  write surfaces (create form, edit form, delete action). Staff and
  admin reach the admin-chrome view at the same URL and the form at
  `/portal/projects/:id/edit`.

  When a client probes a write URL, the response is `404` — not
  `403`. The matter's management surface doesn't exist from their
  perspective, in keeping with [`docs/access-model.md`](../../../../docs/access-model.md).

  Background:
    Given the Navigator app is running

  Scenario: An admin opens a matter via POST /portal/projects
    # The create form always opens a matter on a retainer, for an existing
    # client (the runner seeds the client + entity + retainer template and
    # appends `client_dri_person_id` / `entity_id` / `retainer_template_code`).
    # On success it redirects to the new retainer notation's review screen.
    Given a seeded person "nick@neonlaw.com" with role "admin"
    When "nick@neonlaw.com" submits "name=Atlas%20LLC&status=open" to /portal/projects
    Then the response status is 303
    And the response location contains "/portal/admin/notations/"

  Scenario: An admin sees the edit form at /portal/projects/:id/edit
    Given a seeded person "nick@neonlaw.com" with role "admin"
    And a project "Borealis Trust" with no participants
    When "nick@neonlaw.com" opens the edit page for "Borealis Trust"
    Then the response status is 200
    And the response body contains "Borealis Trust"
    And the response body contains "Edit project"

  Scenario: A client probing the edit page gets 404 (not 403)
    Given a seeded person "capricorn@example.com" with role "client"
    And a project "Capricorn Matter" with "capricorn@example.com" as a participant
    When "capricorn@example.com" opens the edit page for "Capricorn Matter"
    Then the response status is 404

  Scenario: A client probing the delete action gets 404 (not 403)
    Given a seeded person "capricorn@example.com" with role "client"
    And a project "Capricorn Matter" with "capricorn@example.com" as a participant
    When "capricorn@example.com" submits "" to the delete action for "Capricorn Matter"
    Then the response status is 404

  Scenario: A client viewing their own matter sees the lightweight detail (no Edit chrome)
    Given a seeded person "capricorn@example.com" with role "client"
    And a project "Capricorn Matter" with "capricorn@example.com" as a participant
    When "capricorn@example.com" opens the detail page for "Capricorn Matter"
    Then the response status is 200
    And the response body contains "Capricorn Matter"
    And the response body does not contain "Edit project"
    And the response body does not contain "Upload a document"
