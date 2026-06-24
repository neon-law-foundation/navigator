---
title: California LLC Operating Agreement
respondent_type: entity
code: llc__california
confidential: false
questionnaire:
  BEGIN:
    _: company_name
  company_name:
    _: principal_office
  principal_office:
    _: member_list
  member_list:
    _: END
  END: {}
workflow:
  BEGIN:
    _: member_signatures
  member_signatures:
    _: staff_review
  staff_review:
    _: END
  END: {}
---

Operating agreement for `{{company_name}}`, a California limited liability company with its principal office at
`{{principal_office}}`. The agreement is signed by the members listed in `{{member_list}}`.
