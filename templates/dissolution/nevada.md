---
title: Nevada LLC Articles of Dissolution
respondent_type: entity
code: dissolution__nevada
confidential: false
questionnaire:
  BEGIN:
    _: dissolution_reason
  dissolution_reason:
    _: final_debts_settled
  final_debts_settled:
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

Articles of Dissolution for `{{entity_name}}`, a Nevada limited liability company. The members have voted to dissolve
the company for the following reason: `{{dissolution_reason}}`. All debts and obligations of the company have been
settled or otherwise resolved per `{{final_debts_settled}}`. The company directs the Nevada Secretary of State to enter
the dissolution in its records.
