---
title: Notice of Representation
respondent_type: person
code: nautilus__notice_of_representation
jurisdiction: US
confidential: true
questionnaire:
  BEGIN:
    _: custom_text__client_name
  custom_text__client_name:
    _: custom_text__client_email
  custom_text__client_email:
    _: custom_text__collector_name
  custom_text__collector_name:
    _: address__collector_address
  address__collector_address:
    _: custom_text__alleged_account
  custom_text__alleged_account:
    _: custom_yes_no__consent_to_represent
  custom_yes_no__consent_to_represent:
    _: END
  END: {}
prompts:
  client_name: What is the client's full legal name?
  client_email: What is the client's email address?
  collector_name: What is the name of the debt collector contacting you?
  alleged_account: What account or reference number is the collector using?
  consent_to_represent: Do you authorize Neon Law to represent you in communications about this debt?
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

To: `{{custom_text__collector_name}}` \
`{{address__collector_address}}`

Re: `{{custom_text__client_name}}` — account reference `{{custom_text__alleged_account}}`

This letter is formal notice that Neon Law represents `{{custom_text__client_name}}` (the "Client") with respect to the
debt you are
attempting to collect under the account reference above.

Under the federal Fair Debt Collection Practices Act, 15 U.S.C. § 1692c(a)(2), once you know a consumer is represented
by an attorney with respect to a debt, and you have the attorney's name and address, you must direct your communications
to the attorney rather than to the consumer. By this letter you have that knowledge and our contact information.

Accordingly, direct all further communication about this debt to Neon Law, not to the Client. We are reviewing the
account on the Client's behalf and will respond on the matters that require a response.

This letter is signed by the attorney of record for the Client at Neon Law.
