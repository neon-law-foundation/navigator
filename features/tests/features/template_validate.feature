Feature: Template validation rules

  The `rules` crate runs M-family (Markdown), F-family (Frontmatter
  /semantic), and S-family (Style) rules over notation source files.
  Each rule is pure: it takes a `SourceFile` and returns a list of
  `Violation`s. No DB, no async, no I/O.

  Scenario: S101 flags lines that exceed 120 characters
    Given the markdown:
      """
      # heading

      xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
      """
    When the markdown is linted with rule "S101"
    Then 1 violation is reported
    And the violation code is "S101"
    And the violation message contains "max 120"

  Scenario: S101 accepts a line at exactly the limit
    Given the markdown:
      """
      # heading

      xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
      """
    When the markdown is linted with rule "S101"
    Then 0 violations are reported

  Scenario: S101 lints frontmatter too — a long scalar line is flagged
    Given the markdown:
      """
      ---
      description: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
      ---

      body
      """
    When the markdown is linted with rule "S101"
    Then 1 violation is reported
    And the violation code is "S101"
    And the violation message contains "max 120"

  Scenario: S101 accepts a long frontmatter value wrapped as a YAML folded scalar
    Given the markdown:
      """
      ---
      description: >
        A long human description that has been wrapped across two
        physical lines so each one stays comfortably under the limit.
      ---

      body
      """
    When the markdown is linted with rule "S101"
    Then 0 violations are reported

  Scenario: F101 flags missing frontmatter
    Given the markdown:
      """
      # No frontmatter here
      Just a body.
      """
    When the markdown is linted with rule "F101"
    Then 1 violation is reported
    And the violation code is "F101"
    And the violation message contains "Missing"

  Scenario: F101 flags an empty title
    Given the markdown:
      """
      ---
      title:
      ---
      """
    When the markdown is linted with rule "F101"
    Then 1 violation is reported
    And the violation message contains "empty"

  Scenario: F102 rejects an invalid respondent_type value
    Given the markdown:
      """
      ---
      title: Trust
      respondent_type: corporation
      ---
      """
    When the markdown is linted with rule "F102"
    Then 1 violation is reported
    And the violation message contains "corporation"

  Scenario Outline: F102 accepts each valid respondent_type
    Given the markdown:
      """
      ---
      title: Doc
      respondent_type: <kind>
      ---
      """
    When the markdown is linted with rule "F102"
    Then 0 violations are reported

    Examples:
      | kind              |
      | entity            |
      | person            |
      | person_and_entity |
