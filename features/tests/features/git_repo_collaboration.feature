Feature: Git project-repo collaboration, end to end

  Every Project is an append-only git repository served from `web`. This
  follows one matter's document making the round trip: the firm commits a
  document into the Project's repo, it appears in the repo listing, the
  client's git client fetches it over smart-HTTP using a Personal Access
  Token — and only with that token — and an admin governed-expunge later
  removes one path from history.

  Background:
    Given a client named "Capricorn" <capricorn@example.com> with a matter and a repo access token

  Scenario: A committed document is listed, fetched under a PAT, then expunged
    When the firm commits "wills/draft.md" to the Project repo
    Then "wills/draft.md" appears in the Project repo listing
    Then the repo refuses an anonymous git fetch
    And the repo serves a git fetch to the token holder
    When an admin governed-expunges "wills/draft.md" from the repo
    Then "wills/draft.md" is gone from the Project repo listing
