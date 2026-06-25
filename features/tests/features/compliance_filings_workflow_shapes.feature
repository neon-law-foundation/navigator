Feature: Bundled-template workflow composition (compliance filings)

  Three compliance filings round out the law-firm side of the
  template tree: a Nevada LLC dissolution, the annual Nevada list of
  managers/members, and the Nevada Modified Business Tax return.
  Each one is mailed (or filed by mail) to a state office after
  staff review.

  Like `legal_workflow_shapes.feature`, each scenario pins the
  reusable-step composition so an accidental reshape — splitting the
  staff_review step, dropping the outbound mailroom hop — surfaces
  as a named failing scenario. A rejection scenario per template
  confirms the parser's MissingEnd guard stays load-bearing.

  Scenario: Nevada LLC dissolution questionnaire walks reason → debts → END
    Given the bundled template "united_states/nevada/state/business_associations/dissolution.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from                | to                  |
      | BEGIN               | dissolution_reason  |
      | dissolution_reason  | final_debts_settled |
      | final_debts_settled | END                 |

  Scenario: Nevada LLC dissolution workflow mails articles to the Secretary of State
    Given the bundled template "united_states/nevada/state/business_associations/dissolution.md"
    Then the workflow transitions, in BEGIN-first order, are:
      | from              | to                |
      | BEGIN             | member_signatures |
      | member_signatures | staff_review      |
      | staff_review      | mailroom_send     |
      | mailroom_send     | END               |

  Scenario: Nevada dissolution template with END stripped fails to parse
    Given the bundled template "united_states/nevada/state/business_associations/dissolution.md" with the workflow END declaration removed
    Then parsing the workflow spec returns a MissingEnd error

  Scenario: Nevada annual report questionnaire walks period → managers → END
    Given the bundled template "united_states/nevada/state/business_associations/annual_report.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from              | to                |
      | BEGIN             | annual_or_amended |
      | annual_or_amended | managers_list     |
      | managers_list     | END               |

  Scenario: Nevada annual report workflow mails the list after staff review
    Given the bundled template "united_states/nevada/state/business_associations/annual_report.md"
    Then the workflow transitions, in BEGIN-first order, are:
      | from          | to            |
      | BEGIN         | staff_review  |
      | staff_review  | mailroom_send |
      | mailroom_send | END           |

  Scenario: Nevada annual report template with END stripped fails to parse
    Given the bundled template "united_states/nevada/state/business_associations/annual_report.md" with the workflow END declaration removed
    Then parsing the workflow spec returns a MissingEnd error

  Scenario: Nevada Modified Business Tax questionnaire walks year → revenue → END
    Given the bundled template "united_states/nevada/state/taxation/modified_business_tax.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from          | to            |
      | BEGIN         | tax_year      |
      | tax_year      | gross_revenue |
      | gross_revenue | END           |

  Scenario: Nevada Modified Business Tax workflow signs, reviews, and mails the return
    Given the bundled template "united_states/nevada/state/taxation/modified_business_tax.md"
    Then the workflow transitions, in BEGIN-first order, are:
      | from              | to                |
      | BEGIN             | member_signatures |
      | member_signatures | staff_review      |
      | staff_review      | mailroom_send     |
      | mailroom_send     | END               |

  Scenario: Nevada Modified Business Tax template with END stripped fails to parse
    Given the bundled template "united_states/nevada/state/taxation/modified_business_tax.md" with the workflow END declaration removed
    Then parsing the workflow spec returns a MissingEnd error
