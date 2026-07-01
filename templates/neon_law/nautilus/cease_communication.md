---
title: Cease Communication Letter
respondent_type: person
code: nautilus__cease_communication
jurisdiction: US
confidential: true
prompts:
  client_name: What is the client's full legal name?
  collector_name: What is the name of the debt collector contacting you?
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: entity__collector
  entity__collector:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: document_open__cease_communication
  document_open__cease_communication:
    pdf_persisted: staff_review
  staff_review:
    approved: mailroom_send__cease_communication
    rejected: END
  mailroom_send__cease_communication:
    mailed: END
  END: {}
---

To: `{{entity__collector.name}}` \
Re: `{{person__client.name}}`

We represent `{{person__client.name}}`, who has elected to stop your communications about the debt you are
collecting.

Under the federal Fair Debt Collection Practices Act, 15 U.S.C. § 1692c(c), this is written notice that the Client
refuses further communication about this debt. Cease communicating with the Client about it, except as § 1692c(c)
permits — to advise that your collection efforts are ending, or to notify the Client of a specific remedy you may
invoke.

Direct any permitted communication to Neon Law, not to the Client. This letter does not erase the debt; it stops your
communications. This letter is signed by the attorney of record for the Client.
