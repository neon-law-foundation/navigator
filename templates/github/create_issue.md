---
kind: github
title: Create a GitHub Issue
questionnaire:
  BEGIN:
    _: custom_single_choice__change_surface
  custom_single_choice__change_surface:
    _: custom_yes_no__engineering_council
  custom_yes_no__engineering_council:
    _: custom_text__observed_problem
  custom_text__observed_problem:
    _: custom_text__grounded_scope
  custom_text__grounded_scope:
    _: custom_text__acceptance_criteria
  custom_text__acceptance_criteria:
    _: custom_text__covering_tests
  custom_text__covering_tests:
    _: custom_text__blast_radius
  custom_text__blast_radius:
    _: END
  END: {}
custom_questions:
  change_surface:
    prompt: What does this change touch?
    choices:
      web: Web feature
      api: API feature
      infrastructure: Infrastructure
      form: Government form
  engineering_council:
    prompt: >-
      Should the Engineering Council convene before this work starts? Say yes for an
      architecture decision, a cross-cutting refactor, or a call about how to sequence
      the work; no for a change whose shape is already settled.
  observed_problem:
    prompt: >-
      What is actually happening today? Describe the behavior you observed, not the fix
      you have in mind.
  grounded_scope:
    prompt: >-
      What is in scope, and what is deliberately out? Name the boundary so the pull
      request that closes this issue has one concern.
  acceptance_criteria:
    prompt: >-
      How will we know this is done? Write the conditions someone else could check
      without asking you.
  covering_tests:
    prompt: >-
      Which test proves it? Name the test that fails today and passes when the work
      lands, or the one to be written.
  blast_radius:
    prompt: >-
      Which real files does this touch? List the paths you have actually read, not the
      ones you expect to exist.
workflow:
  BEGIN:
    issue_requested: github_issue__engineering
  github_issue__engineering:
    issue_opened: END
  END: {}
---

## Observed problem

{{custom_text__observed_problem}}

## Scope

{{custom_text__grounded_scope}}

Change surface: **{{custom_single_choice__change_surface}}**

## Acceptance criteria

{{custom_text__acceptance_criteria}}

## Covering tests

{{custom_text__covering_tests}}

## Blast radius

{{custom_text__blast_radius}}

## Engineering Council

Convene before work starts: {{custom_yes_no__engineering_council}}
