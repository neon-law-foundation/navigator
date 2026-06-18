Feature: Workshop "Using the Navigator to Rapidly Solve Legal Outcomes"

  Every Bloom-tagged claim the workshop README makes about Navigator
  is grounded by an executable scenario in this file. If a scenario
  here breaks, the workshop's prose is stale — the AIDA + engineer
  council insisted on this contract so the page cannot drift away
  from the runtime that backs it.

  The running matter is the one the workshop README names:

    Project   — Henderson Bungalow Purchase
    Buyer     — Virgo (bound to {{client_name}})
    Template  — real_estate__deed_of_sale (markdown body with one
                {{client_name}} placeholder)

  The attorney is the actor in every When step; Navigator is the
  instrument. Scorpio's load-bearing trust claim — the deed is not
  signed until the attorney advances the workflow — is asserted in
  the final scenario.

  Background:
    Given a fresh Navigator app with a deed-of-sale template
    And the workshop attorney "Virgo" is registered with email "virgo@example.com"

  Scenario: Remember — the four Navigator nouns are real schema entities
    Then the schema has a "projects" table
    And the schema has a "templates" table
    And the schema has a "notations" table
    And the schema has a "persons" table

  Scenario: Apply — the attorney opens a Project for the running matter
    When the attorney creates a Project named "Henderson Bungalow Purchase"
    Then a project named "Henderson Bungalow Purchase" exists in the database
    And the project status is "open"

  Scenario: Apply — the attorney binds the deed template as a notation
    When the attorney binds the deed template as a notation
    Then a notation row exists linking the deed template to Virgo
    And the deed template body carries the "{{client_name}}" placeholder

  Scenario: Create — the deed is not signed until the attorney advances the workflow
    # Scorpio's load-bearing trust claim from the engineer-council
    # review: Navigator must never produce a signed deed on its own.
    # Whatever the runtime calls the initial state, it must NOT be
    # `signed`, `notarized`, or `notarization_pending` — those only
    # appear after an explicit workflow advance the attorney drives.
    When the attorney binds the deed template as a notation
    Then the notation state is not "signed"
    And the notation state is not "notarized"
    And the notation state is not "notarization_pending"
