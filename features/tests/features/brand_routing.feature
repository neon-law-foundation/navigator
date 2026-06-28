Feature: Public site brand routing

  One binary serves two brands. Each handler picks its `SiteBrand`
  via `PageLayout::with_brand`; the layout never branches on the URL.
  The footer is now unified site-wide (same copyright on every page),
  so the per-brand marker we assert on is the `og:site_name` the
  layout emits from each page's `SiteBrand`.

  Background:
    Given the Neon Law Navigator public site is running

  Scenario Outline: Firm-branded pages carry the Neon Law brand
    When a visitor opens <path>
    Then the response status is 200
    And the page is branded "Neon Law"
    And the page is not branded "Neon Law Foundation"

    Examples:
      | path                 |
      | /                    |
      | /contact             |
      | /services/northstar  |
      | /services/nest       |

  Scenario Outline: Foundation-branded pages carry the Foundation brand
    When a visitor opens <path>
    Then the response status is 200
    And the page is branded "Neon Law Foundation"

    Examples:
      | path                              |
      | /foundation                       |
      | /privacy                          |
      | /terms                            |

  Scenario: The old /foundation/contact URL permanently redirects to the shared contact page
    When a visitor opens /foundation/contact
    Then the response status is 308
    And the response redirects to "/contact"

  Scenario: The old /foundation/workshops/navigator URL permanently redirects to its Nebula home
    When a visitor opens /foundation/workshops/navigator
    Then the response status is 308
    And the response redirects to "/foundation/nebula/workshops/use-the-navigator"

  Scenario: The old /foundation/mission URL permanently redirects to the Foundation home
    When a visitor opens /foundation/mission
    Then the response status is 308
    And the response redirects to "/foundation"

  Scenario: The bare /navigator permanently redirects to the Foundation hub
    When a visitor opens /navigator
    Then the response status is 308
    And the response redirects to "/foundation/navigator"

  Scenario: The retired /education route returns 404
    When a visitor opens /education
    Then the response status is 404

  Scenario: The retired /workshops/genai-training route returns 404
    When a visitor opens /workshops/genai-training
    Then the response status is 404

  Scenario: An unknown route returns 404
    When a visitor opens /does-not-exist
    Then the response status is 404
