---
title: Northstar Advance Health-Care Directive (stub)
respondent_type: person
code: northstar__directive_health
jurisdiction: NV
confidential: true
prompts:
  testator_name: What is your full legal name?
  healthcare_agent: Who is your health-care agent?
questionnaire:
  BEGIN:
    _: custom_text__testator_name
  custom_text__testator_name:
    _: custom_text__healthcare_agent
  custom_text__healthcare_agent:
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

# Advance Health-Care Directive of {{custom_text__testator_name}}

> **Draft stub.** This is a placeholder instrument generated from the recorded sitting so the plan has a health-care
> directive to review. A licensed Neon Law attorney replaces this body with the full directive before the client sees a
> final draft.

I, `{{custom_text__testator_name}}`, make this advance health-care directive.

## Health-care agent

I appoint `{{custom_text__healthcare_agent}}` as my health-care agent to make medical decisions for me when I cannot
speak for myself, subject to any limits I state to my attorney.
