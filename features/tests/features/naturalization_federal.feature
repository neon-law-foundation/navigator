Feature: Naturalization, end to end (Form N-400 → Certificate of Naturalization)

  Neon Law's first immigration workflow. It follows one lawful permanent
  resident, Maria Santos, from the Form N-400 intake through the firm's
  review, her signature, and the filing with USCIS, then across the three
  USCIS milestones — the biometrics appointment, the interview and civics
  test, and the oath ceremony — to the moment USCIS issues her Certificate
  of Naturalization (Form N-550), the lifelong proof of citizenship that
  ends the matter.

  The naturalization__federal template binds the intake questionnaire and
  the workflow. The rendered N-400 intake summary is the artifact she signs;
  the workflow records the USCIS filing and files the issued certificate
  into the matter.

  Background:
    Given a fresh Navigator app with the canonical templates seeded
    And a client named "Maria Santos" <maria@example.com>

  Scenario: From N-400 intake to the Certificate of Naturalization
    When the firm opens the "naturalization__federal" matter for the client
    And the applicant answers the naturalization questionnaire:
      | value             |
      | Maria Santos      |
      | maria@example.com |
      | 1990-04-12        |
      | Mexico            |
      | Mexico            |
      | A123456789        |
      | 2019-03-01        |
      | 702-555-0100      |
      | five_year         |
      | married           |
      | 45                |
      | no                |
    Then the application reaches the signature wait
    And the persisted N-400 intake summary is a rendered PDF
    When the applicant signs and the firm e-files the Form N-400 with USCIS
    Then the naturalization workflow reaches the biometrics milestone
    When USCIS sends the biometrics, interview, and oath notices
    Then the naturalization workflow awaits the Certificate of Naturalization
    When USCIS issues the Certificate of Naturalization
    Then the naturalization workflow reaches END
    And a USCIS filing was recorded
    And the issued Certificate of Naturalization is filed in the matter
    And the applicant's twelve intake answers are on file
