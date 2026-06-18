---
title: Nevada Nonprofit Articles of Incorporation (501(c)(3))
respondent_type: entity
code: nonprofit_501c3_formation__nevada
confidential: false
questionnaire:
  BEGIN:
    _: mission_statement
  mission_statement:
    _: board_members
  board_members:
    _: registered_agent
  registered_agent:
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

Articles of Incorporation for `{{entity_name}}`, a Nevada nonprofit corporation organized exclusively for charitable,
educational, and scientific purposes within the meaning of Section 501(c)(3) of the Internal Revenue Code. Mission:
`{{mission_statement}}`. The initial board of directors consists of `{{board_members}}`. The corporation's registered
agent in Nevada is `{{registered_agent}}`. On dissolution, remaining assets pass to another 501(c)(3) organization or to
the federal government for a public purpose.
