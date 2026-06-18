---
title: Simple Last Will and Testament
respondent_type: person
code: will__simple
confidential: true
questionnaire:
  BEGIN:
    _: testator_name
  testator_name:
    _: executor_name
  executor_name:
    _: residuary_beneficiary
  residuary_beneficiary:
    _: END
  END: {}
workflow:
  BEGIN:
    _: testator_signature
  testator_signature:
    _: witnesses
  witnesses:
    _: staff_review
  staff_review:
    _: notarization
  notarization:
    _: END
  END: {}
---

I, `{{testator_name}}`, declare this to be my Last Will and Testament. I name `{{executor_name}}` as the executor of my
estate and direct that the residue pass to `{{residuary_beneficiary}}`.
