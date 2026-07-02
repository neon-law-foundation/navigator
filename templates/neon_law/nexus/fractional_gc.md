---
# STUB TEMPLATE — the questionnaire + workflow below are the real, tested contract.
# The prose body is a minimal placeholder so the ongoing fractional-GC engagement can be
# exercised end to end (intake → staff review → signature); the full engagement-letter
# clauses get filled in later WITHOUT touching the flow. The questionnaire is the contract;
# the body is replaceable.
title: Neon Law Nexus — Fractional General Counsel
respondent_type: person_and_entity
code: onboarding__nexus
jurisdiction: NV
confidential: true
prompts:
  client_name: What is the client's full legal name?
  entity_name: What is the legal name of your LLC?
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: entity__company
  entity__company:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: intake_persisted__client
  intake_persisted__client:
    engagement_rendered: staff_review
  staff_review:
    approved: document_open__engagement_pdf
    rejected: END
  document_open__engagement_pdf:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: END
    signature_declined: END
  END: {}
---

This engagement letter retains Neon Law as fractional general counsel for `{{entity__company.name}}` (the
"Company"), the ongoing legal partner for `{{person__client.name}}`. Neon Law Nexus is a flat **\$2,222 per
month**. It is a continuing relationship, not a single matter: routine contracts, corporate housekeeping, and the
day-to-day legal questions a growing company runs into, with a licensed attorney in the loop for anything that needs
legal judgment.

The Company described the scope it needs covered.
Project-specific scope is recorded in the custom clauses below.
Work product is delivered into
the
Company's Project repository, and questions are answered through the Company's support thread — the ongoing record of
the engagement lives in both.

A licensed Neon Law attorney reviews this engagement before it is countersigned. The Company signs below and the firm
countersigns; either side may end the monthly engagement on thirty days' notice.

{{client.signature}}

{{client.date}}

{{firm.signature}}

{{firm.date}}
