---
title: Northstar Will (stub)
respondent_type: person
code: northstar__will
confidential: true
questionnaire:
  BEGIN:
    _: testator_name
  testator_name:
    _: executor_name
  executor_name:
    _: guardian_for_minors
  guardian_for_minors:
    _: residuary_beneficiary
  residuary_beneficiary:
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

# Last Will and Testament of {{testator_name}}

> **Draft stub.** This is a placeholder instrument generated from the recorded sitting so the plan has a will to review.
> A licensed Neon Law attorney replaces this body with the full will before the client sees a final draft.

I, `{{testator_name}}`, declare this to be my Last Will and Testament, and I revoke all wills and codicils I have
previously made.

## Executor

I name `{{executor_name}}` as the executor of this will, to carry it out through probate if probate is needed.

## Guardian for minor children

I nominate `{{guardian_for_minors}}` as guardian of any minor children of mine.

## Residuary estate

I give the remainder of my estate, after any specific gifts, to `{{residuary_beneficiary}}`.
