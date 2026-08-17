Feature: Nautilus screening-shield, end to end

  Neon Law Nautilus is a flat $66/month consumer-report screening shield.
  This is the whole arc of one engagement, following Pisces — a bold
  rights-fighter whose rental application was denied over a wrong consumer
  report — and one Neon Law attorney from the first inbound adverse-action
  notice to a dispute letter in the mail and a statutory clock running in
  her favor.

  The journey stitches together the pieces the firm already ships: inbound
  triage routes the landlord's adverse-action notice, the FCRA dispute
  notation is walked and rendered, an attorney reviews it before anything
  leaves the building, the mailroom sends it to the reporting agency, and
  the §1681i reinvestigation window is tracked. The flat fee is a
  fixed fee, not a contingency.

  Background:
    Given a client named "Pisces" <pisces@example.com> with an active Nautilus matter

  Scenario: From an inbound adverse-action notice to a mailed, attorney-reviewed dispute letter
    When a landlord sends an adverse-action notice denying the application on a consumer report
    Then the notice is routed to open a consumer-report dispute
    When the firm walks the "nautilus__fcra_dispute" letter for the client
    And the attorney approves the letter and the mailroom sends it
    Then the fcra-dispute letter reaches END
    And the letter was sent to the reporting agency only after attorney review
    And the client's consumer-report dispute answers are on file

  Scenario: The FCRA reinvestigation clock runs in the client's favor
    Then the reinvestigation window closes 30 days after it is triggered on "2026-06-01"
    And the window cites "15 U.S.C. § 1681i(a)(1)"

  Scenario: The adverse-action free-report window runs in the client's favor
    Then the free-report window closes 60 days after it is triggered on "2026-06-01" citing "15 U.S.C. § 1681j(b)"
