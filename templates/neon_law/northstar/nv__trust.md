---
title: Northstar Revocable Living Trust (stub)
respondent_type: person
code: northstar__trust
jurisdiction: NV
confidential: true
prompts:
  testator_name: What is your full legal name?
  successor_trustee: Who is the successor trustee of your trust?
  residuary_beneficiary: Who receives the remainder of your estate?
questionnaire:
  BEGIN:
    _: person__testator
  person__testator:
    _: person__successor_trustee
  person__successor_trustee:
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

# Revocable Living Trust of {{person__testator.name}}

> **Draft stub.** This is a placeholder instrument generated from the recorded sitting so the plan has a trust to
> review. A licensed Neon Law attorney replaces this body with the full revocable living trust before the client sees a
> final draft.

This Revocable Living Trust is established by `{{person__testator.name}}` (the "Grantor"), who is the initial
trustee during the Grantor's lifetime while able to serve.

## Successor trustee

`{{person__successor_trustee.name}}` shall serve as successor trustee, stepping in to manage the trust when the Grantor
no longer can, so the estate stays out of probate.

## Distribution

On the Grantor's death, the trustee distributes the remaining trust estate to `{{person__residuary_beneficiary.name}}`,
after any specific gifts.
