---
title: Nevada Modified Business Tax Return
respondent_type: entity
code: nv_state_tax_filing__modified_business_tax
confidential: true
questionnaire:
  BEGIN:
    _: tax_year
  tax_year:
    _: gross_revenue
  gross_revenue:
    _: END
  END: {}
workflow:
  BEGIN:
    _: member_signatures
  member_signatures:
    _: staff_review
  staff_review:
    _: mailroom_send
  mailroom_send:
    _: END
  END: {}
---

Nevada Modified Business Tax Return for `{{entity_name}}` covering tax year `{{tax_year}}`. Total Nevada gross revenue
for the period is `{{gross_revenue}}`. The signing member certifies under penalty of perjury that this return is true,
correct, and complete to the best of their knowledge.
