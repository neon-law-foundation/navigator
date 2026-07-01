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
    _: person__testator
  person__testator:
    _: person__executor
  person__executor:
    _: person__guardian_for_minors
  person__guardian_for_minors:
    _: person__residuary_beneficiary
  person__residuary_beneficiary:
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

# Last Will and Testament of {{person__testator.name}}

> **Draft stub.** This is a placeholder instrument generated from the recorded sitting so the plan has a will to review.
> A licensed Neon Law attorney replaces this body with the full will before the client sees a final draft.

I, `{{person__testator.name}}`, declare this to be my Last Will and Testament, and I revoke all wills and
codicils I have previously made.

## Executor

I name `{{person__executor.name}}` as the executor of this will, to carry it out through probate if probate
is needed.

## Guardian for minor children

I nominate `{{person__guardian_for_minors.name}}` as guardian of any minor children of mine.

## Residuary estate

I give the remainder of my estate, after any specific gifts, to `{{person__residuary_beneficiary.name}}`.
