Feature: MCP conversational notation creation
  An LLM client drives a notation end-to-end through the
  `aida_create_notation` and `aida_answer_notation` MCP tools.
  The server owns the questionnaire state machine; the client
  just relays prompts to the user and submits the answers.

  Background:
    Given a fresh Navigator app with the canonical templates seeded
    And a seeded person "Libra" with email "libra@example.com"

  Scenario: Full retainer walk over MCP advances the questionnaire to END
    When the LLM calls aida_create_notation for "onboarding__retainer" as "libra@example.com"
    Then the MCP response status is "needs_answer"
    And the MCP next question is "client_name"

    When the LLM calls aida_answer_notation with code "client_name" value "Libra"
    Then the MCP response status is "needs_answer"
    And the MCP next question is "client_email"

    When the LLM calls aida_answer_notation with code "client_email" value "libra@example.com"
    Then the MCP response status is "needs_answer"
    And the MCP next question is "project_name"

    When the LLM calls aida_answer_notation with code "project_name" value "Apollo"
    Then the MCP response status is "needs_answer"
    And the MCP next question is "product_description"

    When the LLM calls aida_answer_notation with code "product_description" value "rocket"
    Then the MCP response status is "complete"
    And the notation has reached the questionnaire END state

  Scenario: Answering with the wrong question code is rejected as invalid arguments
    When the LLM calls aida_create_notation for "onboarding__retainer" as "libra@example.com"
    Then the MCP response status is "needs_answer"

    When the LLM calls aida_answer_notation with code "project_name" value "Apollo"
    Then the MCP tool error mentions "client_name"
