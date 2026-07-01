---
title: Settlement Letter
respondent_type: person
code: nautilus__settlement_letter
jurisdiction: US
confidential: true
prompts:
  client_name: What is the client's full legal name?
  collector_name: What is the name of the debt collector contacting you?
  settlement_target: What amount are you offering to settle this debt for?
  settlement_terms: What terms do you want on the settlement?
  client_authorization: Do you authorize Neon Law to send this settlement offer on these terms?
questionnaire:
  BEGIN:
    _: custom_text__client_name
  custom_text__client_name:
    _: custom_text__collector_name
  custom_text__collector_name:
    _: custom_text__settlement_target
  custom_text__settlement_target:
    _: custom_text__settlement_terms
  custom_text__settlement_terms:
    _: custom_yes_no__client_authorization
  custom_yes_no__client_authorization:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: document_open__settlement_letter
  document_open__settlement_letter:
    pdf_persisted: sent_for_signature__settlement
  sent_for_signature__settlement:
    client_authorized: staff_review
    client_declined: END
  staff_review:
    approved: mailroom_send__settlement_letter
    rejected: END
  mailroom_send__settlement_letter:
    mailed: END
  END: {}
---

To: `{{custom_text__collector_name}}` \
Re: `{{custom_text__client_name}}` — settlement offer

We represent `{{custom_text__client_name}}` and write, at the Client's direction, to offer settlement of the debt you
are collecting.

The Client offers to resolve this account for `{{custom_text__settlement_target}}`, on the following terms:
`{{custom_text__settlement_terms}}`.
This offer is made for settlement purposes. If you accept, confirm the terms in writing to this office before any
payment is made, and report the account consistent with the agreed terms.

This settlement is directed by the Client. The firm's fee for representing the Client is a flat monthly fee; the firm
takes no percentage of any amount the Client saves. Direct your response to Neon Law. This letter is signed by the
attorney of record for the Client.

The Client authorizes Neon Law to send this settlement offer on the Client's behalf:

{{client.signature}}

{{client.date}}
