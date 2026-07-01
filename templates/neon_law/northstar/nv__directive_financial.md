---
title: Northstar Durable Financial Power of Attorney (stub)
respondent_type: person
code: northstar__directive_financial
jurisdiction: NV
confidential: true
prompts:
  testator_name: What is your full legal name?
  financial_agent: Who is your financial agent under a durable power of attorney?
questionnaire:
  BEGIN:
    _: custom_text__testator_name
  custom_text__testator_name:
    _: custom_text__financial_agent
  custom_text__financial_agent:
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

# Durable Financial Power of Attorney of {{custom_text__testator_name}}

> **Draft stub.** This is a placeholder instrument generated from the recorded sitting so the plan has a financial
> directive to review. A licensed Neon Law attorney replaces this body with the full durable power of attorney before
> the client sees a final draft.

I, `{{custom_text__testator_name}}`, make this durable financial power of attorney.

## Financial agent

I appoint `{{custom_text__financial_agent}}` as my agent to act on my financial affairs under this durable power of
attorney, subject to any limits I state to my attorney.
