Feature: Nautilus debt-shield, end to end

  Neon Law Nautilus is a flat $44-a-month debt-collection shield. This is
  the whole arc of one engagement, following Pisces — a bold rights-fighter
  who refuses to be pushed around by a collector — and one Neon Law attorney
  from the first inbound collector contact to a letter in the mail and a
  statutory clock running in her favor.

  The journey stitches together the pieces the firm already ships: inbound
  triage routes the collector's contact, the debt-validation notation is
  walked and rendered, an attorney reviews it before anything leaves the
  building, the mailroom sends it, and the FDCPA validation window is
  tracked. Throughout, the flat fee never takes a cut of anything the client
  saves — the shield is a subscription, not a contingency.

  Background:
    Given a client named "Pisces" <pisces@example.com> with an active Nautilus matter

  Scenario: From an inbound collector contact to a mailed, attorney-reviewed letter
    When a collector makes first contact demanding payment of an alleged debt
    Then the contact is routed to debt validation
    When the firm walks the "nautilus__debt_validation" letter for the client
    And the attorney approves the letter and the mailroom sends it
    Then the debt-validation letter reaches END
    And the letter was sent to the collector only after attorney review
    And the founder's debt-validation answers are on file

  Scenario: The FDCPA validation clock runs in the client's favor
    Then the debt-validation window closes 30 days after it is triggered on "2026-06-01"
    And the window cites "15 U.S.C. § 1692g(a)"

  Scenario: The flat fee never takes a cut of what the client saves
    Then settling a debt and saving 250000 cents costs the client a 0-cent firm cut
