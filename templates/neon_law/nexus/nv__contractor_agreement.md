---
kind: agreement
title: Nonprofit Independent Contractor Agreement (1099)
code: contractor__nonprofit_1099
jurisdiction: NV
respondent_type: person
confidential: true
questionnaire:
  BEGIN:
    _: entity__nonprofit
  entity__nonprofit:
    _: jurisdiction__nonprofit
  jurisdiction__nonprofit:
    _: person__worker
  person__worker:
    _: custom_text__worker_duties
  custom_text__worker_duties:
    _: custom_datetime__engagement_start_date
  custom_datetime__engagement_start_date:
    _: custom_text__contractor_term
  custom_text__contractor_term:
    _: custom_text__contractor_rate
  custom_text__contractor_rate:
    _: custom_text__termination_notice_days
  custom_text__termination_notice_days:
    _: END
  END: {}
prompts:
  nonprofit_legal_name: What is the full legal name of the nonprofit organization?
  nonprofit_state: In which U.S. state is the nonprofit incorporated?
  worker_legal_name: What is the worker's full legal name?
  worker_title: What is the position or title?
custom_questions:
  worker_duties:
    prompt: Summarize the duties or scope of work.
  engagement_start_date:
    prompt: What is the start date?
  contractor_term:
    prompt: What is the term of the engagement?
  contractor_rate:
    prompt: What is the contractor's compensation?
  termination_notice_days:
    prompt: How many days' written notice may either party give to end the engagement?
workflow:
  BEGIN:
    intake_submitted: intake_persisted__worker
  intake_persisted__worker:
    rendered: lawyer_review
  lawyer_review:
    approved: generate_pdf__agreement
    rejected: END
  generate_pdf__agreement:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: END
    signature_declined: END
  END: {}
---

# Independent Contractor Agreement

This Independent Contractor Agreement (this "Agreement") is between `{{entity__nonprofit.name}}`, a
nonprofit corporation organized under the laws of the State of `{{jurisdiction__nonprofit.name}}` (the "Organization"),
and `{{person__worker.name}}` (the "Contractor"). The Organization and the Contractor agree as follows.

## 1. Services

The Contractor will provide the following services in the role of `{{person__worker.title}}`:
`{{custom_text__worker_duties}}`. The Contractor controls the manner and means by which the services are performed and
supplies the Contractor's own tools and work methods.

## 2. Independent contractor status

The Contractor is an **independent contractor**, not an employee, partner, or agent of the Organization. Consistent with
that status:

- The Organization will report payments to the Contractor on **IRS Form 1099-NEC** and will **not** withhold income or
  employment taxes. The Contractor is solely responsible for the Contractor's own income, self-employment, and other
  taxes.
- The Contractor is **not** eligible for employee benefits, paid leave, workers' compensation, or unemployment insurance
  through the Organization.
- The Contractor has no authority to bind the Organization or to act on its behalf except as the Organization expressly
  authorizes in writing.

The parties intend a true independent-contractor relationship and will conduct themselves accordingly.

## 3. Term

This engagement begins on `{{custom_datetime__engagement_start_date}}` and continues `{{custom_text__contractor_term}}`.

## 4. Compensation

The Organization will pay the Contractor `{{custom_text__contractor_rate}}`. The Contractor will submit invoices for
services performed, and the Organization will pay undisputed invoices within thirty (30) days of receipt.

## 5. Termination

Either party may end this engagement, for convenience, on `{{custom_text__termination_notice_days}}` days' written
notice. On termination, the Organization will pay the Contractor for services properly performed through the termination
date.

## 6. Confidentiality

The Contractor will keep the Organization's confidential information — donor and personnel records, financial data, and
anything not public — in confidence during and after the engagement, and will use it only to perform the services.

## 7. Work product

Work product the Contractor creates in performing the services belongs to the Organization. The Contractor assigns that
work product to the Organization and will sign documents reasonably needed to confirm the Organization's ownership.

## 8. General

This Agreement is governed by the laws of the State of `{{jurisdiction__nonprofit.name}}`. It is the entire agreement
between the parties about these services and supersedes any prior understanding. If any provision is held
unenforceable, the rest remains in effect.

## Signatures

**`{{entity__nonprofit.name}}`**

By: ______________________________  Date: ______________

Name: ______________________________

Title: ______________________________

**Contractor**

The Contractor signs this Agreement electronically; the Organization's authorized officer signs above.

{{client.signature}}

{{client.date}}

`{{person__worker.name}}`
