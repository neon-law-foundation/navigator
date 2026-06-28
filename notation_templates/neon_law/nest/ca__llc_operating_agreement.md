---
title: California LLC Operating Agreement
respondent_type: entity
code: ca__llc_operating_agreement
jurisdiction: CA
confidential: false
questionnaire:
  BEGIN:
    _: entity__company
  entity__company:
    _: address__principal_office
  address__principal_office:
    _: people__members
  people__members:
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

Operating agreement for `{{entity__company}}`, a California limited liability company with its principal office at
`{{address__principal_office}}`. The agreement is signed by the members listed in `{{people__members}}`.
