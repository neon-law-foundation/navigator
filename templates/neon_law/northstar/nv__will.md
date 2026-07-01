---
title: Northstar Will (stub)
respondent_type: person
code: northstar__will
jurisdiction: NV
confidential: true
prompts:
  testator_name: What is your full legal name?
  executor_name: Who is the executor of your will?
  guardian_for_minors: Who do you nominate as guardian for any minor children?
  residuary_beneficiary: Who receives the remainder of your estate?
questionnaire:
  BEGIN:
    _: custom_text__testator_name
  custom_text__testator_name:
    _: custom_text__executor_name
  custom_text__executor_name:
    _: custom_text__guardian_for_minors
  custom_text__guardian_for_minors:
    _: custom_text__residuary_beneficiary
  custom_text__residuary_beneficiary:
    _: END
  END: {}
workflow:
  BEGIN:
    drafted: staff_review
  staff_review:
    released: client_review
    rejected: END
  client_review:
    approved: END
  END: {}
---

# Last Will and Testament of {{custom_text__testator_name}}

> **Draft stub.** This is a placeholder instrument generated from the recorded sitting so the plan has a will to review.
> A licensed Neon Law attorney replaces this body with the full will before the client sees a final draft.

I, `{{custom_text__testator_name}}`, declare this to be my Last Will and Testament, and I revoke all wills and
codicils I have previously made.

## Executor

I name `{{custom_text__executor_name}}` as the executor of this will, to carry it out through probate if probate
is needed.

## Guardian for minor children

I nominate `{{custom_text__guardian_for_minors}}` as guardian of any minor children of mine.

## Residuary estate

I give the remainder of my estate, after any specific gifts, to `{{custom_text__residuary_beneficiary}}`.
