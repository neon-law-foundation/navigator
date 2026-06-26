---
title: Nonprofit At-Will Employment Agreement (W-2)
code: employment__nonprofit_w2
respondent_type: person
confidential: true
questionnaire:
  BEGIN:
    _: nonprofit_legal_name
  nonprofit_legal_name:
    _: nonprofit_state
  nonprofit_state:
    _: worker_legal_name
  worker_legal_name:
    _: worker_title
  worker_title:
    _: worker_duties
  worker_duties:
    _: engagement_start_date
  engagement_start_date:
    _: annual_salary
  annual_salary:
    _: pay_schedule
  pay_schedule:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: intake_persisted__worker
  intake_persisted__worker:
    rendered: staff_review
  staff_review:
    approved: document_open__agreement
    rejected: END
  document_open__agreement:
    pdf_persisted: END
  END: {}
---

# At-Will Employment Agreement

This Employment Agreement (this "Agreement") is between `{{nonprofit_legal_name}}`, a nonprofit corporation organized
under the laws of the State of `{{nonprofit_state}}` (the "Organization"), and `{{worker_legal_name}}` (the "Employee").
The Organization and the Employee agree as follows.

## 1. Position and duties

The Organization employs the Employee as `{{worker_title}}`, beginning on `{{engagement_start_date}}`. The Employee's
duties are: `{{worker_duties}}`. The Employee will report to the Organization's board of directors or its designee and
will perform the duties faithfully, competently, and in the Organization's best interest.

## 2. At-will employment

The Employee's employment is **at will**. Either the Organization or the Employee may end the employment at any time,
for any reason or no reason, with or without cause and with or without notice. Nothing in this Agreement, and nothing in
any handbook, policy, or statement, creates a contract of employment for any fixed term or limits the at-will
relationship. **Only a writing signed by an authorized officer of the Organization** can change the at-will nature of
this employment.

## 3. Compensation and tax treatment

The Organization will pay the Employee an annual base salary of `{{annual_salary}}`, paid `{{pay_schedule}}` and subject
to all required payroll withholding. The Organization will treat the Employee as a **W-2 employee**: it will withhold
income and employment taxes, pay the employer's share of employment taxes, and report the Employee's wages on **IRS Form
W-2**. Eligibility for any benefit plan the Organization may offer is governed by the terms of that plan.

## 4. Confidentiality

The Employee will keep the Organization's confidential information — donor and personnel records, financial data, and
anything not public — in confidence during and after employment, and will use it only for the Organization's purposes.

## 5. Work product

Work the Employee creates within the scope of employment belongs to the Organization. The Employee assigns that work
product to the Organization and will sign documents reasonably needed to confirm the Organization's ownership.

## 6. Compliance

The Employee will comply with the Organization's lawful policies and with applicable law, including the Organization's
conflict of interest policy.

## 7. General

This Agreement is governed by the laws of the State of `{{nonprofit_state}}`. It is the entire agreement between the
parties about the Employee's employment and supersedes any prior understanding. If any provision is held unenforceable,
the rest remains in effect.

## Signatures

**`{{nonprofit_legal_name}}`**

By: ______________________________  Date: ______________

Name: ______________________________

Title: ______________________________

**Employee**

______________________________  Date: ______________

`{{worker_legal_name}}`
