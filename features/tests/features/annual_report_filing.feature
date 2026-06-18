Feature: A Nevada annual report runs end-to-end to a recorded filing
  The compliance steps are no longer human-driven dead-ends: when the
  annual-report workflow reaches `mailroom_send` (after staff_review),
  the worker records a durable `filings` row — the firm's proof of what
  was mailed to which office — and the flow advances to END. The filing
  side effect can only fire after the attorney review gate, by spec
  construction.

  Background:
    Given an annual-report notation for a project

  Scenario: The annual report reaches END and records a filing after review
    When the annual-report workflow runs through staff_review to mailroom_send and END
    Then the workflow reached "END"
    And one filing was recorded for the notation
    And the recorded filing's office is "Nevada Secretary of State"

  Scenario: The annual-report spec gates the filing behind staff_review
    Then no submission in the annual-report spec is reachable without staff_review
