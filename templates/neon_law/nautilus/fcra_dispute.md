---
title: FCRA Credit-Report Dispute
respondent_type: person
code: nautilus__fcra_dispute
jurisdiction: US
confidential: true
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: custom_single_choice__credit_bureau
  custom_single_choice__credit_bureau:
    _: custom_text__tradeline
  custom_text__tradeline:
    _: custom_text__report_error
  custom_text__report_error:
    _: END
  END: {}
prompts:
  client_name: What is the client's full legal name?
  credit_bureau: Which credit bureau is reporting the error?
  tradeline: Which account on your credit report is wrong?
  report_error: What is wrong with how this account is reported?
choices:
  credit_bureau:
    equifax: Equifax
    experian: Experian
    transunion: TransUnion
    all: All three
workflow:
  BEGIN:
    intake_submitted: document_open__fcra_dispute
  document_open__fcra_dispute:
    pdf_persisted: staff_review
  staff_review:
    approved: mailroom_send__fcra_dispute
    rejected: END
  mailroom_send__fcra_dispute:
    mailed: END
  END: {}
---

To: `{{custom_single_choice__credit_bureau}}` \
Re: `{{person__client.name}}` — disputed account `{{custom_text__tradeline}}`

We represent `{{person__client.name}}` and dispute the accuracy of the account above as it appears on the Client's
credit report.

Under the federal Fair Credit Reporting Act, 15 U.S.C. § 1681i, you must conduct a free, reasonable reinvestigation of
this disputed item and complete it within thirty days of receiving this dispute. The Client states the following is
inaccurate: `{{custom_text__report_error}}`.

Reinvestigate the disputed item, and if it cannot be verified as accurate and complete, delete or correct it and send
the Client written notice of the result. Direct your response to Neon Law. This letter is signed by the attorney of
record for the Client.
