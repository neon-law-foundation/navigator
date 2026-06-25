Feature: Workshop "Deploy the Navigator"

  Every renderable claim the "Deploy the Navigator" workshop (DEPLOY.md)
  makes is grounded by a scenario here. If one breaks, the workshop's
  prose is stale — the same contract the sibling
  workshop_navigator_walkthrough.feature carries.

  This feature owns the half that needs the running web app: the
  workshop is registered on the Foundation surface, renders under the
  Foundation brand, opens with an agenda, splits into stepped content,
  and shows the reader the real provisioning command. The other half —
  that the services, buckets, and command the prose names match what
  `navigator gcp setup` actually calls — is asserted next to the code in
  `cli/src/devx/gcp/mod.rs::deploy_workshop_prose_matches_the_dry_run_pipeline`,
  the only place `cli`'s `devx::gcp::run` is reachable. The two halves together
  are the cross-reference.

  The workshop is donated open-source infrastructure addressed to a
  deployer standing up their own instance — never to a legal client. It
  therefore lives under the Foundation brand, away from the firm's
  client intake surface.

  Background:
    Given the "Deploy the Navigator" workshop is loaded from the content directory

  Scenario: Remember — the workshop is registered on the Foundation surface
    When a reader visits "/foundation/nebula/workshops/deploy-the-navigator"
    Then the response status is 200
    And the page title is "Neon Law Foundation | Deploy the Navigator"
    And the page shows no "not accepting clients" banner

  Scenario: Understand — the agenda opens a stepped walkthrough
    Then the workshop's first section is titled "Agenda"
    And the workshop splits into at least 7 sections
    And the rendered body carries no duplicate top-level heading

  Scenario: Apply — the workshop shows the reader the real provisioning command
    Then the rendered workshop shows the command "cargo run -p cli -- gcp setup --project-id"
    And the rendered workshop shows the "--dry-run" flag

  Scenario: Verify — the markdown twin serves raw markdown
    When a reader visits "/foundation/nebula/workshops/deploy-the-navigator.md"
    Then the response status is 200
    And the response content-type is "text/markdown; charset=utf-8"
    And the markdown twin contains "## Agenda"
