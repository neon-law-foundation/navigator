Feature: Neon Law Nautilus correspondence workflows

  Nautilus is the $66/month consumer-report screening shield: screening
  mail — adverse-action notices, forwarded reports, and a consumer
  reporting agency's reinvestigation results — comes to the firm and goes
  back out as attorney-signed FCRA dispute letters under the client's
  rights. The dispute letter is a bundled notation whose questionnaire
  collects the intake and whose workflow renders the letter, gates it
  behind attorney review (the `@approve` gate, modeled as a bare
  `lawyer_review` state), and only then sends it. These scenarios pin the
  notation's shape, prove the unauthorized-practice-of-law gate holds, and
  lock down inbound triage and the litigation boundary — so an accidental
  reshape (dropping the review gate, or wiring an auto-send path) surfaces
  as a named failing scenario.

  Scenario: FCRA dispute intake walks client → agency → item → error → END
    Given the bundled template "neon_law/nautilus/fcra_dispute.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from                             | to                               |
      | BEGIN                            | person__client                   |
      | person__client                   | custom_text__reporting_agency    |
      | custom_text__reporting_agency    | custom_text__disputed_item       |
      | custom_text__disputed_item       | custom_text__report_error        |
      | custom_text__report_error        | END                              |

  Scenario: FCRA dispute letter renders, is attorney-reviewed, then mailed
    Given the bundled template "neon_law/nautilus/fcra_dispute.md"
    Then every workflow state resolves to a StepKind
    And the workflow gates every outbound letter behind attorney review

  Scenario Outline: Inbound triage routes screening mail on an active matter
    Given an inbound screening email on an active matter saying "<text>"
    Then it is classified as "<class>" and routed to "<route>"

    Examples:
      | text                                                                    | class                 | route                 |
      | You are being sued; a summons is enclosed in this civil action.         | LawsuitOrSummons      | ReferLitigation       |
      | Enclosed are the results of your reinvestigation of the disputed item.  | ReinvestigationResult | ReinvestigationReview |
      | We denied your application based on information in your consumer report. | AdverseAction         | OpenDispute           |
      | Attached is the tenant screening report the landlord ran on you.        | ReportForwarded       | OpenDispute           |
      | Attached is my screening report; the eviction record is not mine.       | ReportForwarded       | OpenDispute           |
      | Please call our office at your convenience.                             | Other                 | LawyerReview           |

  Scenario: Inbound mail from an unmatched sender is flagged for a lawyer
    Given an inbound screening email with no matching matter saying "We denied your application based on your consumer report."
    Then it is routed to "LawyerReview"

  Scenario Outline: A consumer reporting agency's reinvestigation result is classified for the client
    Given a consumer reporting agency reinvestigation response saying "<text>"
    Then the FCRA result is "<result>"

    Examples:
      | text                                                         | result             |
      | The disputed item has been deleted from your file.           | CorrectedOrDeleted |
      | We verified the item as accurate; it remains on your report. | VerifiedUnchanged  |

  Scenario: A lawsuit leaves the shield and is referred to litigation counsel
    Given an inbound screening email on an active matter saying "You are being sued; a summons in this civil action is enclosed."
    Then it is classified as "LawsuitOrSummons" and routed to "ReferLitigation"
    And the litigation referral links to "/contact" and is not answered as correspondence
