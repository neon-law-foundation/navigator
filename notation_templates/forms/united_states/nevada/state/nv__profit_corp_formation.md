---
title: Neon Law Nest — Nevada Profit Corporation Formation
respondent_type: person_and_entity
code: nv__profit_corp_formation
jurisdiction: NV
origin_url: https://www.nvsos.gov/businesses/commercial-recordings/forms-fees/all-business-forms
confidential: false
form: nv__profit_corp_formation
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
    _: directors
  directors:
    _: corporate_officers
  corporate_officers:
    _: shares_authorized
  shares_authorized:
    _: par_value
  par_value:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: intake_persisted__incorporator
  intake_persisted__incorporator:
    articles_rendered: staff_review
  staff_review:
    approved: document_open__articles_pdf
    rejected: END
  document_open__articles_pdf:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: filing__nv_sos
    signature_declined: END
  filing__nv_sos:
    filed: END
  END: {}
---

This Nevada entity formation engagement (the "Engagement") incorporates `{{entity_name}}`, a Nevada profit corporation,
for `{{client_name}}` (the "Incorporator"). Neon Law's flat Nest fee is **\$1,111 per year**. That fee covers the
Articles of Incorporation, the Initial List of Officers, and the State Business License application filed with the
Nevada Secretary of State, together with the corporation's registered agent of record, `{{registered_agent}}`.

The corporation's board of directors:

`{{directors}}`

Its officers, as reported on the Initial List:

`{{corporate_officers}}`

The corporation is authorized to issue `{{shares_authorized}}` shares at a par value of \$`{{par_value}}` per share. The
first director listed signs the Articles of Incorporation as the Incorporator.

Your answers above are placed onto the Secretary of State's own formation packet — the same official form the state
publishes — and a licensed Neon Law attorney reviews the **filled packet** before anything is signed or filed. Nothing
reaches a government office unreviewed. The Incorporator signs below and the firm countersigns; Neon Law then files the
packet with the Nevada Secretary of State and returns the stamped formation record. Confirmations go to
`{{client_email}}`.

{{client.signature}}

{{client.date}}

{{firm.signature}}

{{firm.date}}
