Feature: Neon Law Nautilus correspondence workflows

  Nautilus is the $44/month debt-collection shield: collector mail comes
  to the firm and goes back out as attorney-signed letters under the
  client's FDCPA / FCRA rights. Each letter is a bundled notation whose
  questionnaire collects the intake and whose workflow renders the
  letter, gates it behind attorney review (the `@approve` gate, modeled
  as a bare `staff_review` state), and only then sends it. These
  scenarios pin each notation's shape and prove the unauthorized-
  practice-of-law gate holds, so an accidental reshape — dropping the
  review gate, or wiring an auto-send path — surfaces as a named
  failing scenario.

  Scenario: Notice of representation intake walks client → collector → consent → END
    Given the bundled template "neon_law/nautilus/notice_of_representation.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from                 | to                   |
      | BEGIN                | client_name          |
      | client_name          | client_email         |
      | client_email         | collector_name       |
      | collector_name       | collector_address    |
      | collector_address    | alleged_account      |
      | alleged_account      | consent_to_represent |
      | consent_to_represent | END                  |

  Scenario: Notice of representation renders, is attorney-reviewed, then mailed
    Given the bundled template "neon_law/nautilus/notice_of_representation.md"
    Then every workflow state resolves to a StepKind
    And the workflow gates every outbound letter behind attorney review

  Scenario Outline: Inbound triage routes collector mail on an active matter
    Given an inbound collector email on an active matter saying "<text>"
    Then it is classified as "<class>" and routed to "<route>"

    Examples:
      | text                                                                  | class             | route           |
      | You are being sued; a summons is enclosed in this civil action.       | LawsuitOrSummons  | ReferLitigation |
      | Enclosed is the verification of the debt you requested.               | ValidationResponse| DebtValidation  |
      | We can settle this account for a lump sum of 60% of the balance.      | SettlementOffer   | Settlement      |
      | This is an attempt to collect a debt. The amount due is past due.     | NewContact        | DebtValidation  |
      | Please call our office at your convenience.                           | Other             | StaffReview     |

  Scenario: Inbound mail from an unmatched sender is flagged for staff
    Given an inbound collector email with no matching matter saying "This is an attempt to collect a debt."
    Then it is routed to "StaffReview"

  Scenario: Debt validation intake walks debt → creditor → dispute → END
    Given the bundled template "neon_law/nautilus/debt_validation.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from              | to                |
      | BEGIN             | client_name       |
      | client_name       | collector_name    |
      | collector_name    | alleged_account   |
      | alleged_account   | original_creditor |
      | original_creditor | disputed_reason   |
      | disputed_reason   | END               |

  Scenario: Debt validation letter is attorney-reviewed before it is mailed
    Given the bundled template "neon_law/nautilus/debt_validation.md"
    Then every workflow state resolves to a StepKind
    And the workflow gates every outbound letter behind attorney review

  Scenario Outline: A collector's verification response is classified for the client
    Given a collector verification response saying "<text>"
    Then the verification outcome is "<outcome>"

    Examples:
      | text                                                          | outcome     |
      | Enclosed is the verification of the debt with an itemization. | Verified    |
      | We are unable to verify this debt and have ceased collection. | NotVerified |
      | We can verify a portion of the balance only.                 | Partial     |

  Scenario: Collecting during an open dispute is flagged as a possible FDCPA violation
    Given a written dispute is open and no verification has been mailed
    When the collector makes a fresh collection attempt
    Then a possible FDCPA violation is flagged for attorney review

  Scenario: Cease-communication letter is attorney-reviewed before it is mailed
    Given the bundled template "neon_law/nautilus/cease_communication.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from        | to             |
      | BEGIN       | client_name    |
      | client_name | collector_name |
      | collector_name | END         |
    And every workflow state resolves to a StepKind
    And the workflow gates every outbound letter behind attorney review

  Scenario: A cease letter is honest that it does not erase the debt
    Then the cease-communication disclaimer says it does not erase the debt

  Scenario: FCRA dispute intake walks bureau → tradeline → error → END
    Given the bundled template "neon_law/nautilus/fcra_dispute.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from         | to           |
      | BEGIN        | client_name  |
      | client_name  | credit_bureau|
      | credit_bureau| tradeline    |
      | tradeline    | report_error |
      | report_error | END          |
    And every workflow state resolves to a StepKind
    And the workflow gates every outbound letter behind attorney review

  Scenario Outline: A bureau's FCRA reinvestigation result is classified for the client
    Given a credit bureau reinvestigation response saying "<text>"
    Then the FCRA result is "<result>"

    Examples:
      | text                                                         | result            |
      | The disputed item has been deleted from your file.           | CorrectedOrDeleted|
      | We verified the item as accurate; it remains on your report. | VerifiedUnchanged |

  Scenario: Settlement intake walks target → terms → authorization → END
    Given the bundled template "neon_law/nautilus/settlement_letter.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from                | to                  |
      | BEGIN               | client_name         |
      | client_name         | collector_name      |
      | collector_name      | settlement_target   |
      | settlement_target   | settlement_terms    |
      | settlement_terms    | client_authorization|
      | client_authorization| END                 |

  Scenario: Settlement is client-authorized and attorney-reviewed before it is mailed
    Given the bundled template "neon_law/nautilus/settlement_letter.md"
    Then every workflow state resolves to a StepKind
    And the workflow gates every outbound letter behind attorney review

  Scenario Outline: The firm never takes a cut of the client's settlement savings
    Given the client saves <savings> cents in settlement
    Then the firm's cut is 0 cents

    Examples:
      | savings   |
      | 0         |
      | 50000     |
      | 5000000   |

  Scenario: A lawsuit leaves the shield and is referred to litigation counsel
    Given an inbound collector email on an active matter saying "You are being sued; a summons in this civil action is enclosed."
    Then it is classified as "LawsuitOrSummons" and routed to "ReferLitigation"
    And the litigation referral links to "/services/litigation" and is not answered as correspondence
