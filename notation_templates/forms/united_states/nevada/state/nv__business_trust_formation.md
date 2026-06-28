---
title: Neon Law Nest — Nevada Business Trust Formation
respondent_type: person_and_entity
code: nv__business_trust_formation
jurisdiction: NV
origin_url: https://www.nvsos.gov/businesses/commercial-recordings/forms-fees/all-business-forms
confidential: false
form: nv__business_trust_formation
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: client_email
  client_email:
    _: entity_name
  entity_name:
    _: registered_agent
  registered_agent:
    _: trustees
  trustees:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: intake_persisted__trustee
  intake_persisted__trustee:
    certificate_rendered: staff_review
  staff_review:
    approved: document_open__certificate_pdf
    rejected: END
  document_open__certificate_pdf:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: filing__nv_sos
    signature_declined: END
  filing__nv_sos:
    filed: END
  END: {}
---

This Nevada entity formation engagement (the "Engagement") forms `{{entity_name}}`, a Nevada business trust, for
`{{client_name}}`. Neon Law's flat Nest fee is **\$1,111 per year**. That fee covers the Certificate of Business Trust,
the Initial List of Trustees, and the State Business License application filed with the Nevada Secretary of State,
together with the trust's registered agent of record, `{{registered_agent}}`.

The trustees of the business trust:

`{{trustees}}`

The first trustee listed signs the Certificate of Business Trust, and the certificate prints up to two trustees.

Your answers above are placed onto the Secretary of State's own formation packet — the same official form the state
publishes — and a licensed Neon Law attorney reviews the **filled packet** before anything is signed or filed. Nothing
reaches a government office unreviewed. The first trustee signs below and the firm countersigns; Neon Law then files the
packet with the Nevada Secretary of State and returns the stamped formation record. Confirmations go to
`{{client_email}}`.

{{client.signature}}

{{client.date}}

{{firm.signature}}

{{firm.date}}
