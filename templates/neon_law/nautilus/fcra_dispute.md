---
kind: letter
title: FCRA Consumer-Report Dispute
respondent_type: person
code: nautilus__fcra_dispute
jurisdiction: US
confidential: true
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: custom_text__reporting_agency
  custom_text__reporting_agency:
    _: custom_text__disputed_item
  custom_text__disputed_item:
    _: custom_text__report_error
  custom_text__report_error:
    _: END
  END: {}
prompts:
  client_name: What is the client's full legal name?
custom_questions:
  reporting_agency:
    prompt: Which screening or reporting agency reported the error?
  disputed_item:
    prompt: Which item on your report is wrong?
  report_error:
    prompt: What is wrong with how this item is reported?
workflow:
  BEGIN:
    intake_submitted: generate_pdf__fcra_dispute
  generate_pdf__fcra_dispute:
    pdf_persisted: lawyer_review
  lawyer_review:
    approved: mailroom_send__fcra_dispute
    rejected: END
  mailroom_send__fcra_dispute:
    mailed: END
  END: {}
---

To: `{{custom_text__reporting_agency}}` \
Re: `{{person__client.name}}` — disputed item `{{custom_text__disputed_item}}`

We represent `{{person__client.name}}` and dispute the accuracy of the item above as it appears on the Client's consumer
report.

Under the federal Fair Credit Reporting Act, 15 U.S.C. § 1681i, you must conduct a free, reasonable reinvestigation of
this disputed item and complete it within thirty days of receiving this dispute. The Client states the following is
inaccurate: `{{custom_text__report_error}}`.

Reinvestigate the disputed item, and if it cannot be verified as accurate and complete, delete or correct it and send
the Client written notice of the result. Direct your response to Neon Law. This letter is signed by the attorney of
record for the Client.
