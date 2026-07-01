Feature: Client self-serve intake (the magic link)

  A client answers the client-facing questions on their notation
  themselves — the demand-side mirror of the admin walker. Staff can
  pre-fill answers on the client's behalf; the client sees them
  pre-filled and editable, and both authorships interleave on the one
  notation. The typed custom-question registry exposes all four
  retainer questions to the client; a non-participant cannot reach the
  surface at all.

  Background:
    Given a retainer matter opened for "Libra" <libra@example.com>

  Scenario: The client confirms a staff-prefilled answer and finishes their part
    Given staff pre-filled the client's name as "Staff-typed Libra"
    When the client opens their intake
    Then the intake asks the "custom_text__client_name" question
    And the intake is pre-filled with "Staff-typed Libra"
    When the client answers "Libra Prime"
    And the client opens their intake
    Then the intake asks the "custom_text__client_email" question
    When the client answers "libra@example.com"
    And the client opens their intake
    Then the intake asks the "custom_text__project_name" question
    When the client answers "Estate plan"
    And the client opens their intake
    Then the intake asks the "custom_text__product_description" question
    When the client answers "Trust formation"
    And the client opens their intake
    Then the client's part of the intake is complete
    And the client's name answer on file is "Libra Prime" from the client

  Scenario: A non-participant cannot reach the intake
    When a stranger opens the client's intake
    Then the intake response status is 404
