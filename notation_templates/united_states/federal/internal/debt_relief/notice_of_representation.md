---
title: Notice of Representation
respondent_type: person
code: nautilus__notice_of_representation
confidential: true
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: client_email
  client_email:
    _: collector_name
  collector_name:
    _: collector_address
  collector_address:
    _: alleged_account
  alleged_account:
    _: consent_to_represent
  consent_to_represent:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: document_open__notice_of_representation
  document_open__notice_of_representation:
    pdf_persisted: staff_review
  staff_review:
    approved: mailroom_send__notice_of_representation
    rejected: END
  mailroom_send__notice_of_representation:
    mailed: END
  END: {}
---

To: `{{collector_name}}` \
`{{collector_address}}`

Re: `{{client_name}}` — account reference `{{alleged_account}}`

This letter is formal notice that Neon Law represents `{{client_name}}` (the "Client") with respect to the debt you are
attempting to collect under the account reference above.

Under the federal Fair Debt Collection Practices Act, 15 U.S.C. § 1692c(a)(2), once you know a consumer is represented
by an attorney with respect to a debt, and you have the attorney's name and address, you must direct your communications
to the attorney rather than to the consumer. By this letter you have that knowledge and our contact information.

Accordingly, direct all further communication about this debt to Neon Law, not to the Client. We are reviewing the
account on the Client's behalf and will respond on the matters that require a response.

This letter is signed by the attorney of record for the Client at Neon Law.
