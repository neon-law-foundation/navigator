Feature: Nexus fractional-GC engagement, end to end

  Neon Law Nexus is fractional general counsel — a flat $5,000-a-month
  ongoing relationship, not a one-shot matter. This follows Sagittarius,
  a founder who retains the firm as the company's outside legal department,
  through the shape of that relationship: the engagement letter is signed,
  then the firm delivers work product into the company's Project repository
  and answers the founder's questions through the support thread. The
  onboarding__nexus body is a stub; its questionnaire and workflow are the
  tested contract.

  Background:
    Given a client named "Sagittarius" <sagittarius@example.com> with a fractional-GC engagement
    And a staff member "staff@neonlaw.com"

  Scenario: The engagement is signed, then runs as an ongoing relationship
    When the firm opens the Nexus engagement for the founder
    And the founder signs the engagement letter
    Then the engagement is active
    When the firm delivers a board resolution through the Project repo
    Then the resolution appears in the Project repo listing
    When the founder emails a question to support
    Then the question is routed to staff
