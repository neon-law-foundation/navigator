Feature: Profit corporation and business trust formations on the official packets

  Nest forms a Nevada entity for a flat $1,111 a year — and "entity" is
  bigger than the LLC. These journeys follow the same founder through the
  other two formation packets the Secretary of State publishes: a profit
  corporation (NRS 78) and a business trust (NRS 88A). Each template binds
  the state's own AcroForm packet, so the founder's answers land on the
  official form via its field map, a Neon Law attorney reviews the filled
  packet, and the matter ends at a recorded Secretary-of-State filing.

  Background:
    Given a fresh Navigator app with the canonical templates seeded
    And a client named "Libra" <libra@example.com>

  Scenario: A profit corporation forms on the official SoS packet
    When the firm opens the "onboarding__nest_corp" matter for the client
    And the founder answers the formation questionnaire:
      | value |
      | Libra |
      | libra@example.com |
      | Bright Star Inc |
      | Neon Law Registered Agent |
      | Libra; 1 Main St; Las Vegas; NV; 89101; USA |
      | Libra; President; 1 Main St; Las Vegas; NV; 89101; USA |
      | 1000 |
      | 0.01 |
    Then the formation reaches the signature wait
    And the persisted corporation packet carries the founder's answers
    When the attorney files the formation packet with the Nevada Secretary of State
    Then the formation workflow reaches END
    And a filing was recorded with the "Nevada Secretary of State"

  Scenario: A business trust forms on the official SoS packet
    When the firm opens the "onboarding__nest_business_trust" matter for the client
    And the founder answers the formation questionnaire:
      | value |
      | Libra |
      | libra@example.com |
      | Bright Star Holdings |
      | Neon Law Registered Agent |
      | Libra; 1 Main St; Las Vegas; NV; 89101; USA |
    Then the formation reaches the signature wait
    And the persisted business-trust packet carries the founder's answers
    When the attorney files the formation packet with the Nevada Secretary of State
    Then the formation workflow reaches END
    And a filing was recorded with the "Nevada Secretary of State"
