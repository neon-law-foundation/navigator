---
title: FCRA Credit-Report Dispute
respondent_type: person
code: nautilus__fcra_dispute
confidential: true
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: credit_bureau
  credit_bureau:
    _: tradeline
  tradeline:
    _: report_error
  report_error:
    _: END
  END: {}
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

To: `{{credit_bureau}}` \
Re: `{{client_name}}` — disputed account `{{tradeline}}`

We represent `{{client_name}}` and dispute the accuracy of the account above as it appears on the Client's credit
report.

Under the federal Fair Credit Reporting Act, 15 U.S.C. § 1681i, you must conduct a free, reasonable reinvestigation of
this disputed item and complete it within thirty days of receiving this dispute. The Client states the following is
inaccurate: `{{report_error}}`.

Reinvestigate the disputed item, and if it cannot be verified as accurate and complete, delete or correct it and send
the Client written notice of the result. Direct your response to Neon Law. This letter is signed by the attorney of
record for the Client.
