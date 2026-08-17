Feature: /app/projects writes — the firm tiers on their own matters, clients get 404

  One path serves both lenses. A client sees their matter's lightweight
  detail at `/app/projects/:id` and never the write surfaces (create form,
  edit form, delete action). Owner, Admin, and Lawyer reach the firm
  workbench at `/app/projects` and the form at `/app/projects/:id/edit` —
  but only on matters they are actually on. Since ENG-81 there is no
  privileged bypass on the matter surface: a firm-side
  `person_project_roles` row is required of every tier.

  When a client probes a write URL, the response is `404` — not
  `403`. The matter's management surface doesn't exist from their
  perspective, in keeping with [`docs/access-model.md`](../../../../docs/access-model.md).

  Background:
    Given the Neon Law Navigator app is running

  Scenario: An Owner sees the edit form for a matter they are on
    Given a seeded person "owner@neonlaw.com" with role "owner"
    And a project "Owner Matter" with "owner@neonlaw.com" as a participant
    When "owner@neonlaw.com" opens the edit page for "Owner Matter"
    Then the response status is 200
    And the response body contains "Edit project"

  Scenario: An Owner does not see the edit form for a matter nobody put them on
    # The guard for the ENG-81 decision. Before it, the `is_admin_tier()`
    # short-circuit handed Owner and Admin every matter silently, so this
    # scenario is the one that would otherwise pass by accident.
    Given a seeded person "owner@neonlaw.com" with role "owner"
    And a project "Owner Matter" with no participants
    When "owner@neonlaw.com" opens the edit page for "Owner Matter"
    Then the response body does not contain "Edit project"

  Scenario: An admin opens a matter via POST /app/projects
    # The create form opens a matter for an existing client
    # (the runner seeds the client + entity and appends
    # `client_dri_person_id` / `entity_id`, plus the required
    # conflict `attestation`). It opens the matter and only the matter — the
    # retainer is a separate step — so it redirects to the matter itself, not
    # to a notation it opened for you.
    Given a seeded person "nick@neonlaw.com" with role "admin"
    When "nick@neonlaw.com" submits "name=Atlas%20LLC" to /app/projects
    Then the response status is 303
    And the response location contains "/app/projects/"

  Scenario: An admin sees the edit form for a matter they are on
    Given a seeded person "nick@neonlaw.com" with role "admin"
    And a project "Borealis Trust" with "nick@neonlaw.com" as a participant
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
    And the response body does not contain "Upload documents"
