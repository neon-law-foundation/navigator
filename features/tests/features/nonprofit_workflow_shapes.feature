Feature: Bundled-template workflow shapes (Foundation / nonprofit)

  The Foundation brand runs the nonprofit side of Navigator: 501(c)(3)
  formation, the annual Form 990, and state-level charitable
  solicitation registration. These scenarios pin each template's
  exact transition chain — like `legal_workflow_shapes.feature` does
  for the firm side — so an accidental reshape on the Foundation
  surface surfaces as a named failing scenario.

  A rejection scenario per template confirms the parser's MissingEnd
  guard catches a hand-mutilated copy with the workflow END dropped.

  Scenario: Nevada 501(c)(3) formation questionnaire walks mission → board → agent → END
    Given the bundled template "nonprofit/nevada_501c3_formation.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from              | to                |
      | BEGIN             | mission_statement |
      | mission_statement | board_members     |
      | board_members     | registered_agent  |
      | registered_agent  | END               |

  Scenario: Nevada 501(c)(3) formation workflow signs, reviews, and mails the articles
    Given the bundled template "nonprofit/nevada_501c3_formation.md"
    Then the workflow transitions, in BEGIN-first order, are:
      | from             | to               |
      | BEGIN            | board_signatures |
      | board_signatures | staff_review     |
      | staff_review     | mailroom_send    |
      | mailroom_send    | END              |
    And every workflow state resolves to a StepKind

  Scenario: Nevada 501(c)(3) formation template with END stripped fails to parse
    Given the bundled template "nonprofit/nevada_501c3_formation.md" with the workflow END declaration removed
    Then parsing the workflow spec returns a MissingEnd error

  Scenario: Form 990 questionnaire walks tax_year → revenue → END
    Given the bundled template "nonprofit/form990_annual_report.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from            | to              |
      | BEGIN           | tax_year        |
      | tax_year        | revenue_summary |
      | revenue_summary | END             |

  Scenario: Form 990 workflow signs, reviews, and mails to the IRS
    Given the bundled template "nonprofit/form990_annual_report.md"
    Then the workflow transitions, in BEGIN-first order, are:
      | from             | to               |
      | BEGIN            | board_signatures |
      | board_signatures | staff_review     |
      | staff_review     | mailroom_send    |
      | mailroom_send    | END              |
    And every workflow state resolves to a StepKind

  Scenario: Form 990 template with END stripped fails to parse
    Given the bundled template "nonprofit/form990_annual_report.md" with the workflow END declaration removed
    Then parsing the workflow spec returns a MissingEnd error

  Scenario: Charitable solicitation registration questionnaire walks period → activities → END
    Given the bundled template "nonprofit/nevada_charitable_solicitation_registration.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from                   | to                     |
      | BEGIN                  | annual_or_amended      |
      | annual_or_amended      | fundraising_activities |
      | fundraising_activities | END                    |

  Scenario: Charitable solicitation registration workflow reviews and mails the statement
    Given the bundled template "nonprofit/nevada_charitable_solicitation_registration.md"
    Then the workflow transitions, in BEGIN-first order, are:
      | from          | to            |
      | BEGIN         | staff_review  |
      | staff_review  | mailroom_send |
      | mailroom_send | END           |
    And every workflow state resolves to a StepKind

  Scenario: Charitable solicitation template with END stripped fails to parse
    Given the bundled template "nonprofit/nevada_charitable_solicitation_registration.md" with the workflow END declaration removed
    Then parsing the workflow spec returns a MissingEnd error
