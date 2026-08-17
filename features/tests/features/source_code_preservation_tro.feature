Feature: Source code preservation TRO workflow shape

  The `source_code_preservation_tro__california` notation drives an emergency
  motion preserving a party's source code repositories. The questionnaire
  collects the Rule 65(d)(1)(C) repository inventory plus the custody and
  imminence facts the motion stands or falls on; the workflow routes every
  draft through `lawyer_review` before a PDF is generated.

  Scenario: Source code preservation TRO questionnaire walks BEGIN to END
    Given the bundled spec yaml "source_code_preservation_tro__california"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from                                     | to                                       |
      | BEGIN                                    | person__client                           |
      | person__client                           | entity__company                          |
      | entity__company                          | project__engagement                      |
      | project__engagement                      | custom_text__repository_inventory        |
      | custom_text__repository_inventory        | person__contact                          |
      | person__contact                          | custom_text__custody_transfer            |
      | custom_text__custody_transfer            | custom_datetime__custody_transfer_date   |
      | custom_datetime__custody_transfer_date   | custom_single_choice__independent_backup |
      | custom_single_choice__independent_backup | custom_single_choice__imminence_basis    |
      | custom_single_choice__imminence_basis    | custom_text__threat_evidence             |
      | custom_text__threat_evidence             | custom_datetime__litigation_hold_date    |
      | custom_datetime__litigation_hold_date    | custom_single_choice__personnel_relief   |
      | custom_single_choice__personnel_relief   | custom_text__protected_duties            |
      | custom_text__protected_duties            | custom_datetime__hearing_date            |
      | custom_datetime__hearing_date            | END                                      |
