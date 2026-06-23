---
title: Closing Letter
respondent_type: person_and_entity
code: closing__letter
confidential: true
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: project_name
  project_name:
    _: matter_summary
  matter_summary:
    _: fee_status
  fee_status:
    _: file_retention
  file_retention:
    _: next_obligation
  next_obligation:
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

This letter confirms that Neon Law (the "Firm") has completed its work for `{{client_name}}` (the "Client") on the
matter referred to as `{{project_name}}`, and that the Firm's representation of the Client on this matter is now
concluded.

Summary of the work completed: `{{matter_summary}}`.

Fee status at closing: `{{fee_status}}`. The Client remains responsible only for fees and expenses already incurred and
invoiced on this matter; closing the matter itself adds no further charge.

The Client's file will be handled as follows: `{{file_retention}}`. The Client may request a copy of the file during the
retention period at no additional cost.

Next steps that belong to the Client: `{{next_obligation}}`. The Firm will take no further action on this matter. Should
a new need arise, the Client is welcome to open a new matter with the Firm at any time.

It has been our privilege to do this work alongside you. This letter is signed on behalf of the Firm by the Neon Law
staff member of record for the matter.
