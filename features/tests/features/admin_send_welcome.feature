Feature: Admin re-sends welcome email from /portal/admin/people

  Staff sometimes need to re-fire a welcome email — the OAuth callback
  fires one on first signup, but a user who never opened that one (or
  whose first signup predates this feature) should be reachable from
  the admin people index without leaving the browser.

  Every send through the `EmailService` trait is journaled to
  `sent_emails` by the `LoggingEmail` decorator regardless of trigger
  source. That guarantees the admin button and the callback share one
  audit story.

  Background:
    Given the application uses a CapturingEmail backend wrapped in LoggingEmail

  Scenario: Staff clicks Send welcome for an existing person
    Given a persons row for "Aries" with email "aries@example.com"
    When staff POSTs to /portal/admin/people/{aries.id}/welcome
    Then the response is a redirect to /portal/admin/people
    And exactly 1 sent_emails row exists
    And the sent_emails row has recipient "aries@example.com"
    And the sent_emails row has subject "Welcome to Neon Law"
    And the sent_emails row has template_slug "welcome"
    And the sent_emails row has outcome "sent"

  Scenario: Staff clicks Send welcome twice in a row
    Given a persons row for "Aries" with email "aries@example.com"
    When staff POSTs to /portal/admin/people/{aries.id}/welcome
    And staff POSTs to /portal/admin/people/{aries.id}/welcome
    Then exactly 2 sent_emails rows exist
    # Append-only: each click is its own row, never an UPDATE.

  Scenario: Staff clicks Send welcome for a missing person
    When staff POSTs to /portal/admin/people/{random_uuid}/welcome
    Then the response is 404
    And no sent_emails rows are written
