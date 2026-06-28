---
title: Northstar Estate Plan
respondent_type: person
code: onboarding__estate
jurisdiction: NV
confidential: true
questionnaire:
  BEGIN:
    _: recording_consent
  recording_consent:
    _: testator_name
  testator_name:
    _: executor_name
  executor_name:
    _: successor_trustee
  successor_trustee:
    _: guardian_for_minors
  guardian_for_minors:
    _: residuary_beneficiary
  residuary_beneficiary:
    _: healthcare_agent
  healthcare_agent:
    _: financial_agent
  financial_agent:
    _: END
  END: {}
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

# Northstar Estate Plan for {{testator_name}}

This is the plan of `{{testator_name}}` (the "Client"), prepared by Neon Law from one recorded sitting. The plan is one
flat fee and three instruments: a **will**, a **revocable living trust**, and **health and financial directives**. This
summary names the people the Client chose; the full instruments are generated as separate drafts for the Client to read
and comment on before signing.

The Client confirmed at the start of the recording that the sitting could be recorded: `{{recording_consent}}`.

## The people in this plan

- **Executor of the will** — `{{executor_name}}`, who will carry out the will through probate if probate is needed.
- **Successor trustee of the trust** — `{{successor_trustee}}`, who steps in to manage the trust when the Client no
  longer can, so the estate stays out of probate.
- **Guardian for minor children** — `{{guardian_for_minors}}`, nominated to raise any minor children of the Client.
- **Residuary beneficiary** — `{{residuary_beneficiary}}`, who receives what remains of the estate after specific gifts.
- **Health-care agent** — `{{healthcare_agent}}`, who makes medical decisions under the advance health-care directive
  when the Client cannot speak for themselves.
- **Financial agent** — `{{financial_agent}}`, who acts under the durable financial power of attorney.

## How the plan is finished

A licensed Neon Law attorney reviews every generated instrument before the Client sees a final draft. The Client then
reads each instrument online, takes the time they need, and leaves comments — nothing is final until they have read it.
When the Client approves, the will, trust, and directives go to electronic signature together.

The Client signs below to adopt this plan and authorize Neon Law to finalize the will, trust, and directives for
execution:

{{client.signature}}

{{client.date}}
