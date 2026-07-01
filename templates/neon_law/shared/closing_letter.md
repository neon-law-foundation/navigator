---
title: Closing Letter
respondent_type: person_and_entity
code: closing__letter
jurisdiction: NV
confidential: true
prompts:
  client_name: What is the client's full legal name?
  project_name: What is the project name for this engagement?
  matter_summary: Summarize the matter and the work the firm completed.
  fee_status: What is the fee status as the matter closes?
  file_retention: How will the client's file be retained or returned?
  next_obligation: What is the client's next obligation or deadline, if any?
choices:
  fee_status:
    paid_in_full: Paid in full
    balance_due: Balance due
    waived: Fees waived
questionnaire:
  BEGIN:
    _: custom_text__client_name
  custom_text__client_name:
    _: custom_text__project_name
  custom_text__project_name:
    _: custom_text__matter_summary
  custom_text__matter_summary:
    _: custom_single_choice__fee_status
  custom_single_choice__fee_status:
    _: custom_text__file_retention
  custom_text__file_retention:
    _: custom_text__next_obligation
  custom_text__next_obligation:
    _: END
  END: {}
workflow:
  BEGIN:
    close_requested: staff_review
  staff_review:
    approved: document_open__closing_letter
    rejected: END
  document_open__closing_letter:
    pdf_persisted: firm_signature__closing_letter
  firm_signature__closing_letter:
    signed: END
  END: {}
---

This letter confirms that Neon Law (the "Firm") has completed its work for `{{custom_text__client_name}}` (the "Client")
on the matter referred to as `{{custom_text__project_name}}`, and that the Firm's representation of the Client on this
matter is now concluded.

Summary of the work completed: `{{custom_text__matter_summary}}`.

Fee status at closing: `{{custom_single_choice__fee_status}}`. The Client remains responsible only for fees and expenses
already incurred and invoiced on this matter; closing the matter itself adds no further charge.

The Client's file will be handled as follows: `{{custom_text__file_retention}}`. The Client may request a copy of the
file during the retention period at no additional cost.

Next steps that belong to the Client: `{{custom_text__next_obligation}}`. The Firm will take no further action on this
matter. Should a new need arise, the Client is welcome to open a new matter with the Firm at any time.

It has been our privilege to do this work alongside you. This letter is signed on behalf of the Firm by the Neon Law
staff member of record for the matter.
