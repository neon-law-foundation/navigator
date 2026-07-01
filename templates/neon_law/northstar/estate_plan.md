---
title: Northstar Estate Plan
respondent_type: person
code: onboarding__estate
jurisdiction: NV
confidential: true
questionnaire:
  BEGIN:
    _: custom_yes_no__recording_consent
  custom_yes_no__recording_consent:
    _: person__testator
  person__testator:
    _: person__executor
  person__executor:
    _: person__successor_trustee
  person__successor_trustee:
    _: person__guardian_for_minors
  person__guardian_for_minors:
    _: person__residuary_beneficiary
  person__residuary_beneficiary:
    _: person__healthcare_agent
  person__healthcare_agent:
    _: person__financial_agent
  person__financial_agent:
    _: END
  END: {}
prompts:
  recording_consent: Do you consent to recording this sitting?
  testator_name: What is your full legal name?
  executor_name: Who is the executor of your will?
  successor_trustee: Who is the successor trustee of your trust?
  guardian_for_minors: Who do you nominate as guardian for any minor children?
  residuary_beneficiary: Who receives the remainder of your estate?
  healthcare_agent: Who is your health-care agent?
  financial_agent: Who is your financial agent under a durable power of attorney?
workflow:
  BEGIN:
    transcript_uploaded: document_intake__transcript
  document_intake__transcript:
    transcript_ready: extract__inputs
  extract__inputs:
    inputs_ready: document_drafts__estate
  document_drafts__estate:
    drafts_persisted: staff_review
  staff_review:
    approved: client_review
    rejected: END
  client_review:
    client_approved: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: END
    signature_declined: END
  END: {}
---

# Northstar Estate Plan for {{person__testator.name}}

This is the plan of `{{person__testator.name}}` (the "Client"), prepared by Neon Law from one recorded sitting.
The plan is one flat fee and three instruments: a **will**, a **revocable living trust**, and **health and financial
directives**. This summary names the people the Client chose; the full instruments are generated as separate drafts for
the Client to read and comment on before signing.

The Client confirmed at the start of the recording that the sitting could be recorded:
`{{custom_yes_no__recording_consent}}`.

## The people in this plan

- **Executor of the will** — `{{person__executor.name}}`, who will carry out the will through probate if probate is
  needed.
- **Successor trustee of the trust** — `{{person__successor_trustee.name}}`, who steps in to manage the trust when the
  Client no longer can, so the estate stays out of probate.
- **Guardian for minor children** — `{{person__guardian_for_minors.name}}`, nominated to raise any minor children of the
  Client.
- **Residuary beneficiary** — `{{person__residuary_beneficiary.name}}`, who receives what remains of the estate after
  specific gifts.
- **Health-care agent** — `{{person__healthcare_agent.name}}`, who makes medical decisions under the advance health-care
  directive when the Client cannot speak for themselves.
- **Financial agent** — `{{person__financial_agent.name}}`, who acts under the durable financial power of attorney.

## How the plan is finished

A licensed Neon Law attorney reviews every generated instrument before the Client sees a final draft. The Client then
reads each instrument online, takes the time they need, and leaves comments — nothing is final until they have read it.
When the Client approves, the will, trust, and directives go to electronic signature together.

The Client signs below to adopt this plan and authorize Neon Law to finalize the will, trust, and directives for
execution:

{{client.signature}}

{{client.date}}
