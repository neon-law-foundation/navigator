Feature: Questionnaire intake in the client's own language
  The questionnaire's "answered in their own words" promise is a named
  requirement, not an assumption: a client whose `preferred_language` is
  Spanish sees the attorney-reviewed Spanish prompts and can complete
  intake end-to-end. Translation is reviewed copy seeded into
  `question_translations` — it does not bypass the staff_review gate.

  Background:
    Given a fresh Neon Law Navigator app with the canonical templates seeded
    And a Spanish-speaking client "Gemini" <gemini@example.com> with a retainer notation at BEGIN

  Scenario: The questionnaire renders the first prompt in Spanish
    When the staff visits /portal/admin/notations/:id/step
    Then the response status is 200
    And the page shows "¿Cuál es el nombre legal completo del cliente?"

  Scenario: Walking the questionnaire in Spanish reaches END
    When the staff submits the full questionnaire:
      | value            |
      | Gemini           |
      | Plan patrimonial |
    Then the final response status is 200
    And the last questionnaire transition lands on "END"
