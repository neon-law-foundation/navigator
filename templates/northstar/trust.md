---
title: Northstar Revocable Living Trust (stub)
respondent_type: person
code: northstar__trust
confidential: true
questionnaire:
  BEGIN:
    _: testator_name
  testator_name:
    _: successor_trustee
  successor_trustee:
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

# Revocable Living Trust of {{testator_name}}

> **Draft stub.** This is a placeholder instrument generated from the recorded sitting so the plan has a trust to
> review. A licensed Neon Law attorney replaces this body with the full revocable living trust before the client sees a
> final draft.

This Revocable Living Trust is established by `{{testator_name}}` (the "Grantor"), who is the initial trustee during the
Grantor's lifetime while able to serve.

## Successor trustee

`{{successor_trustee}}` shall serve as successor trustee, stepping in to manage the trust when the Grantor no longer
can, so the estate stays out of probate.

## Distribution

On the Grantor's death, the trustee distributes the remaining trust estate to `{{residuary_beneficiary}}`, after any
specific gifts.
