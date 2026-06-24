---
title: Nevada Annual List of Managers, Members, and Registered Agent
respondent_type: entity
code: annual_report__nevada
confidential: false
questionnaire:
  BEGIN:
    _: annual_or_amended
  annual_or_amended:
    _: managers_list
  managers_list:
    _: END
  END: {}
workflow:
  BEGIN:
    _: staff_review
  staff_review:
    _: mailroom_send
  mailroom_send:
    _: END
  END: {}
---

Annual List for `{{entity_name}}`, filed with the Nevada Secretary of State for the period ending
`{{annual_or_amended}}`. The current managers and members of the company are: `{{managers_list}}`. The registered agent
remains the one of record unless updated by a separate filing.
