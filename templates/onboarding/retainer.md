---
title: Retainer Agreement
respondent_type: person_and_entity
code: onboarding__retainer
confidential: true
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: client_email
  client_email:
    _: project_name
  project_name:
    _: product_description
  product_description:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: intake_persisted__client
  intake_persisted__client:
    retainer_rendered: staff_review
  staff_review:
    approved: document_open__retainer_pdf
    rejected: END
  document_open__retainer_pdf:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: END
    signature_declined: END
  END: {}
---

This Engagement Agreement (the "Agreement") is entered into between Neon Law (the "Firm") and `{{client_name}}` (the
"Client"), reachable at `{{client_email}}`, for legal services rendered on the matter referred to as `{{project_name}}`.

The Firm will provide the following services: `{{product_description}}`. Fees are billed monthly against the rate sheet
attached to this Agreement; expenses are passed through at cost.

**Scope of the engagement.** The Firm's representation is limited to the services described above and in the clauses of
this Agreement. Work outside that scope — including any new matter, dispute, or proceeding — requires a separate written
engagement or a written amendment to this one signed by both the Client and the Firm.

Either party may terminate this Agreement upon written notice. The Client remains responsible for fees and expenses
incurred prior to termination.

{{custom_clauses}}

**Firm-wide conflicts.** Neon Law is a small firm, and we treat a conflict for any one of our attorneys as a conflict
for the entire firm. Before we take on a new matter, we check it against all of our current and former matters across
every attorney here. If that check turns up a conflict we cannot properly take on — for example, where the matter would
have the Firm representing a business and an individual whose interests are adverse to each other, or would place the
Firm adverse to a current or former client — we will tell you promptly, decline the matter rather than wall it off
internally, refer you to outside counsel, and return any materials you shared with us. The Firm neither pays nor accepts
a referral fee on any matter it refers out. By engaging us, you acknowledge that our attorneys share matter information
among themselves for this purpose.

**Your file, kept for ten years.** The Firm keeps your complete matter file — every document, signed agreement, and the
privileged correspondence we exchange with you — for ten years after your matter closes. You may request a copy of your
file at any point during that period. After ten years, the Firm securely destroys the file and its contents.

The Client acknowledges receipt of the Firm's privacy notice and agrees to electronic delivery of invoices and case
correspondence at `{{client_email}}`.

The Client and the Firm execute this Agreement electronically as of the dates signed below.

{{client.signature}}

{{client.date}}

By initialing here, the Client acknowledges that this engagement covers the flat-fee transactional and document work
described above and does not include litigation, courtroom, or contested-hearing representation; any such matter
requires a separate written engagement with the Firm or a referral to outside counsel: {{client.initials}}

{{firm.signature}}

{{firm.date}}
