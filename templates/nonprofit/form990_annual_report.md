---
title: IRS Form 990 — Return of Organization Exempt From Income Tax
respondent_type: entity
code: form_990__annual_report
confidential: false
questionnaire:
  BEGIN:
    _: tax_year
  tax_year:
    _: revenue_summary
  revenue_summary:
    _: END
  END: {}
workflow:
  BEGIN:
    _: board_signatures
  board_signatures:
    _: staff_review
  staff_review:
    _: mailroom_send
  mailroom_send:
    _: END
  END: {}
---

IRS Form 990 for `{{entity_name}}` covering tax year `{{tax_year}}`. Summary of gross revenue, program-service expense,
and end-of-year net assets: `{{revenue_summary}}`. The officer signing this return certifies under penalty of perjury
that the return is true, correct, and complete to the best of their knowledge. Filed with the Internal Revenue Service
no later than the 15th day of the 5th month after the close of the tax year.
