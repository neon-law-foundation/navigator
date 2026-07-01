---
title: Debt Validation Request
respondent_type: person
code: nautilus__debt_validation
jurisdiction: US
confidential: true
questionnaire:
  BEGIN:
    _: custom_text__client_name
  custom_text__client_name:
    _: custom_text__collector_name
  custom_text__collector_name:
    _: custom_text__alleged_account
  custom_text__alleged_account:
    _: custom_text__original_creditor
  custom_text__original_creditor:
    _: custom_text__disputed_reason
  custom_text__disputed_reason:
    _: END
  END: {}
prompts:
  client_name: What is the client's full legal name?
  collector_name: What is the name of the debt collector contacting you?
  alleged_account: What account or reference number is the collector using?
  original_creditor: Who is the original creditor, if you know?
  disputed_reason: What do you dispute about this debt?
workflow:
  BEGIN:
    intake_submitted: document_open__debt_validation
  document_open__debt_validation:
    pdf_persisted: staff_review
  staff_review:
    approved: mailroom_send__debt_validation
    rejected: END
  mailroom_send__debt_validation:
    mailed: END
  END: {}
---

To: `{{custom_text__collector_name}}` \
Re: `{{custom_text__client_name}}` — account reference `{{custom_text__alleged_account}}`

We represent `{{custom_text__client_name}}` with respect to the debt above and write to dispute it and to demand
validation.

Under the federal Fair Debt Collection Practices Act, 15 U.S.C. § 1692g, the Client disputes this debt. This written
dispute is made within the thirty-day period that § 1692g(a) provides. Under § 1692g(b), you must now cease collection
of this debt until you obtain verification and mail a copy of that verification to the Client through this office.

Please mail verification of the debt, including the amount claimed, an itemization of how it was calculated, and the
name of the original creditor — stated here by the Client as `{{custom_text__original_creditor}}`. The Client disputes
the debt on the following basis: `{{custom_text__disputed_reason}}`.

Direct all communication about this debt to Neon Law. This letter is signed by the attorney of record for the Client.
