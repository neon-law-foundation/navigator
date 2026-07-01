---
title: Nonprofit At-Will Employment Agreement (W-2)
code: employment__nonprofit_w2
jurisdiction: NV
respondent_type: person
confidential: true
prompts:
  nonprofit_legal_name: What is the full legal name of the nonprofit organization?
  nonprofit_state: In which U.S. state is the nonprofit incorporated?
  worker_legal_name: What is the worker's full legal name?
  worker_title: What is the position or title?
  worker_duties: Summarize the duties or scope of work.
  engagement_start_date: What is the start date?
  annual_salary: What is the annual base salary?
  pay_schedule: How often is the employee paid?
questionnaire:
  BEGIN:
    _: custom_text__nonprofit_legal_name
  custom_text__nonprofit_legal_name:
    _: custom_text__nonprofit_state
  custom_text__nonprofit_state:
    _: custom_text__worker_legal_name
  custom_text__worker_legal_name:
    _: custom_text__worker_title
  custom_text__worker_title:
    _: custom_text__worker_duties
  custom_text__worker_duties:
    _: custom_datetime__engagement_start_date
  custom_datetime__engagement_start_date:
    _: custom_text__annual_salary
  custom_text__annual_salary:
    _: custom_text__pay_schedule
  custom_text__pay_schedule:
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

This Employment Agreement (this "Agreement") is between `{{custom_text__nonprofit_legal_name}}`, a nonprofit
corporation organized under the laws of the State of `{{custom_text__nonprofit_state}}` (the "Organization"), and
`{{custom_text__worker_legal_name}}` (the "Employee"). The Organization and the Employee agree as follows.

## 1. Position and duties

The Organization employs the Employee as `{{custom_text__worker_title}}`, beginning on
`{{custom_datetime__engagement_start_date}}`. The Employee's duties are: `{{custom_text__worker_duties}}`. The Employee
will report to the Organization's board of directors or its designee and will perform the duties faithfully,
competently, and in the Organization's best interest.

## 2. At-will employment

The Employee's employment is **at will**. Either the Organization or the Employee may end the employment at any time,
for any reason or no reason, with or without cause and with or without notice. Nothing in this Agreement, and nothing in
any handbook, policy, or statement, creates a contract of employment for any fixed term or limits the at-will
relationship. **Only a writing signed by an authorized officer of the Organization** can change the at-will nature of
this employment.

## 3. Compensation and tax treatment

The Organization will pay the Employee an annual base salary of `{{custom_text__annual_salary}}`, paid
`{{custom_text__pay_schedule}}` and subject to all required payroll withholding. The Organization will treat the
Employee as a **W-2 employee**: it will withhold
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

This Agreement is governed by the laws of the State of `{{custom_text__nonprofit_state}}`. It is the entire agreement
between the parties about the Employee's employment and supersedes any prior understanding. If any provision is held
unenforceable,
the rest remains in effect.

## Signatures

**`{{custom_text__nonprofit_legal_name}}`**

By: ______________________________  Date: ______________

Name: ______________________________

Title: ______________________________

**Employee**

______________________________  Date: ______________

`{{custom_text__worker_legal_name}}`
