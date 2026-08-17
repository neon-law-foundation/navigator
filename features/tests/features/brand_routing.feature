Feature: Public routing across one site's two faces

  The `neon` binary serves the whole site. Neon Law — the firm — holds the
  root, and the Neon Law Foundation sits beneath `/foundation`. They were two
  binaries on two hosts; the prefix is what keeps them separable now that one
  binary answers for both. The per-brand marker we assert on is the
  `og:site_name` the layout emits from each page's `SiteBrand`, which is how a
  page that mounted under the wrong prefix is caught wearing the wrong name.

  Background:
    Given the Neon Law Navigator public site is running

  # Every scenario here builds a whole app in its Background, and each build
  # takes a slice of the Dioxus pinned-worker pool, so this file stays a thin
  # representative sample on purpose. The exhaustive per-path tables — every
  # gated page, every retired redirect — live in `server/tests/routes.rs`,
  # which drives one router.
  #
  # This harness loads no Nebula content, so the material catalogs are not
  # asserted here; `server/tests/firm_routes.rs` covers them against real
  # content.

  Scenario: The firm's front door is the site root
    When a visitor opens /
    Then the response status is 200
    And the page is branded "Neon Law"

  Scenario: The Foundation's front door is its own prefix
    When a visitor opens /foundation
    Then the response status is 200
    And the page is branded "Neon Law Foundation"

  Scenario Outline: The firm's anonymous marketing surface serves at the root
    # These 404'd on the Foundation host while the two were separate. One
    # binary serves them now, and each is anonymous: a stranger deciding
    # whether to hire a lawyer must not meet a login door.
    When a visitor opens <path>
    Then the response status is 200

    Examples:
      | path           |
      | /contact       |
      | /litigation    |
      | /fractional-gc |
      | /services      |

  Scenario Outline: Everything else the Foundation publishes needs a session
    # The nav still names these, so a signed-out reader learns they exist and
    # meets the login door rather than a 404.
    When a visitor opens <path>
    Then the response status is 303

    Examples:
      | path                     |
      | /foundation/transparency |

  Scenario Outline: The Foundation's former root URLs redirect beneath its prefix
    # These were live pages on `neonlaw.org` for as long as the Foundation had
    # a host of its own, so they are the most-linked retired URLs on the site.
    # The firm holds the root now, so each has to be carried across rather than
    # dropped on a firm page.
    When a visitor opens <path>
    Then the response status is 308
    And the response redirects to <destination>

    Examples:
      | path            | destination                |
      | /mission        | "/foundation/mission"      |
      | /notations      | "/foundation/notations"    |
      | /transparency   | "/foundation/transparency" |
      | /education      | "/foundation/education"    |
      | /show-and-tell  | "/foundation/show-and-tell" |

  Scenario: The Foundation home is a page, not a redirect
    # It `301`ed to `/` while the Foundation was canonical at the site root.
    # Reinstating that would bounce the nonprofit's own home page onto the
    # firm's — the single most damaging way this consolidation could regress.
    When a visitor opens /foundation
    Then the response status is 200

  Scenario: An unknown route returns 404
    When a visitor opens /does-not-exist
    Then the response status is 404
