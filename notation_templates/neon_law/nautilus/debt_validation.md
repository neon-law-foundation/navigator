---
title: Debt Validation Request
respondent_type: person
code: nautilus__debt_validation
jurisdiction: US
confidential: true
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: collector_name
  collector_name:
    _: alleged_account
  alleged_account:
    _: original_creditor
  original_creditor:
    _: disputed_reason
  disputed_reason:
    _: END
  END: {}
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

To: `{{collector_name}}` \
Re: `{{client_name}}` — account reference `{{alleged_account}}`

We represent `{{client_name}}` with respect to the debt above and write to dispute it and to demand validation.

Under the federal Fair Debt Collection Practices Act, 15 U.S.C. § 1692g, the Client disputes this debt. This written
dispute is made within the thirty-day period that § 1692g(a) provides. Under § 1692g(b), you must now cease collection
of this debt until you obtain verification and mail a copy of that verification to the Client through this office.

Please mail verification of the debt, including the amount claimed, an itemization of how it was calculated, and the
name of the original creditor — stated here by the Client as `{{original_creditor}}`. The Client disputes the debt on
the following basis: `{{disputed_reason}}`.

Direct all communication about this debt to Neon Law. This letter is signed by the attorney of record for the Client.
