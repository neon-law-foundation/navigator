---
title: Nevada Nonprofit Articles of Incorporation (501(c)(3))
respondent_type: entity
code: nonprofit_501c3_formation__nevada
confidential: false
questionnaire:
  BEGIN:
    _: custom_text__mission_statement
  custom_text__mission_statement:
    _: people__board_members
  people__board_members:
    _: registered_agent
  registered_agent:
    _: END
  END: {}
prompts:
  mission_statement: What is the mission statement?
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

Articles of Incorporation for `{{entity_name}}`, a Nevada nonprofit corporation organized exclusively for charitable,
educational, and scientific purposes within the meaning of Section 501(c)(3) of the Internal Revenue Code. Mission:
`{{custom_text__mission_statement}}`. The initial board of directors consists of `{{people__board_members}}`. The
corporation's registered agent in Nevada is `{{registered_agent}}`. On dissolution, remaining assets pass to another
501(c)(3) organization or to the federal government for a public purpose.
