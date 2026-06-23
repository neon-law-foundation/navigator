---
title: Cease Communication Letter
respondent_type: person
code: nautilus__cease_communication
confidential: true
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: collector_name
  collector_name:
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

To: `{{collector_name}}` \
Re: `{{client_name}}`

We represent `{{client_name}}`, who has elected to stop your communications about the debt you are collecting.

Under the federal Fair Debt Collection Practices Act, 15 U.S.C. § 1692c(c), this is written notice that the Client
refuses further communication about this debt. Cease communicating with the Client about it, except as § 1692c(c)
permits — to advise that your collection efforts are ending, or to notify the Client of a specific remedy you may
invoke.

Direct any permitted communication to Neon Law, not to the Client. This letter does not erase the debt; it stops your
communications. This letter is signed by the attorney of record for the Client.
