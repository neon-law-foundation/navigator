Feature: Spanish-language client journey, end to end

  A Spanish-speaking client walks the same pre-engagement funnel an English
  speaker does — landing page, services index, the Neon Law Nest product
  page, and the mission behind the pricing — entirely under the `/es`
  locale. This proves journey 1 (Nest formation) carries the same flow in
  Spanish: every step is served in the reader's language and never drops
  them back out of the `/es` funnel (`project_i18n_spanish_phase1`).

  Background:
    Given a fresh Navigator app with the canonical templates seeded

  Scenario: The whole pre-engagement funnel stays in Spanish
    When a Spanish-speaking client opens "/es"
    Then the page is served in Spanish
    And the navigation stays within the "/es" funnel
    When a Spanish-speaking client opens "/es/services"
    Then the page is served in Spanish
    And the navigation stays within the "/es" funnel
    When a Spanish-speaking client opens "/es/services/corporate"
    Then the page is served in Spanish
    And the navigation stays within the "/es" funnel
    When a Spanish-speaking client opens "/es/foundation/mission"
    Then the page is served in Spanish
