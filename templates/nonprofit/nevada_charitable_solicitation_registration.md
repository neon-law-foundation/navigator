---
title: Nevada Charitable Solicitation Registration
respondent_type: entity
code: charitable_solicitation_registration__nevada
confidential: false
questionnaire:
  BEGIN:
    _: annual_or_amended
  annual_or_amended:
    _: fundraising_activities
  fundraising_activities:
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

Nevada Charitable Solicitation Registration Statement for `{{entity_name}}` filed with the Secretary of State for the
period ending `{{annual_or_amended}}`. The organization's fundraising activities during the period are:
`{{fundraising_activities}}`. This registration is required of any nonprofit that solicits contributions from Nevada
residents, and is renewed annually.
